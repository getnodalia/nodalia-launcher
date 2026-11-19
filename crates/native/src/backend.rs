//! [`NativeBackend`]: the desktop implementation of the UI's [`Backend`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use nodalia_launcher_core::catalog::{LAUNCHER, LAUNCHER_ID, SUITE};
use nodalia_launcher_core::platform::{Arch, Os, Platform, pick_asset};
use nodalia_launcher_core::protocol::{
    Backend, BackendOptions, CANCELLED, Capabilities, Command, Event, INSTALL_CANCELLED, JobPhase, LatestRelease, PAUSED, PausedDownload, Source,
    VersionTrack,
};
use nodalia_launcher_core::release::{Asset, Release};
use nodalia_launcher_core::{Installed, PackageKind, SuiteApp, version};

use crate::http::{self, Http, Stop};
use crate::install::{self, Job};
use crate::manifest::Manifest;
use crate::paths::Paths;
use crate::process;
use crate::source::{GitHubSource, ReleaseSource};

/// The longest the launcher's own update waits for other installs to end before it gives up
/// (it can be cancelled sooner).
const RESTART_WAIT: Duration = Duration::from_secs(10 * 60);
/// Automatic checks closer together than this reuse the last answers. The UI asks every five
/// minutes; this stops a burst of requests (refresh, focus, timers) from repeating work.
const FRESH_FOR: i64 = 4 * 60;
/// GitHub is asked about an app at most this often (pressing Refresh asks right away). The
/// answers are revalidated with ETags, so most asks cost no API quota.
const GITHUB_EVERY: i64 = 10 * 60;
/// Requests in flight at once during a check.
const WORKERS: usize = 4;

type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct DownloadSize {
    revision: u64,
    plan: Option<(PackageKind, u64)>,
}

impl DownloadSize {
    fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.plan = None;
    }
}

struct Shared {
    events: Mutex<Vec<Event>>,
    /// Held while an app's latest release or installed copy changes and its [`Event::Installable`]
    /// is worked out and queued, so no answer from older state lands after a newer one. Always
    /// taken first, before any other lock here.
    reporting: Mutex<()>,
    /// Held while a report of what an app's install would download is worked out and sent
    /// ([`report_partial`]), so one worked out from older settings or state can't land after a newer
    /// one. Taken before `reporting`.
    planning: Mutex<()>,
    /// What this system could install, and when that was asked ([`Shared::system_kinds`]).
    system: Mutex<Option<(Instant, install::SystemKinds)>>,
    waker: Waker,
    latest: Mutex<HashMap<String, Release>>,
    download_sizes: Mutex<HashMap<String, DownloadSize>>,
    installed: Mutex<HashMap<String, Installed>>,
    jobs: Mutex<HashMap<String, Arc<Stop>>>,
    /// The launcher is replacing itself and will restart: no new job starts.
    restarting: AtomicBool,
    manifest: Mutex<Manifest>,
    options: Mutex<BackendOptions>,
    checking: AtomicBool,
    /// A detection pass is running; another request waits for the next one.
    detecting: AtomicBool,
    last_check: Mutex<Option<i64>>,
    /// The newer Nodalia Launcher release, once a check has found one, with the repo that check
    /// read it from (the feed's rename, if any): its download links must be that repo's.
    launcher_release: Mutex<Option<(Release, String)>>,
    /// How this launcher is installed, when it can replace itself. Worked out on the check thread
    /// just before a newer launcher is announced: finding out can ask the host's Flatpak, which
    /// mustn't hold up the first frame.
    self_install: OnceLock<Option<crate::self_update::Install>>,
    /// Whether this copy can update itself ([`crate::self_update::is_self_updating`]: on Windows,
    /// only a signed copy). Worked out with `self_install`.
    self_updating: OnceLock<bool>,
    /// When GitHub was last asked about each app (Unix seconds), to keep the fallback rare.
    github_asked: Mutex<HashMap<String, i64>>,
    /// Apps whose details pictures are on their way (a second request meanwhile does nothing).
    pictures_busy: Mutex<std::collections::HashSet<&'static str>>,
}

#[derive(Clone, Copy)]
enum BusyFlag {
    Checking,
    Detecting,
}

struct BusyGuard {
    shared: Arc<Shared>,
    flag: BusyFlag,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        let flag = match self.flag {
            BusyFlag::Checking => &self.shared.checking,
            BusyFlag::Detecting => &self.shared.detecting,
        };
        flag.store(false, Ordering::SeqCst);
    }
}

struct JobGuard {
    shared: Arc<Shared>,
    app_id: String,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.shared.jobs.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.app_id);
    }
}

/// How long the installability answer trusts what this system could install ([`Shared::system_kinds`]).
const SYSTEM_KINDS_FOR: Duration = Duration::from_secs(60);

impl Shared {
    fn store_latest(&self, app: &str, release: &Release) {
        let mut latest = self.latest.lock().unwrap_or_else(PoisonError::into_inner);
        if latest.get(app) != Some(release) {
            latest.insert(app.to_string(), release.clone());
            self.invalidate_download_size(app);
        }
    }

    fn store_detected(&self, app: &str, found: Option<&Installed>) {
        let mut installed = self.installed.lock().unwrap_or_else(PoisonError::into_inner);
        if installed.get(app) != found {
            match found {
                Some(i) => installed.insert(app.to_string(), i.clone()),
                None => installed.remove(app),
            };
            self.invalidate_download_size(app);
        }
    }

    fn invalidate_download_size(&self, app: &str) {
        self.download_sizes.lock().unwrap_or_else(PoisonError::into_inner).entry(app.to_string()).or_default().invalidate();
    }

    fn download_revision(&self, app: &str) -> u64 {
        self.download_sizes.lock().unwrap_or_else(PoisonError::into_inner).entry(app.to_string()).or_default().revision
    }

    fn report_download(&self, app: &str, revision: u64, plan: Option<(PackageKind, u64)>, download: Option<PausedDownload>, ready: bool) -> bool {
        {
            let mut sizes = self.download_sizes.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(cached) = sizes.get_mut(app).filter(|cached| cached.revision == revision) else { return false };
            cached.plan = plan;
            let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
            events.push(Event::Paused { app: app.to_string(), download });
            events.push(Event::ReadyToInstall { app: app.to_string(), ready });
        }
        (self.waker)();
        true
    }

    /// What this system can install, asked again at most once a [`SYSTEM_KINDS_FOR`]: inside the
    /// Flatpak sandbox asking runs a program on the host (up to 3 s), and the installability answer
    /// is worked out for each installed app on every check. An install still asks afresh. Always
    /// asked before `reporting` is taken. [`report_partial`] asks it holding `planning`, which only
    /// its own worker threads take.
    fn system_kinds(&self) -> install::SystemKinds {
        if let Some((at, kinds)) = self.system.lock().unwrap_or_else(PoisonError::into_inner).as_ref()
            && at.elapsed() < SYSTEM_KINDS_FOR
        {
            return kinds.clone();
        }
        let kinds = install::SystemKinds::now();
        *self.system.lock().unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), kinds.clone()));
        kinds
    }

    /// What this system could install when last asked, however long ago, for an answer worked out
    /// holding `reporting`: no probe runs there. A copy only enters `installed` through
    /// [`publish_detected`], which asks [`Self::system_kinds`] first, so with a copy installed
    /// there is always a view; without one (never in practice) nothing installs.
    fn last_system_kinds(&self) -> install::SystemKinds {
        self.system
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map_or_else(|| install::SystemKinds { available: Vec::new(), can_pacman: false }, |(_, kinds)| kinds.clone())
    }

    /// Whether some copy of `app` is installed (as last detected).
    fn has_copy(&self, app: &SuiteApp) -> bool {
        self.installed.lock().unwrap_or_else(PoisonError::into_inner).contains_key(app.id)
    }

    fn emit(&self, event: Event) {
        self.emit_all([event]);
    }

    /// Queues `events` together: a poll sees all of them or none.
    fn emit_all(&self, events: impl IntoIterator<Item = Event>) {
        self.events.lock().unwrap_or_else(PoisonError::into_inner).extend(events);
        (self.waker)();
    }
}

pub struct NativeBackend {
    shared: Arc<Shared>,
    paths: Paths,
    http: Option<Http>,
    source: Option<Arc<dyn ReleaseSource>>,
    platform: Option<Platform>,
    /// Runs a job's worker. Each worker gets its own thread; a test swaps this to see what is in
    /// the download line at the moment each one would start.
    spawn: Spawn,
}

/// Runs a job's worker (see [`NativeBackend::spawn`]).
type Spawn = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

impl NativeBackend {
    /// `waker` is called (from any thread) whenever events are waiting; it should repaint the UI.
    pub fn new(waker: impl Fn() + Send + Sync + 'static) -> Self {
        let waker: Waker = Arc::new(waker);
        let paths = Paths::new().unwrap_or_else(|e| {
            log::error!("{e}; keeping files in the temporary folder");
            Paths::under(&std::env::temp_dir().join("nodalia-launcher"))
        });
        let http = match Http::new() {
            Ok(h) => Some(h),
            Err(e) => {
                log::error!("HTTPS unavailable: {e}");
                None
            }
        };
        let source: Option<Arc<dyn ReleaseSource>> =
            http.clone().map(|h| Arc::new(GitHubSource::new(h, paths.release_cache())) as Arc<dyn ReleaseSource>);
        let shared = Arc::new(Shared {
            events: Mutex::new(Vec::new()),
            reporting: Mutex::new(()),
            system: Mutex::new(None),
            planning: Mutex::new(()),
            waker,
            latest: Mutex::new(HashMap::new()),
            download_sizes: Mutex::new(HashMap::new()),
            installed: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            restarting: AtomicBool::new(false),
            manifest: Mutex::new(Manifest::load(&paths.manifest())),
            options: Mutex::new(BackendOptions::default()),
            checking: AtomicBool::new(false),
            detecting: AtomicBool::new(false),
            launcher_release: Mutex::new(None),
            self_install: OnceLock::new(),
            self_updating: OnceLock::new(),
            github_asked: Mutex::new(HashMap::new()),
            pictures_busy: Mutex::new(std::collections::HashSet::new()),
            last_check: Mutex::new(None),
        });
        let spawn: Spawn = Arc::new(|work| {
            std::thread::spawn(work);
        });
        let backend = Self { shared, paths, http, source, platform: native_platform(), spawn };
        backend.replay_cache();
        backend.detect_all();
        backend
    }

    /// Shows the saved answers from the last run straight away, before any network request.
    fn replay_cache(&self) {
        let pre = self.options().prereleases;
        let Some(source) = &self.source else { return };
        for app in SUITE.iter().filter(|a| self.platform.is_none_or(|p| a.runs_on(p.os))) {
            if let Some(latest) = source.cached(app.repo, pre) {
                publish_latest(app, latest, &self.shared, self.platform);
            }
        }
    }

    /// Works out again, off the UI thread, what each app's install would download, and tells the
    /// UI ([`report_partial`]): inside the Flatpak sandbox that asks the host (up to 3 s an app).
    fn replan(&self) {
        let shared = self.shared.clone();
        let paths = self.paths.clone();
        let platform = self.platform;
        std::thread::spawn(move || {
            for app in SUITE.iter().filter(|a| platform.is_none_or(|p| a.runs_on(p.os))) {
                report_partial(app, &shared, &paths, platform);
            }
        });
    }

    fn options(&self) -> BackendOptions {
        self.shared.options.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Scans for installed Nodalia apps and paused downloads on a background thread.
    fn detect_all(&self) {
        if self.shared.detecting.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = self.shared.clone();
        let busy = BusyGuard { shared: shared.clone(), flag: BusyFlag::Detecting };
        let paths = self.paths.clone();
        let platform = self.platform;
        std::thread::spawn(move || {
            let busy = busy;
            // Leftovers from interrupted installs or older versions of the launcher. A paused
            // download stays a month; a finished one kept after a failed install, a week.
            http::clean_downloads(&paths.downloads(), Duration::from_secs(30 * 86_400), Duration::from_secs(7 * 86_400));
            let snapshot = crate::detect::Snapshot::take();
            let manifest = shared.manifest.lock().unwrap_or_else(PoisonError::into_inner).clone();
            for app in SUITE.iter().filter(|a| platform.is_none_or(|p| a.runs_on(p.os))) {
                let found = crate::detect::detect(app, &manifest, &snapshot);
                publish_detected(app, found, &shared, platform);
            }
            // Downloads paused (or cut off by quitting) in an earlier run.
            for app in SUITE.iter().filter(|a| platform.is_none_or(|p| a.runs_on(p.os))) {
                report_partial(app, &shared, &paths, platform);
            }
            drop(busy);
        });
    }

    /// Looks up the latest releases on GitHub, at most once per [`GITHUB_EVERY`] per app unless
    /// `force`.
    fn check(&self, force: bool) {
        let (Some(source), Some(_http)) = (self.source.clone(), self.http.clone()) else {
            for app in SUITE.iter().filter(|a| self.platform.is_none_or(|p| a.runs_on(p.os))) {
                self.shared
                    .emit(Event::Latest { app: app.id.to_string(), latest: LatestRelease::Failed("secure connections aren't available".into()) });
            }
            return;
        };
        let now = crate::now();
        let recent = self.shared.last_check.lock().unwrap_or_else(PoisonError::into_inner).is_some_and(|t| now - t < FRESH_FOR);
        if (recent && !force) || self.shared.checking.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = self.shared.clone();
        let busy = BusyGuard { shared: shared.clone(), flag: BusyFlag::Checking };
        let paths = self.paths.clone();
        let platform = self.platform;
        let options = self.options();
        std::thread::spawn(move || {
            let busy = busy;
            shared.emit(Event::CheckStarted);
            let now = crate::now();
            let queue = Mutex::new(
                SUITE.iter().filter(|a| platform.is_none_or(|p| a.runs_on(p.os))).chain(std::iter::once(&LAUNCHER)).collect::<Vec<&SuiteApp>>(),
            );
            let sources = sweep(&queue, &source, &shared, (platform, &options), force, now);
            source.save();
            // A newer release changes which file an install would use.
            for app in SUITE.iter().filter(|a| platform.is_none_or(|p| a.runs_on(p.os))) {
                report_partial(app, &shared, &paths, platform);
            }
            let source_used = source_used_of(&sources);
            let at = crate::now();
            *shared.last_check.lock().unwrap_or_else(PoisonError::into_inner) = Some(at);
            let rate_limited_until = limit_notice(source_used, source.rate_limited_until());
            drop(busy);
            shared.emit(Event::CheckFinished { at, source: source_used, rate_limited_until });
        });
    }

    /// Starts the job, or returns `false` without starting one: the app is unknown, doesn't run
    /// on this platform, is already running a job, or the launcher is restarting into an update.
    fn start_job(&self, app_id: &str, work: Work) -> bool {
        let Some(spawn) = self.prepare_job(app_id, work) else { return false };
        spawn();
        true
    }

    /// Gets a job ready without running it: the checks of [`Self::start_job`], the app's entry in
    /// the running jobs and, for an install, its place in the download line. Returns what starts
    /// its thread, or `None` where `start_job` would return `false`; dropping it instead gives the
    /// entry and the place back. A batch prepares every member before starting any, so no worker
    /// can reach a free download slot ahead of a row above it that hasn't taken its place yet.
    fn prepare_job(&self, app_id: &str, work: Work) -> Option<Box<dyn FnOnce() + Send>> {
        let uninstall = matches!(work, Work::Uninstall);
        let app = SuiteApp::by_id(app_id)?;
        if self.platform.is_some_and(|p| !app.runs_on(p.os)) {
            return None;
        }
        log::info!("{}: starting {} job", app.name, if uninstall { "uninstall" } else { "install/update" });
        let stop = Arc::new(Stop::default());
        let restarting = {
            let mut jobs = self.shared.jobs.lock().unwrap_or_else(PoisonError::into_inner);
            if jobs.contains_key(app.id) {
                return None;
            }
            let restarting = self.shared.restarting.load(Ordering::SeqCst);
            if !restarting {
                jobs.insert(app.id.to_string(), stop.clone());
            }
            restarting
        };
        if restarting {
            let result = Err("Nodalia Launcher is restarting to finish its update. Try again once it's back.".into());
            self.shared.emit(Event::JobFinished { app: app.id.to_string(), result });
            return None;
        }
        let shared = self.shared.clone();
        let job = JobGuard { shared: shared.clone(), app_id: app.id.to_string() };
        let paths = self.paths.clone();
        let http = self.http.clone();
        let platform = self.platform;
        let options = self.options();
        let source = self.source.clone();
        // Taken now, before the job's thread runs, so apps started together (Update all) download
        // in screen order whichever job reaches its download first.
        let place = (!uninstall).then(|| install::DOWNLOADS.join(app.id));
        let spawn = self.spawn.clone();
        Some(Box::new(move || {
            spawn(Box::new(move || {
                let job = job;
                let existing = shared.installed.lock().unwrap_or_else(PoisonError::into_inner).get(app.id).cloned();
                let record = shared.manifest.lock().unwrap_or_else(PoisonError::into_inner).apps.get(app.id).cloned();
                let result = if uninstall {
                    shared.emit(Event::Progress { app: app.id.to_string(), phase: JobPhase::Removing });
                    let mut phase = |phase| shared.emit(Event::Progress { app: app.id.to_string(), phase });
                    match &existing {
                        Some(inst) => install::uninstall(app, inst, record.as_ref(), &paths, &stop, &mut phase).map(|()| {
                            let mut m = shared.manifest.lock().unwrap_or_else(PoisonError::into_inner);
                            if m.apps.remove(app.id).is_some() {
                                let _ = m.save(&paths.manifest());
                            }
                            format!("{} was removed", app.name)
                        }),
                        None => Err(format!("{} isn't installed", app.name)),
                    }
                } else {
                    let (approved, batch) = match &work {
                        Work::Replace(path) => (Some(path.as_str()), None),
                        Work::Install(batch) => (None, batch.clone()),
                        Work::Uninstall => (None, None),
                    };
                    let r = run_install(
                        app,
                        &shared,
                        &paths,
                        http.as_ref(),
                        platform,
                        &options,
                        existing.as_ref(),
                        record.as_ref(),
                        &stop,
                        approved,
                        source.as_deref(),
                        batch,
                        place,
                    );
                    // Stopped by the connection rather than by the user: say so, and how to go on.
                    match r {
                        Err(e) if e == PAUSED && stop.check().is_ok() => {
                            Err("The download paused because the connection keeps dropping. Press Resume to continue where it stopped.".into())
                        }
                        other => other,
                    }
                };
                // Look again: the result is whatever is on disk now.
                let manifest = shared.manifest.lock().unwrap_or_else(PoisonError::into_inner).clone();
                let found = crate::detect::detect(app, &manifest, &crate::detect::Snapshot::take());
                // One installer call can cover several apps, and fail after some of them went in.
                let latest = shared.latest.lock().unwrap_or_else(PoisonError::into_inner).get(app.id).cloned();
                let result = match (result, latest) {
                    (Err(e), Some(release))
                        if !uninstall && release.version.as_ref().is_some_and(|v| landed_despite(&e, existing.as_ref(), found.as_ref(), v)) =>
                    {
                        log::warn!("{}: {e}; the new version is installed all the same", app.id);
                        Ok(installed_text(app, existing.is_some(), &release))
                    }
                    (result, _) => result,
                };
                // The manifest may change the package plan even when detection finds the same copy.
                shared.invalidate_download_size(app.id);
                publish_detected(app, found, &shared, platform);
                report_partial(app, &shared, &paths, platform);
                drop(job);
                shared.emit(Event::JobFinished { app: app.id.to_string(), result });
            }));
        }))
    }

    /// Starts one install per app, together (see [`Command::InstallAll`]): where the platform
    /// needs a permission prompt, the batch shares one — each member waits for the others'
    /// downloads before the system installer is asked once. Apps unknown or already running a
    /// job are left out; a batch of one installs the same as a single [`Command::Install`].
    fn install_all(&self, apps: &[String]) {
        let running = self.shared.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let mut ids: Vec<&str> = Vec::new();
        for id in apps {
            let Some(app) = SuiteApp::by_id(id) else { continue };
            if running.contains_key(app.id) || ids.contains(&app.id) {
                continue;
            }
            ids.push(app.id);
        }
        drop(running);
        #[cfg(any(target_os = "linux", windows))]
        let batch = (!ids.is_empty()).then(|| install::INSTALLER.gather(ids.len()));
        #[cfg(not(any(target_os = "linux", windows)))]
        let batch = None;
        // Every member takes its place in the download line before any worker starts.
        let ready: Vec<_> = ids.iter().filter_map(|id| self.prepare_job(id, Work::Install(batch.clone()))).collect();
        let started = ready.len();
        for spawn in ready {
            spawn();
        }
        // Members that could not start after all (another job raced in for them) are not coming:
        // their places in the batch are given back, so the rest don't wait for them.
        for _ in started..ids.len() {
            if let Some(batch) = &batch {
                batch.leave();
            }
        }
    }

    /// What updating this launcher to the newer release takes, when this copy can replace itself
    /// ([`crate::self_update::is_self_updating`]) and the release has the file the update
    /// downloads. Otherwise the UI offers the download.
    fn launcher_update_job(&self) -> Option<(crate::self_update::Install, Release, String, Platform)> {
        let install = self.shared.self_install.get().cloned().flatten()?;
        let platform = self.platform?;
        if !self.shared.self_updating.get().copied().unwrap_or(false) {
            return None;
        }
        let (release, repo) = self.shared.launcher_release.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
        crate::self_update::installs_from(&install, &release, platform).then_some((install, release, repo, platform))
    }

    /// Downloads the newer launcher, puts it in place and asks the UI to quit so it can start.
    fn update_launcher(&self) {
        let id = LAUNCHER_ID.to_string();
        let refuse = || {
            self.shared.emit(Event::JobFinished {
                app: id.clone(),
                result: Err("this copy of Nodalia Launcher can't update itself; download the new version instead".into()),
            });
        };
        if self.shared.self_install.get().cloned().flatten().is_none() || self.platform.is_none() {
            return refuse();
        }
        if self.shared.launcher_release.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            return;
        }
        let Some((install, release, repo, platform)) = self.launcher_update_job() else { return refuse() };
        let stop = Arc::new(Stop::default());
        {
            let mut jobs = self.shared.jobs.lock().unwrap_or_else(PoisonError::into_inner);
            if jobs.contains_key(&id) {
                return;
            }
            jobs.insert(id.clone(), stop.clone());
        }
        let job = JobGuard { shared: self.shared.clone(), app_id: id.clone() };
        let (shared, paths, http) = (self.shared.clone(), self.paths.clone(), self.http.clone());
        std::thread::spawn(move || {
            let job = job;
            let mut last_emit = Instant::now() - Duration::from_secs(1);
            let mut phase = |p: JobPhase| {
                if matches!(p, JobPhase::Downloading { done, total } if done != total && done != 0) && last_emit.elapsed() < Duration::from_millis(50)
                {
                    return;
                }
                last_emit = Instant::now();
                shared.emit(Event::Progress { app: LAUNCHER_ID.to_string(), phase: p });
            };
            let last = || {
                let mut waiting = || shared.emit(Event::Progress { app: LAUNCHER_ID.to_string(), phase: JobPhase::Waiting });
                claim_restart(&shared.jobs, &shared.restarting, &stop, RESTART_WAIT, &mut waiting)
            };
            let result = match http.as_ref() {
                Some(http) => crate::self_update::update(&install, &release, &repo, platform, http, &paths, &stop, &mut phase, &last),
                None => Err("secure connections aren't available on this system".into()),
            };
            drop(job);
            match result {
                Ok(()) => shared.emit(Event::LauncherRestarting),
                Err(e) => {
                    shared.restarting.store(false, Ordering::SeqCst);
                    shared.emit(Event::JobFinished { app: LAUNCHER_ID.to_string(), result: Err(e) });
                }
            }
        });
    }

    fn installed(&self, app: &str) -> Option<Installed> {
        self.shared.installed.lock().unwrap_or_else(PoisonError::into_inner).get(app).cloned()
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Work {
    /// Install or update. Carries the batch this job is part of, if any (see
    /// [`Command::InstallAll`]): where the platform needs a permission prompt, the batch shares one.
    Install(Option<crate::queue::Gather>),
    /// Install over the path the user agreed to overwrite (see [`Command::Replace`]).
    Replace(String),
    Uninstall,
}

fn release_matches_track(release: &Release, track: &VersionTrack, prereleases: bool) -> bool {
    match track {
        VersionTrack::Stable => prereleases || (!release.prerelease && release.version.as_ref().is_some_and(version::is_stable)),
        VersionTrack::Nightly => nodalia_launcher_core::release::is_nightly(release),
        VersionTrack::Pinned(tag) => &release.tag == tag,
    }
}

/// Whether `chosen` is a version picked by name that is older than the installed copy.
fn is_chosen_downgrade(track: &VersionTrack, chosen: &Release, existing: Option<&Installed>) -> bool {
    let VersionTrack::Pinned(tag) = track else { return false };
    let (Some(chosen_v), Some(installed_v)) = (chosen.version.as_ref(), existing.and_then(|i| i.version.as_ref())) else { return false };
    chosen.tag == *tag && version::is_newer(installed_v, chosen_v)
}

/// For a version chosen by name that is older than the installed copy: the installed version's own
/// published release, to put back if the downgrade fails. `None` for anything else, without
/// asking `list`. An error means the downgrade can't be made safe, so it must not start.
fn downgrade_previous(
    app: &SuiteApp,
    track: &VersionTrack,
    chosen: &Release,
    existing: Option<&Installed>,
    list: impl FnOnce() -> Result<Vec<Release>, String>,
) -> Result<Option<Release>, String> {
    let Some(installed_v) = existing.and_then(|i| i.version.as_ref()).filter(|_| is_chosen_downgrade(track, chosen, existing)) else {
        return Ok(None);
    };
    let releases =
        list().map_err(|e| format!("couldn't find {} {installed_v} to put back if going back fails ({e}), so nothing was changed.", app.name))?;
    releases.into_iter().find(|r| r.version.as_ref() == Some(installed_v)).map(Some).ok_or_else(|| {
        format!("{} {installed_v} isn't published any more, so it couldn't be put back if going back fails. Nothing was changed.", app.name)
    })
}

/// What an install does with a Flatpak its repository updates: `flatpak update`, on Linux, and
/// only on the stable track (it follows the repository's latest build, never a pinned or nightly
/// one). `None` for any other copy, which [`install_plan`] decides; else the copy, or why the
/// update can't run. [`run_install`] and the UI's [`Event::Installable`] both ask this.
fn repository_update<'a>(app: &SuiteApp, existing: Option<&'a Installed>, options: &BackendOptions) -> Option<Result<&'a Installed, String>> {
    let inst = existing.filter(|i| i.source == nodalia_launcher_core::InstallSource::Flatpak && i.externally_managed)?;
    Some(if !matches!(options.versions.get(app.id), None | Some(VersionTrack::Stable)) {
        Err(install::managed_elsewhere(app, inst, false).unwrap_or_else(|| "Update this app through its Flatpak repository".into()))
    } else if !cfg!(target_os = "linux") {
        Err(flatpak_unsupported(app))
    } else {
        Ok(inst)
    })
}

/// Why a Flatpak can't be updated on a system without Flatpak.
fn flatpak_unsupported(app: &SuiteApp) -> String {
    format!("{} is a Flatpak, which is not supported on this platform", app.name)
}

/// Who asks [`install_plan`].
#[derive(Clone, Copy, Debug)]
enum Asking<'a> {
    /// An install: it asks this system what it can install now, and explains a refusal in full.
    Install,
    /// The installability answer: what this system could install a moment ago
    /// ([`Shared::system_kinds`]), and only whether a refusal would come.
    Answer(&'a install::SystemKinds),
}

/// What an install of `app` would download, worked out from what is installed and chosen
/// before anything is fetched.
struct InstallPlan {
    release: Release,
    platform: Platform,
    asset: Asset,
    kind: PackageKind,
}

/// The checks an install makes before it downloads anything: a copy installed outside the
/// launcher (a system binary, a repository package) isn't replaced; the release must be known and
/// on the chosen version track; and it must have a package that can replace the copy the way it
/// was installed. [`run_install`] and the UI's [`Event::Installable`] both ask this, so the UI
/// never offers an update the install would refuse.
fn install_plan(
    app: &SuiteApp,
    latest: Option<Release>,
    platform: Option<Platform>,
    options: &BackendOptions,
    (existing, record): (Option<&Installed>, Option<&crate::manifest::Record>),
    asking: Asking<'_>,
) -> Result<InstallPlan, String> {
    let managed = existing.and_then(|e| match asking {
        Asking::Install => install::managed_elsewhere(app, e, false),
        // No pacman lookup for the wording of a refusal: only yes or no is wanted.
        Asking::Answer(_) => install::is_managed_elsewhere(app, e).then(String::new),
    });
    if let Some(msg) = managed {
        return Err(msg);
    }
    let platform = platform.ok_or("this system isn't supported")?;
    let release = latest.ok_or("the selected release hasn't been found yet. Check for updates first.")?;
    let track = options.versions.get(app.id).unwrap_or(&VersionTrack::Stable);
    if !release_matches_track(&release, track, options.prereleases) {
        return Err("the selected version isn't available yet. Check for updates before installing.".into());
    }
    let system = match asking {
        Asking::Install => None,
        Asking::Answer(system) => Some(system),
    };
    let (asset, kind) = planned_asset_on(app, &release, platform, options, (existing, record), system).ok_or_else(|| {
        if platform.os == nodalia_launcher_core::Os::Linux
            && options.install_locations.folder(app.id).is_some()
            && existing.is_none()
            && record.is_none()
        {
            "this release has no AppImage for a custom install folder".to_string()
        } else {
            format!("{} {} has no download for {}", app.name, release.version_label(), platform.label())
        }
    })?;
    Ok(InstallPlan { release, platform, asset, kind })
}

/// Plans and runs an install or update of `app` from its latest known release, and returns the
/// message to show. `place` is the job's spot in the download line, used when the download starts.
#[allow(clippy::too_many_arguments)]
fn run_install(
    app: &SuiteApp,
    shared: &Shared,
    paths: &Paths,
    http: Option<&Http>,
    platform: Option<Platform>,
    options: &BackendOptions,
    existing: Option<&Installed>,
    record: Option<&crate::manifest::Record>,
    stop: &Stop,
    approved: Option<&str>,
    source: Option<&dyn ReleaseSource>,
    batch: Option<crate::queue::Gather>,
    place: Option<crate::downloads::Place<'static>>,
) -> Result<String, String> {
    // The ways out below that never reach the installer (a missing release, the overwrite
    // question, a paused download) free the rest of the batch at once; from `install_at` on, the
    // same guard does it there.
    let mut batch = install::BatchPlace(batch);
    if let Some(update) = repository_update(app, existing, options) {
        // A repository-managed Flatpak never joins the batch's installer call: leave before its
        // own update (network, maybe a prompt) so the ready members don't wait for it.
        drop(batch);
        let inst = update?;
        #[cfg(target_os = "linux")]
        {
            shared.emit(Event::Progress { app: app.id.to_string(), phase: JobPhase::Installing });
            let mut phase = |p| shared.emit(Event::Progress { app: app.id.to_string(), phase: p });
            let default_id = app.bundle_id();
            let target_id = inst.package.as_deref().unwrap_or(&default_id);
            let scope = crate::packages::flatpak_scope(inst.installation.as_deref());
            let deployed_commit = || {
                crate::process::capture_flatpak(&["info", &scope, "--show-commit", target_id], std::time::Duration::from_secs(15))
                    .filter(|value| !value.trim().is_empty())
            };
            let before = deployed_commit();
            install::linux::update_flatpak_app(app, target_id, inst.installation.as_deref(), inst.origin.as_deref(), stop, &mut phase)?;
            let after = deployed_commit();
            return Ok(match (before, after) {
                (Some(before), Some(after)) if before.trim() == after.trim() => format!("{} is already up to date", app.name),
                (Some(_), Some(_)) => format!("{} was updated through its Flatpak repository", app.name),
                _ => format!("Flatpak finished checking {} for updates", app.name),
            });
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = inst;
            return Err(flatpak_unsupported(app));
        }
    }
    let latest = shared.latest.lock().unwrap_or_else(PoisonError::into_inner).get(app.id).cloned();
    let InstallPlan { release, platform, asset, kind } = install_plan(app, latest, platform, options, (existing, record), Asking::Install)?;
    let http = http.ok_or("secure connections aren't available on this system")?;
    let track = options.versions.get(app.id).unwrap_or(&VersionTrack::Stable);
    // Going back to a version chosen by name. A macOS bundle, a launcher AppImage or Flatpak keeps the old
    // copy until the new one is in place. Otherwise the installed version's own package is found
    // first, so a downgrade that fails can put it back. Without it nothing changes.
    let keeps_old_copy = install::keeps_old_copy(kind, existing, record);
    let previous = if keeps_old_copy {
        None
    } else {
        downgrade_previous(app, track, &release, existing, || {
            source.ok_or_else(|| "secure connections aren't available on this system".to_string())?.versions(app.repo)
        })?
    };
    let previous_asset = match &previous {
        Some(p) => Some(
            planned_asset(app, p, platform, options, existing, record)
                .filter(|(_, k)| *k == kind)
                .map(|(a, _)| a)
                .ok_or_else(|| format!("{} {} has no matching download to put back, so nothing was changed.", app.name, p.version_label()))?,
        ),
        None => None,
    };
    let downgrade = if keeps_old_copy {
        is_chosen_downgrade(track, &release, existing).then_some(install::Downgrade::KeepOldCopy)
    } else {
        previous.as_ref().zip(previous_asset.as_ref()).map(|(previous, asset)| install::Downgrade::Reinstall { previous, asset })
    };
    let occupied = install::occupied(app, &asset, kind, existing, record, &options.install_locations, paths)?;
    let replace = match replace_consent(occupied.as_deref(), approved) {
        Consent::Free => false,
        Consent::Given => true,
        Consent::Ask(path) => {
            shared.emit(Event::Occupied { app: app.id.to_string(), path: path.display().to_string() });
            return Err(format!("{} already exists", path.display()));
        }
    };
    let approved_path = occupied.as_deref().filter(|_| replace);
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut phase = |p: JobPhase| {
        // Downloads report thousands of times; the UI needs ~20 a second.
        if matches!(p, JobPhase::Downloading { done, total } if done != total && done != 0) && last_emit.elapsed() < Duration::from_millis(50) {
            return;
        }
        last_emit = Instant::now();
        shared.emit(Event::Progress { app: app.id.to_string(), phase: p });
    };
    let mut job = Job {
        http,
        paths,
        stop,
        phase: &mut phase,
        install_locations: &options.install_locations,
        replace,
        approved_path,
        downgrade,
        batch: batch.take(),
        place,
    };
    let mut commit_record = |record: &crate::manifest::Record| {
        let mut manifest = shared.manifest.lock().unwrap_or_else(PoisonError::into_inner);
        let mut updated = manifest.clone();
        updated.apps.insert(app.id.to_string(), record.clone());
        updated.save(&paths.manifest())?;
        *manifest = updated;
        Ok(())
    };
    let new_record = install::install_at(app, &release, &asset, kind, existing, record, &mut job, Some(&mut commit_record))?;
    {
        let mut m = shared.manifest.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = match new_record {
            Some(r) => {
                m.apps.insert(app.id.to_string(), r);
                true
            }
            // Installed the platform's own way: any old launcher record is stale.
            None => m.apps.remove(app.id).is_some(),
        };
        if changed && let Err(e) = m.save(&paths.manifest()) {
            log::warn!("couldn't save the install record: {e}");
        }
    }
    Ok(installed_text(app, existing.is_some(), &release))
}

/// Whether an install may write over what is at its destination.
#[derive(Debug, PartialEq, Eq)]
enum Consent<'a> {
    /// Nothing the launcher didn't put there is in the way.
    Free,
    /// The user agreed to overwrite exactly this destination.
    Given,
    /// Something is in the way and the user hasn't agreed to overwrite it: ask.
    Ask(&'a std::path::Path),
}

/// The user's agreement to replace a file covers the path they were shown (`approved`), and only
/// that one. The release can change between the question and the answer (a new version, another
/// asset, another folder), and the install then writes somewhere else: it must not take the old
/// "yes" as leave to overwrite a different file (CWE-367).
fn replace_consent<'a>(occupied: Option<&'a std::path::Path>, approved: Option<&str>) -> Consent<'a> {
    match occupied {
        None => Consent::Free,
        Some(path) if approved == Some(path.display().to_string().as_str()) => Consent::Given,
        Some(path) => Consent::Ask(path),
    }
}

/// What a finished install tells the user.
fn installed_text(app: &SuiteApp, update: bool, release: &Release) -> String {
    let verb = if update { "updated to" } else { "installed:" };
    format!("{} {verb} {}", app.name, release.version_label())
}

/// Whether an install whose job failed with `error` went in all the same. Only a failed call
/// shared by several apps (Linux packages installed together) can have installed some of them;
/// any other error, a cancellation or a pause stands, whatever is found afterwards.
fn landed_despite(error: &str, before: Option<&Installed>, after: Option<&Installed>, latest: &version::Version) -> bool {
    error != PAUSED && error != CANCELLED && error != INSTALL_CANCELLED && install::failed_in_shared_call(error) && landed(before, after, latest)
}

/// Whether an install took, whatever its job reported: `latest` is there now and wasn't before.
fn landed(before: Option<&Installed>, after: Option<&Installed>, latest: &version::Version) -> bool {
    let has_latest = |copy: Option<&Installed>| copy.and_then(|i| i.version.as_ref()).is_some_and(|v| !version::is_update(latest, v));
    has_latest(after) && !has_latest(before)
}

/// For the launcher's own update, before it replaces itself: waits (saying so through `waiting`)
/// until every other job has ended, then sets `restarting`, which keeps new jobs from starting.
/// The restart that follows would otherwise cut their installs short. The user can cancel the
/// wait (`stop`); after `limit` it gives up and names the apps still busy, since one may be
/// stuck behind a password prompt nobody has answered.
fn claim_restart(
    jobs: &Mutex<HashMap<String, Arc<Stop>>>,
    restarting: &AtomicBool,
    stop: &Stop,
    limit: Duration,
    waiting: &mut dyn FnMut(),
) -> Result<(), String> {
    let started = Instant::now();
    let mut told = false;
    loop {
        stop.check()?;
        let busy: Vec<&'static str> = {
            let jobs = jobs.lock().unwrap_or_else(PoisonError::into_inner);
            let busy = jobs.keys().filter(|id| *id != LAUNCHER_ID).map(|id| SuiteApp::by_id(id).map_or("another app", |a| a.name)).collect();
            if jobs.keys().all(|id| id == LAUNCHER_ID) {
                restarting.store(true, Ordering::SeqCst);
                return Ok(());
            }
            busy
        };
        if started.elapsed() >= limit {
            return Err(restart_blocked(busy));
        }
        if !told {
            told = true;
            waiting();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Why the launcher didn't restart: `busy` apps were still installing, and what to do about it.
fn restart_blocked(mut busy: Vec<&str>) -> String {
    busy.sort_unstable();
    busy.dedup();
    let (names, were, them) = match busy.as_slice() {
        [one] => ((*one).to_string(), "was", "it"),
        _ => (busy.join(", "), "were", "them"),
    };
    format!(
        "Nodalia Launcher didn't restart to update: {names} {were} still installing. Answer or close any password prompt, \
         let {them} finish or cancel {them}, then press Update and restart again."
    )
}

/// The file an install of `app` would download now: same format as the installed copy, else
/// the user's preference, else the best this system supports.
fn planned_asset(
    app: &SuiteApp,
    release: &Release,
    platform: Platform,
    options: &BackendOptions,
    existing: Option<&Installed>,
    record: Option<&crate::manifest::Record>,
) -> Option<(Asset, PackageKind)> {
    planned_asset_on(app, release, platform, options, (existing, record), None)
}

/// [`planned_asset`], on what `system` can install when given (else asking this system now).
fn planned_asset_on(
    app: &SuiteApp,
    release: &Release,
    platform: Platform,
    options: &BackendOptions,
    (existing, record): (Option<&Installed>, Option<&crate::manifest::Record>),
    system: Option<&install::SystemKinds>,
) -> Option<(Asset, PackageKind)> {
    let stems: Vec<&str> = app.stems().collect();
    let custom_linux = platform.os == nodalia_launcher_core::Os::Linux
        && !process::in_sandbox()
        && existing.is_none()
        && record.is_none()
        && options.install_locations.folder(app.id).is_some();
    let kinds = if existing.is_none() && record.is_some_and(|r| r.kind == PackageKind::PortableZip) {
        vec![PackageKind::PortableZip]
    } else if custom_linux {
        vec![nodalia_launcher_core::PackageKind::AppImage]
    } else {
        match system {
            Some(system) => install::choose_kinds_on(app, existing, record, &options.kinds, system),
            None => install::choose_kinds(app, existing, record, &options.kinds),
        }
    };
    pick_asset(release, &stems, platform, &kinds).map(|(a, k)| (a.clone(), k))
}

/// Tells the UI whether installing `app` would go ahead now ([`Event::Installable`]): the same
/// [`install_plan`] an install makes, for the copy that is installed. Nothing is said until its
/// release is known, or while nothing is installed (the UI only uses it for an installed copy).
fn report_installable(app: &SuiteApp, shared: &Shared, platform: Option<Platform>) {
    let system = shared.has_copy(app).then(|| shared.system_kinds());
    let _reporting = shared.reporting.lock().unwrap_or_else(PoisonError::into_inner);
    shared.emit_all(installable_event(app, shared, platform, system));
}

/// [`report_installable`]'s answer from the shared state as it is, with the current settings:
/// whether [`install_plan`] would let an install go ahead. `system` is what this system can
/// install, asked before `reporting` was taken; when a copy appeared since, the view its detection
/// asked for is used, so nothing is probed here. The caller holds `reporting`.
fn installable_event(app: &SuiteApp, shared: &Shared, platform: Option<Platform>, system: Option<install::SystemKinds>) -> Option<Event> {
    let existing = shared.installed.lock().unwrap_or_else(PoisonError::into_inner).get(app.id).cloned()?;
    let release = shared.latest.lock().unwrap_or_else(PoisonError::into_inner).get(app.id).cloned()?;
    let record = shared.manifest.lock().unwrap_or_else(PoisonError::into_inner).apps.get(app.id).cloned();
    let options = shared.options.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let installable = match repository_update(app, Some(&existing), &options) {
        // Updated with `flatpak update`: nothing is downloaded, so nothing to ask the system.
        Some(update) => update.is_ok(),
        None => {
            // A copy appeared after the caller looked: what was asked moments ago, not a probe here.
            let system = system.unwrap_or_else(|| shared.last_system_kinds());
            install_plan(app, Some(release), platform, &options, (Some(&existing), record.as_ref()), Asking::Answer(&system)).is_ok()
        }
    };
    Some(Event::Installable { app: app.id.to_string(), installable })
}

/// Records `app`'s latest release and tells the UI, with whether an install would go ahead with
/// it, in one batch: the UI never sees the release without its answer.
fn publish_latest(app: &SuiteApp, latest: LatestRelease, shared: &Shared, platform: Option<Platform>) {
    let system = shared.has_copy(app).then(|| shared.system_kinds());
    let _reporting = shared.reporting.lock().unwrap_or_else(PoisonError::into_inner);
    if let LatestRelease::Found(r) = &latest {
        shared.store_latest(app.id, r);
    }
    let installable = installable_event(app, shared, platform, system);
    shared.emit_all(installable.into_iter().chain([Event::Latest { app: app.id.to_string(), latest }]));
}

/// Records what is installed of `app` (`found`) and tells the UI, with whether an install would
/// go ahead to replace it, in one batch.
fn publish_detected(app: &SuiteApp, found: Option<Installed>, shared: &Shared, platform: Option<Platform>) {
    let system = found.is_some().then(|| shared.system_kinds());
    let _reporting = shared.reporting.lock().unwrap_or_else(PoisonError::into_inner);
    shared.store_detected(app.id, found.as_ref());
    let installable = installable_event(app, shared, platform, system);
    shared.emit_all(installable.into_iter().chain([Event::Detected { app: app.id.to_string(), installed: found }]));
}

/// Tells the UI whether `app` has a paused download waiting, or a finished one to install, and
/// whether an install would go ahead at all (the package choices may have changed). Worked out
/// with the settings and state as they are now, one report at a time (`planning`), so a worker
/// that started before a settings change never has the last word.
fn report_partial(app: &SuiteApp, shared: &Shared, paths: &Paths, platform: Option<Platform>) {
    let _one_at_a_time = shared.planning.lock().unwrap_or_else(PoisonError::into_inner);
    // A release or detection can change during planning; retry any rejected report.
    loop {
        // Capture the revision before the options: Configure invalidates it while changing options.
        let revision = shared.download_revision(app.id);
        let options = shared.options.lock().unwrap_or_else(PoisonError::into_inner).clone();
        report_installable(app, shared, platform);
        let planned = planned_download(app, shared, paths, platform, &options);
        let download = planned.as_ref().and_then(|(dest, _, size)| Some(PausedDownload { done: http::partial_bytes(dest, *size)?, total: *size }));
        let ready = planned.as_ref().is_some_and(|(dest, _, size)| http::finished(dest, *size));
        if shared.report_download(app.id, revision, planned.as_ref().map(|(_, kind, size)| (*kind, *size)), download, ready) {
            return;
        }
    }
}

/// Where the planned download of `app` is kept, its package format and published size.
fn planned_download(
    app: &SuiteApp,
    shared: &Shared,
    paths: &Paths,
    platform: Option<Platform>,
    options: &BackendOptions,
) -> Option<(std::path::PathBuf, PackageKind, u64)> {
    let release = shared.latest.lock().unwrap_or_else(PoisonError::into_inner).get(app.id).cloned()?;
    let existing = shared.installed.lock().unwrap_or_else(PoisonError::into_inner).get(app.id).cloned();
    let record = shared.manifest.lock().unwrap_or_else(PoisonError::into_inner).apps.get(app.id).cloned();
    let (asset, kind) = planned_asset(app, &release, platform?, options, existing.as_ref(), record.as_ref())?;
    nodalia_launcher_core::release::valid_file_name(&asset.name).then(|| (install::download_path(paths, &asset), kind, asset.size))
}

/// Answers the apps from a shared queue over a few workers, and returns which sources answered.
fn sweep(
    queue: &Mutex<Vec<&'static SuiteApp>>,
    source: &Arc<dyn ReleaseSource>,
    shared: &Arc<Shared>,
    (platform, options): (Option<Platform>, &BackendOptions),
    force: bool,
    now: i64,
) -> Vec<Source> {
    let sources = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let Some(app) = queue.lock().unwrap_or_else(PoisonError::into_inner).pop() else { break };
                    let track =
                        if app.id == LAUNCHER_ID { &options.launcher_version } else { options.versions.get(app.id).unwrap_or(&VersionTrack::Stable) };
                    let (latest, used) = if *track == VersionTrack::Stable {
                        latest_for(app, source.as_ref(), shared, options.prereleases, force, now)
                    } else {
                        let answer = source.chosen(app.repo, track);
                        (Some(answer.latest), Some(answer.source))
                    };
                    if let Some(used) = used {
                        sources.lock().unwrap_or_else(PoisonError::into_inner).push(used);
                    }
                    check_app(app, latest, shared, platform);
                }
            });
        }
    });
    sources.into_inner().unwrap_or_else(PoisonError::into_inner)
}

/// Publishes one app's answer to the shared state, or handles the launcher's own update check
/// first (it doesn't appear in the grid, so it has no card of its own to publish to).
fn check_app(app: &'static SuiteApp, latest: Option<LatestRelease>, shared: &Arc<Shared>, platform: Option<Platform>) {
    if app.id == LAUNCHER_ID {
        launcher_update(latest, shared);
        return;
    }
    let Some(latest) = latest else { return };
    publish_latest(app, latest, shared, platform);
}

/// Offers the launcher's own update when its answer is a newer release than the running one.
fn launcher_update(latest: Option<LatestRelease>, shared: &Arc<Shared>) {
    let Some(LatestRelease::Found(r)) = latest else { return };
    let (Some(newest), Some(current)) = (r.version.as_ref(), crate::self_update::current_version()) else { return };
    if version::is_newer(newest, &current) {
        shared.self_install.get_or_init(crate::self_update::current);
        shared.self_updating.get_or_init(crate::self_update::is_self_updating);
        *shared.launcher_release.lock().unwrap_or_else(PoisonError::into_inner) = Some((r.clone(), LAUNCHER.repo.to_string()));
        shared.emit(Event::LauncherUpdate(r));
    }
}

/// The weakest source that answered this check; it sets the wording of the "checked with" line.
fn source_used_of(sources: &[Source]) -> Source {
    if sources.contains(&Source::Cache) {
        Source::Cache
    } else if sources.contains(&Source::GitHubPages) {
        Source::GitHubPages
    } else {
        Source::GitHubApi
    }
}

/// The latest release of `app`: GitHub when it's due (or `force`), else GitHub's last answer. No
/// release when there's nothing new to say.
fn latest_for(
    app: &SuiteApp,
    source: &dyn ReleaseSource,
    shared: &Shared,
    pre: bool,
    force: bool,
    now: i64,
) -> (Option<LatestRelease>, Option<Source>) {
    let due = {
        let mut asked = shared.github_asked.lock().unwrap_or_else(PoisonError::into_inner);
        let due = force || asked.get(app.id).is_none_or(|t| now - t >= GITHUB_EVERY);
        if due {
            asked.insert(app.id.to_string(), now);
        }
        due
    };
    if due {
        let answer = source.latest(app.repo, pre);
        (Some(answer.latest), Some(answer.source))
    } else {
        (source.cached(app.repo, pre), None)
    }
}

/// Until when the footer says GitHub's limit holds details back: only while it does, that is
/// when this check had to settle for the release pages or saved answers. When the feed (or the
/// API itself) answered everything, nothing is missing, however recently GitHub said no.
fn limit_notice(source_used: Source, limited_until: Option<i64>) -> Option<i64> {
    limited_until.filter(|_| matches!(source_used, Source::GitHubPages | Source::Cache))
}

fn native_platform() -> Option<Platform> {
    let mut p = Platform::current()?;
    if p.os == Os::Windows {
        let arch = std::env::var("PROCESSOR_ARCHITEW6432").or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE")).unwrap_or_default();
        if arch.eq_ignore_ascii_case("ARM64") {
            p.arch = Arch::Arm64;
        } else if arch.eq_ignore_ascii_case("AMD64") {
            p.arch = Arch::X64;
        }
    }
    Some(p)
}

/// Clears an app's "pictures on their way" mark when the thread fetching them ends, panic or not.
struct PicturesBusy {
    shared: Arc<Shared>,
    app: &'static str,
}

impl Drop for PicturesBusy {
    fn drop(&mut self) {
        self.shared.pictures_busy.lock().unwrap_or_else(PoisonError::into_inner).remove(self.app);
    }
}

impl NativeBackend {
    /// Starts fetching the pictures of an app's details page, unless they are already on their way.
    /// An app without a page, or a name that isn't an app, is ignored.
    fn fetch_screenshots(&self, app: &str) {
        let Some(details) = nodalia_launcher_core::details::AppDetails::by_id(app) else { return };
        if !self.shared.pictures_busy.lock().unwrap_or_else(PoisonError::into_inner).insert(details.id) {
            return;
        }
        let (shared, http, paths) = (self.shared.clone(), self.http.clone(), self.paths.clone());
        std::thread::spawn(move || {
            let _busy = PicturesBusy { shared: shared.clone(), app: details.id };
            crate::screenshots::fetch_all(http.as_ref(), &paths, details, |key, result| {
                shared.emit(Event::Screenshot { app: details.id.to_string(), key: key.to_string(), result });
            });
        });
    }
}

impl Backend for NativeBackend {
    /// Carries out a command from the UI. Long work (checks, installs, removals) runs on its own
    /// thread.
    fn send(&mut self, command: Command) {
        match command {
            Command::CheckForUpdates { force } => self.check(force),
            Command::Detect => self.detect_all(),
            Command::ListVersions { app } => {
                let Some(def) = (if app == LAUNCHER_ID { Some(&LAUNCHER) } else { SuiteApp::by_id(&app) }) else { return };
                let source = self.source.clone();
                let shared = self.shared.clone();
                let repo = def.repo.to_string();
                std::thread::spawn(move || {
                    let result = source.ok_or("secure connections aren't available on this system".to_string()).and_then(|s| s.versions(&repo));
                    shared.emit(Event::Versions { app, result });
                });
            }
            Command::Install { app } => {
                self.start_job(&app, Work::Install(None));
            }
            Command::InstallAll { apps } => self.install_all(&apps),
            Command::Replace { app, path } => {
                self.start_job(&app, Work::Replace(path));
            }
            Command::Uninstall { app } => {
                self.start_job(&app, Work::Uninstall);
            }
            Command::Pause { app } => {
                if let Some(stop) = self.shared.jobs.lock().unwrap_or_else(PoisonError::into_inner).get(&app) {
                    stop.pause();
                }
            }
            Command::Cancel { app } => {
                let running = self.shared.jobs.lock().unwrap_or_else(PoisonError::into_inner).get(&app).cloned();
                match running {
                    Some(stop) => stop.cancel(),
                    // A paused download, or one kept after a failed install: throw the file away.
                    None => {
                        if let Some(def) = SuiteApp::by_id(&app)
                            && let Some((dest, _, size)) = planned_download(def, &self.shared, &self.paths, self.platform, &self.options())
                        {
                            http::discard(&dest, size);
                        }
                        self.shared.emit(Event::Paused { app: app.clone(), download: None });
                        self.shared.emit(Event::ReadyToInstall { app, ready: false });
                    }
                }
            }
            Command::Launch { app, args } => {
                let Some(def) = SuiteApp::by_id(&app) else { return };
                let result = match self.installed(&app) {
                    Some(inst) => install::launch(def, &inst, &self.paths, &args),
                    None => Err(format!("{} isn't installed", def.name)),
                };
                if let Err(e) = result {
                    self.shared.emit(Event::JobFinished { app, result: Err(e) });
                }
            }
            Command::Reveal { app } => {
                if let Some(inst) = self.installed(&app) {
                    let path = std::path::PathBuf::from(&inst.location);
                    let result = if path.exists() { crate::process::reveal(&path) } else { Err("it isn't a file on this computer".into()) };
                    if let Err(e) = result {
                        self.shared.emit(Event::JobFinished { app, result: Err(format!("Couldn't show it: {e}")) });
                    }
                }
            }
            Command::CreateDesktopShortcut { app } => {
                let Some(def) = SuiteApp::by_id(&app) else { return };
                let Some(inst) = self.installed(&app) else {
                    self.shared.emit(Event::Notice { app, result: Err(format!("{} isn't installed", def.name)) });
                    return;
                };
                let path = std::path::PathBuf::from(&inst.location);
                let name = def.name;
                let app_id = app;
                let shared = self.shared.clone();
                // PowerShell-free .lnk write can still take a moment; don't freeze the UI thread.
                std::thread::spawn(move || {
                    let result = match crate::process::create_desktop_shortcut(name, &path) {
                        Ok(()) => Ok(format!("Desktop shortcut created for {name}")),
                        Err(e) if e.starts_with("A desktop shortcut named ") => Err(e),
                        Err(e) => Err(format!("Couldn't create the desktop shortcut: {e}")),
                    };
                    shared.emit(Event::Notice { app: app_id, result });
                });
            }
            Command::UpdateLauncher => self.update_launcher(),
            Command::FetchScreenshots { app } => self.fetch_screenshots(&app),
            Command::DownloadOrder(order) => install::DOWNLOADS.set_order(order),
            Command::Configure(options) => {
                let changed_pre = {
                    let mut o = self.shared.options.lock().unwrap_or_else(PoisonError::into_inner);
                    let changed =
                        o.prereleases != options.prereleases || o.versions != options.versions || o.launcher_version != options.launcher_version;
                    install::DOWNLOADS.set_limit(options.simultaneous_downloads.into());
                    *o = options;
                    // An older worker must not overwrite the plan for these settings.
                    for cached in self.shared.download_sizes.lock().unwrap_or_else(PoisonError::into_inner).values_mut() {
                        cached.invalidate();
                    }
                    changed
                };
                // Another package format means another file to download.
                self.replan();
                if changed_pre {
                    self.check(true);
                }
            }
        }
    }

    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut *self.shared.events.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn launcher_kind(&self) -> Option<PackageKind> {
        // Looked up (it can run package-manager queries) with a newer launcher release; never here.
        self.shared.self_install.get().and_then(Option::as_ref).map(crate::self_update::kind)
    }

    fn capabilities(&self) -> Capabilities {
        let reveal_label = match self.platform.map(|p| p.os) {
            Some(Os::MacOs) => "Show in Finder",
            Some(Os::Windows) => "Show in Explorer",
            _ => "Show in Files",
        };
        Capabilities {
            manage_apps: true,
            reveal_label,
            self_update: self.launcher_update_job().is_some(),
            desktop_shortcuts: matches!(self.platform.map(|p| p.os), Some(Os::Windows)),
        }
    }

    fn platform(&self) -> Option<Platform> {
        self.platform
    }

    fn download_size(&self, app: &str) -> Option<(PackageKind, u64)> {
        self.shared.download_sizes.lock().unwrap_or_else(PoisonError::into_inner).get(app).and_then(|cached| cached.plan)
    }

    fn now(&self) -> i64 {
        crate::now()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use nodalia_launcher_core::InstallSource;

    use nodalia_launcher_core::protocol::LatestRelease;

    use super::*;

    use crate::source::Answer;

    /// A member of a batch that ends before it ever reaches the installer — its download failed,
    /// or it turned out to need none of the batch's prompts — frees the rest of the batch at
    /// once, rather than holding them back until the batch's wait runs out.
    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn a_batch_member_that_never_reaches_the_installer_frees_the_rest() {
        let gather = install::INSTALLER.gather(1);
        {
            let _place = install::BatchPlace(Some(gather.clone()));
            assert!(!gather.open(), "the place is held while the job is on its way");
        }
        assert!(gather.open(), "dropping the place on the way out opens the gate");
    }

    /// A repository-managed Flatpak in an "Update all" leaves the batch before its own update
    /// runs, so a slow or blocked Flatpak never holds back the system packages waiting to share
    /// the batch's prompt.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_repository_flatpak_leaves_the_batch_before_updating() {
        let gather = install::INSTALLER.gather(1);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut shared = shared();
        shared.waker = Arc::new({
            let (gather, seen) = (gather.clone(), seen.clone());
            move || seen.lock().unwrap().push(gather.open())
        });
        let app = SuiteApp::by_id("lienzo").unwrap();
        let installed = Installed {
            version: version::parse("0.5.0"),
            source: InstallSource::Flatpak,
            location: String::new(),
            package: None,
            externally_managed: true,
            origin: None,
            installation: Some("user".into()),
        };
        let dir = tempfile::tempdir().unwrap();
        let stop = Stop::default();
        stop.cancel();
        let options = BackendOptions::default();
        let result = run_install(
            app,
            &shared,
            &Paths::under(dir.path()),
            None,
            None,
            &options,
            Some(&installed),
            None,
            &stop,
            None,
            None,
            Some(gather.clone()),
            None,
        );
        assert_eq!(result, Err(nodalia_launcher_core::protocol::CANCELLED.into()));
        let seen = seen.lock().unwrap();
        assert!(!seen.is_empty(), "the Flatpak update reports its progress");
        assert!(seen.iter().all(|&open| open), "the batch was already free when the Flatpak update began");
    }

    fn tagged(v: &str) -> Release {
        Release { tag: format!("v{v}"), version: version::parse(v), ..Default::default() }
    }

    /// A "Replace" answer covers the path that was shown, never a destination that changed since.
    #[test]
    fn replacing_needs_consent_for_the_exact_path_in_the_way() {
        let shown = std::path::Path::new("/home/a/Applications/lienzo-0.5.0-linux-x86_64.AppImage");
        let moved = std::path::Path::new("/home/a/Applications/lienzo-0.6.0-linux-x86_64.AppImage");
        let approved = shown.display().to_string();
        assert_eq!(replace_consent(None, None), Consent::Free);
        assert_eq!(replace_consent(None, Some(&approved)), Consent::Free, "nothing in the way: nothing to overwrite");
        assert_eq!(replace_consent(Some(shown), None), Consent::Ask(shown));
        assert_eq!(replace_consent(Some(shown), Some(&approved)), Consent::Given);
        assert_eq!(replace_consent(Some(moved), Some(&approved)), Consent::Ask(moved), "a yes for one file must not overwrite another");
    }

    #[test]
    fn a_downgrade_is_only_a_version_chosen_by_name_below_the_installed_one() {
        let installed = Installed {
            version: version::parse("0.14.0"),
            source: InstallSource::MacApp,
            location: String::new(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        };
        let pinned = VersionTrack::Pinned("v0.13.0".into());
        assert!(is_chosen_downgrade(&pinned, &tagged("0.13.0"), Some(&installed)));
        assert!(!is_chosen_downgrade(&VersionTrack::Stable, &tagged("0.13.0"), Some(&installed)));
        assert!(!is_chosen_downgrade(&VersionTrack::Pinned("v0.15.0".into()), &tagged("0.15.0"), Some(&installed)));
        assert!(!is_chosen_downgrade(&pinned, &tagged("0.12.0"), Some(&installed)));
        assert!(!is_chosen_downgrade(&pinned, &tagged("0.13.0"), None));
    }

    #[test]
    fn only_a_version_chosen_by_name_below_the_installed_one_looks_for_a_package_to_put_back() {
        let app = SuiteApp::by_id("toma").unwrap();
        let installed = Installed {
            version: version::parse("0.14.0"),
            source: InstallSource::Msi,
            location: String::new(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        };
        let pinned = VersionTrack::Pinned("v0.13.0".into());
        let never = || -> Result<Vec<Release>, String> { panic!("no list is needed") };
        // An update, the latest release, nothing installed, or a stale release: no downgrade.
        assert_eq!(downgrade_previous(app, &VersionTrack::Stable, &tagged("0.13.0"), Some(&installed), never), Ok(None));
        assert_eq!(downgrade_previous(app, &VersionTrack::Pinned("v0.15.0".into()), &tagged("0.15.0"), Some(&installed), never), Ok(None));
        assert_eq!(downgrade_previous(app, &pinned, &tagged("0.13.0"), None, never), Ok(None));
        assert_eq!(downgrade_previous(app, &pinned, &tagged("0.12.0"), Some(&installed), never), Ok(None));
        // Going back finds the installed version's own release.
        let found = downgrade_previous(app, &pinned, &tagged("0.13.0"), Some(&installed), || Ok(vec![tagged("0.14.0"), tagged("0.13.0")]));
        assert_eq!(found.unwrap().map(|r| r.tag), Some("v0.14.0".to_string()));
        // Without it, or without the list, the downgrade doesn't start.
        let gone = downgrade_previous(app, &pinned, &tagged("0.13.0"), Some(&installed), || Ok(vec![tagged("0.13.0")])).unwrap_err();
        assert!(gone.contains("Toma 0.14.0 isn't published any more") && gone.contains("Nothing was changed"), "{gone}");
        let offline = downgrade_previous(app, &pinned, &tagged("0.13.0"), Some(&installed), || Err("offline".into())).unwrap_err();
        assert!(offline.contains("(offline)") && offline.contains("nothing was changed"), "{offline}");
    }

    /// Going back is a conscious choice: a check for a pinned app reports the pinned release, so
    /// a newer one never shows as an update (badge, Update all, notification or automatic install).
    #[test]
    fn a_pinned_app_is_never_offered_a_newer_release() {
        struct Published;
        impl ReleaseSource for Published {
            fn latest(&self, repo: &str, _prereleases: bool) -> crate::source::Answer {
                let found = SuiteApp::by_id(APP).is_some_and(|a| a.repo == repo);
                let latest = if found { LatestRelease::Found(tagged("0.6.0")) } else { LatestRelease::None };
                crate::source::Answer { latest, source: Source::GitHubApi }
            }
            fn versions(&self, _repo: &str) -> Result<Vec<Release>, String> {
                Ok(vec![tagged("0.6.0"), tagged("0.5.0")])
            }
            fn cached(&self, _repo: &str, _prereleases: bool) -> Option<LatestRelease> {
                None
            }
            fn rate_limited_until(&self) -> Option<i64> {
                None
            }
            fn save(&self) {}
        }
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, Some(Arc::new(Published)));
        backend.http = Some(Http::new().unwrap());
        {
            let mut options = backend.shared.options.lock().unwrap();
            options.prereleases = true; // asks the source directly, never GitHub's API
            options.versions.insert(APP.to_string(), VersionTrack::Pinned("v0.5.0".into()));
        }
        backend.send(Command::CheckForUpdates { force: true });
        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !events.iter().any(|e| matches!(e, Event::CheckFinished { .. })) {
            assert!(Instant::now() < deadline, "the check never finished");
            std::thread::sleep(Duration::from_millis(10));
            events.extend(backend.poll());
        }
        let answered = events.iter().rev().find_map(|e| match e {
            Event::Latest { app, latest: LatestRelease::Found(r), .. } if app == APP => Some(r.tag.clone()),
            _ => None,
        });
        assert_eq!(answered.as_deref(), Some("v0.5.0"), "the pinned release, not the newer one");
    }

    #[test]
    fn an_install_cannot_use_a_stale_release_from_a_different_track() {
        let mut release = Release { tag: "v0.8.0".into(), version: version::parse("v0.8.0"), ..Default::default() };
        assert!(!release_matches_track(&release, &VersionTrack::Nightly, false));
        assert!(!release_matches_track(&release, &VersionTrack::Pinned("v0.7.0".into()), false));
        assert!(release_matches_track(&release, &VersionTrack::Stable, false));
        release.tag = "v0.9.0-nightly.20261028".into();
        release.version = version::parse(&release.tag);
        release.prerelease = true;
        assert!(release_matches_track(&release, &VersionTrack::Nightly, false));
        assert!(!release_matches_track(&release, &VersionTrack::Stable, false));
    }

    fn shared() -> Shared {
        Shared {
            events: Mutex::new(Vec::new()),
            reporting: Mutex::new(()),
            system: Mutex::new(None),
            planning: Mutex::new(()),
            waker: Arc::new(|| {}),
            latest: Mutex::new(HashMap::new()),
            download_sizes: Mutex::new(HashMap::new()),
            installed: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            restarting: AtomicBool::new(false),
            manifest: Mutex::new(Manifest::default()),
            options: Mutex::new(BackendOptions::default()),
            checking: AtomicBool::new(false),
            detecting: AtomicBool::new(false),
            last_check: Mutex::new(None),
            launcher_release: Mutex::new(None),
            self_install: OnceLock::new(),
            self_updating: OnceLock::new(),
            github_asked: Mutex::new(HashMap::new()),
            pictures_busy: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// GitHub, counting how often it's asked.
    #[derive(Default)]
    struct CountingGitHub(std::sync::atomic::AtomicUsize);

    impl ReleaseSource for CountingGitHub {
        fn latest(&self, _repo: &str, _pre: bool) -> Answer {
            self.0.fetch_add(1, Ordering::SeqCst);
            Answer { latest: LatestRelease::None, source: Source::GitHubApi }
        }
        fn cached(&self, _repo: &str, _pre: bool) -> Option<LatestRelease> {
            None
        }
        fn rate_limited_until(&self) -> Option<i64> {
            None
        }
        fn save(&self) {}
    }

    const APP: &str = "lienzo";

    /// A Lienzo release with one file per platform and format, so whichever this machine
    /// installs is there.
    fn release(version: &str) -> Release {
        let suffixes = ["-portable.zip", ".tar.gz", ".msi", ".dmg", ".AppImage", ".deb", ".rpm", ".flatpak"];
        let tokens = [
            "windows-x64",
            "windows-x86",
            "windows-arm64",
            "macos-universal",
            "linux-x86_64",
            "linux-aarch64",
            "linux-riscv64",
            "freebsd-x86_64",
            "freebsd-aarch64",
        ];
        let assets = tokens
            .iter()
            .flat_map(|t| suffixes.iter().map(move |s| format!("{APP}-{version}-{t}{s}")))
            .map(|name| Asset { url: format!("https://example.invalid/{name}"), name, size: 3, sha256: None })
            .collect();
        Release { assets, ..Release::default() }
    }

    #[cfg(windows)]
    #[test]
    fn windows_preference_selects_portable_and_existing_installs_keep_their_format() {
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::Windows, arch: Arch::X64 };
        let release = release("0.5.0");
        let portable_options = BackendOptions { kinds: vec![PackageKind::PortableZip], ..Default::default() };
        assert_eq!(planned_asset(app, &release, platform, &portable_options, None, None).unwrap().1, PackageKind::PortableZip);
        let msi = Installed {
            version: None,
            source: nodalia_launcher_core::InstallSource::Msi,
            location: r"C:\Apps\Lienzo\lienzo.exe".into(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        };
        assert_eq!(planned_asset(app, &release, platform, &portable_options, Some(&msi), None).unwrap().1, PackageKind::Msi);
        let launcher = Installed {
            version: None,
            source: nodalia_launcher_core::InstallSource::Launcher,
            location: r"D:\Nodalia Apps\Lienzo\lienzo.exe".into(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        };
        let record = crate::manifest::Record {
            version: "0.4.0".into(),
            kind: PackageKind::PortableZip,
            path: launcher.location.clone().into(),
            installed_at: 1,
            files: vec![],
            needs_root: false,
        };
        let msi_options = BackendOptions { kinds: vec![PackageKind::Msi], ..Default::default() };
        assert_eq!(planned_asset(app, &release, platform, &msi_options, Some(&launcher), Some(&record)).unwrap().1, PackageKind::PortableZip);
        assert_eq!(planned_asset(app, &release, platform, &msi_options, None, Some(&record)).unwrap().1, PackageKind::PortableZip);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_custom_linux_folder_requires_appimage_without_package_fallback() {
        if process::in_sandbox() {
            return;
        }
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::Linux, arch: Arch::X64 };
        let options = BackendOptions {
            kinds: vec![PackageKind::Deb],
            install_locations: nodalia_launcher_core::protocol::InstallLocations { root: Some("/custom/apps".into()), ..Default::default() },
            ..Default::default()
        };
        let mut release = release("0.5.0");
        assert_eq!(planned_asset(app, &release, platform, &options, None, None).unwrap().1, PackageKind::AppImage);
        release.assets.retain(|a| !a.name.ends_with(".AppImage"));
        assert!(planned_asset(app, &release, platform, &options, None, None).is_none());
        let existing = Installed {
            version: None,
            source: nodalia_launcher_core::InstallSource::Deb,
            location: "/usr/bin/lienzo".into(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        };
        let system = BackendOptions { kinds: options.kinds.clone(), ..Default::default() };
        assert_eq!(
            planned_asset(app, &release, platform, &options, Some(&existing), None),
            planned_asset(app, &release, platform, &system, Some(&existing), None)
        );
    }

    /// A backend that knows `release("0.5.0")` and keeps its files under `root`.
    fn backend(root: &std::path::Path, platform: Platform, source: Option<Arc<dyn ReleaseSource>>) -> NativeBackend {
        let shared = shared();
        shared.latest.lock().unwrap().insert(APP.to_string(), release("0.5.0"));
        let shared = Arc::new(shared);
        NativeBackend {
            shared,
            paths: Paths::under(root),
            http: None,
            source,
            platform: Some(platform),
            spawn: Arc::new(|work| {
                std::thread::spawn(work);
            }),
        }
    }

    /// What the installability answers in `events` said about [`APP`], and which `Latest` and
    /// `Detected` came with them, in order.
    fn told(events: Vec<Event>) -> Vec<String> {
        events
            .into_iter()
            .filter_map(|e| match e {
                Event::Installable { app, installable } if app == APP => Some(format!("installable {installable}")),
                Event::Latest { app, .. } if app == APP => Some("latest".to_string()),
                Event::Detected { app, .. } if app == APP => Some("detected".to_string()),
                _ => None,
            })
            .collect()
    }

    /// A copy of [`APP`] at `version`, installed the way `source` says.
    fn copy_of(version: &str, source: InstallSource, location: &str) -> Installed {
        Installed {
            version: nodalia_launcher_core::version::parse(version),
            source,
            location: location.into(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        }
    }

    /// `release(v)` with its version known, so it is on the stable track.
    fn stable(v: &str) -> Release {
        Release { version: nodalia_launcher_core::version::parse(v), ..release(v) }
    }

    /// Makes the backend's view of what this system can install `available` (as if just asked),
    /// so an answer doesn't depend on the computer the test runs on.
    fn system_installs(backend: &NativeBackend, available: Vec<PackageKind>) {
        *backend.shared.system.lock().unwrap() = Some((Instant::now(), install::SystemKinds { available, can_pacman: false }));
    }

    /// Each answer tells the UI whether an install would go ahead before it says the release is
    /// out or what is installed, in the same poll, so the UI never offers (or starts) an update it
    /// can't install. A release with nothing for this computer is not installable; one with a
    /// file is. Nothing is said for an app that isn't installed (the UI doesn't use it there), so
    /// nothing is asked of the system for it either, as at startup before detection.
    #[test]
    fn installability_is_told_with_the_release_or_copy_it_depends_on() {
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::MacOs, arch: Arch::Arm64 };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        system_installs(&backend, vec![PackageKind::Dmg]);

        publish_latest(app, LatestRelease::Found(stable("0.5.0")), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["latest"], "nothing installed: no answer");

        publish_detected(app, Some(copy_of("0.4.0", InstallSource::Launcher, "/Applications/Lienzo.app")), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["installable true", "detected"]);

        let mut windows_only = stable("0.6.0");
        windows_only.assets.retain(|a| a.name.contains("windows"));
        check_app(app, Some(LatestRelease::Found(windows_only)), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["installable false", "latest"]);
        check_app(app, Some(LatestRelease::Found(stable("0.6.0"))), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["installable true", "latest"]);

        publish_detected(app, None, &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["detected"], "removed: no answer");
    }

    /// The launcher offers to replace itself only when this copy can update itself and the newer
    /// release has a file to install it from. Otherwise the UI offers the download, in the format
    /// this copy was installed as. Update and Restart refuses in the same cases without starting a
    /// download.
    #[test]
    fn self_update_is_offered_only_when_the_copy_and_the_release_allow_it() {
        let platform = Platform { os: Os::Windows, arch: Arch::X64 };
        let release = |name: &str| Release {
            assets: vec![Asset { name: name.into(), size: 1, url: format!("https://example.invalid/{name}"), sha256: None }],
            ..stable("0.9.0")
        };
        let offered = |updating: bool, release: Release| {
            let dir = tempfile::tempdir().unwrap();
            let mut backend = backend(dir.path(), platform, None);
            let install = crate::self_update::Install::WindowsMsi { dir: dir.path().to_path_buf(), per_machine: false };
            assert!(backend.shared.self_install.set(Some(install)).is_ok());
            assert!(backend.shared.self_updating.set(updating).is_ok());
            *backend.shared.launcher_release.lock().unwrap() = Some((release, String::new()));
            let offered = backend.capabilities().self_update;
            if !offered {
                backend.update_launcher();
                let events = backend.poll();
                assert!(
                    matches!(&events[..], [Event::JobFinished { app, result: Err(_) }] if app == LAUNCHER_ID),
                    "it refuses without starting a download: {events:?}"
                );
                assert!(backend.shared.jobs.lock().unwrap().is_empty());
            }
            (offered, backend.launcher_kind())
        };
        let msi = release("nodalia-launcher-0.9.0-windows-x64.msi");
        let portable_only = release("nodalia-launcher-0.9.0-windows-x64-portable.zip");
        assert_eq!(offered(true, msi.clone()), (true, Some(PackageKind::Msi)));
        assert_eq!(offered(false, msi), (false, Some(PackageKind::Msi)), "a copy that can't update itself");
        assert_eq!(offered(true, portable_only), (false, Some(PackageKind::Msi)), "a release without a file for this copy");
    }

    /// A report made when only the plan changed (a settings change, a check or an install ending)
    /// still tells the UI whether an install would go ahead, on its own, with the settings as
    /// they are then.
    #[test]
    fn a_plan_report_answers_installability_with_the_current_settings() {
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::MacOs, arch: Arch::Arm64 };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        system_installs(&backend, vec![PackageKind::Dmg]);
        backend.shared.latest.lock().unwrap().insert(APP.to_string(), stable("0.6.0"));
        backend.shared.installed.lock().unwrap().insert(APP.to_string(), copy_of("0.4.0", InstallSource::Launcher, "/Applications/Lienzo.app"));
        let installable = |events: Vec<Event>| told(events).into_iter().filter(|t| t.starts_with("installable")).collect::<Vec<_>>();

        report_partial(app, &backend.shared, &backend.paths, Some(platform));
        assert_eq!(installable(backend.poll()), ["installable true"]);
        backend.shared.options.lock().unwrap().versions.insert(APP.to_string(), VersionTrack::Pinned("v0.7.0".into()));
        report_partial(app, &backend.shared, &backend.paths, Some(platform));
        assert_eq!(installable(backend.poll()), ["installable false"], "kept on another version: not what an install would take");
    }

    /// When a copy appears between a publisher looking and taking `reporting`, the answer uses
    /// the view of the system its detection asked for, however old: nothing is probed while
    /// `reporting` is held.
    #[test]
    fn an_answer_under_the_reporting_lock_never_probes_the_system() {
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::MacOs, arch: Arch::Arm64 };
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(dir.path(), platform, None);
        let asked = Instant::now().checked_sub(SYSTEM_KINDS_FOR * 2).unwrap_or_else(Instant::now);
        *backend.shared.system.lock().unwrap() = Some((asked, install::SystemKinds { available: vec![PackageKind::Dmg], can_pacman: false }));
        backend.shared.latest.lock().unwrap().insert(APP.to_string(), stable("0.6.0"));
        backend.shared.installed.lock().unwrap().insert(APP.to_string(), copy_of("0.4.0", InstallSource::Launcher, "/Applications/Lienzo.app"));

        let answer = {
            let _reporting = backend.shared.reporting.lock().unwrap();
            installable_event(app, &backend.shared, Some(platform), None)
        };
        assert_eq!(answer, Some(Event::Installable { app: APP.to_string(), installable: true }), "from the view it had");
        let kept = backend.shared.system.lock().unwrap().as_ref().map(|(at, _)| *at);
        assert_eq!(kept, Some(asked), "the expired view wasn't asked again under the lock");
    }

    /// However polls and publications interleave, a poll never holds a `Latest` or `Detected`
    /// without the answer queued with it, just before it.
    #[test]
    fn a_poll_never_splits_an_answer_from_its_release_or_copy() {
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::MacOs, arch: Arch::Arm64 };
        let dir = tempfile::tempdir().unwrap();
        let backend = backend(dir.path(), platform, None);
        system_installs(&backend, vec![PackageKind::Dmg]);
        let installed = copy_of("0.4.0", InstallSource::Launcher, "/Applications/Lienzo.app");
        publish_detected(app, Some(installed.clone()), &backend.shared, Some(platform));
        let shared = backend.shared.clone();
        let publisher = std::thread::spawn(move || {
            for i in 0..300 {
                if i % 2 == 0 {
                    publish_latest(app, LatestRelease::Found(stable("0.6.0")), &shared, Some(platform));
                } else {
                    publish_detected(app, Some(installed.clone()), &shared, Some(platform));
                }
            }
        });
        let mut seen = 0;
        while !publisher.is_finished() || !backend.shared.events.lock().unwrap().is_empty() {
            let events = std::mem::take(&mut *backend.shared.events.lock().unwrap());
            for (i, e) in events.iter().enumerate() {
                if matches!(e, Event::Latest { app, .. } | Event::Detected { app, .. } if app == APP) {
                    seen += 1;
                    let before = i.checked_sub(1).and_then(|j| events.get(j));
                    assert!(matches!(before, Some(Event::Installable { app, .. }) if app == APP), "a poll split a pair: {events:?}");
                }
            }
        }
        publisher.join().unwrap();
        assert_eq!(seen, 301, "every publication was seen");
    }

    /// The answer uses the settings as they are when it is worked out, not as a long job found
    /// them when it started: a job that ends after the version track changed answers for the new
    /// track.
    #[test]
    fn installability_follows_the_current_settings() {
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::MacOs, arch: Arch::Arm64 };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        system_installs(&backend, vec![PackageKind::Dmg]);
        backend.shared.latest.lock().unwrap().insert(APP.to_string(), stable("0.6.0"));
        let installed = copy_of("0.4.0", InstallSource::Launcher, "/Applications/Lienzo.app");

        publish_detected(app, Some(installed.clone()), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["installable true", "detected"]);

        // Kept on another version now: 0.6.0 isn't what an install would take.
        backend.shared.options.lock().unwrap().versions.insert(APP.to_string(), VersionTrack::Pinned("v0.7.0".into()));
        publish_detected(app, Some(installed), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["installable false", "detected"]);
    }

    /// A Flatpak its repository updates is updated with `flatpak update` (Linux, stable track), as
    /// an install does, so the answer says yes there and keeps Reinstall, the launcher's route to
    /// that update. Kept on another version it says no, as the install refuses.
    #[test]
    fn a_repository_flatpak_is_installable_where_flatpak_update_runs() {
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::Linux, arch: Arch::X64 };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        // Nothing this system could download: only `flatpak update` can say yes.
        system_installs(&backend, Vec::new());
        backend.shared.latest.lock().unwrap().insert(APP.to_string(), stable("0.5.0"));
        let flatpark = Installed { externally_managed: true, ..copy_of("0.5.0", InstallSource::Flatpak, "dev.danielmiguelt.Lienzo") };
        let options = BackendOptions::default();
        assert_eq!(repository_update(app, Some(&flatpark), &options).map(|u| u.is_ok()), Some(cfg!(target_os = "linux")));

        publish_detected(app, Some(flatpark.clone()), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), [format!("installable {}", cfg!(target_os = "linux")), "detected".to_string()]);

        backend.shared.options.lock().unwrap().versions.insert(APP.to_string(), VersionTrack::Pinned("v0.4.0".into()));
        publish_detected(app, Some(flatpark), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["installable false", "detected"], "flatpak update can't follow a pinned version");
    }

    /// A copy installed outside the launcher (FreeBSD's `/usr/local/bin/<app>`, a system binary)
    /// is never replaced, so a newer release with a matching FreeBSD tarball is not installable
    /// for it: the UI is told no, and an install is refused for the same reason. A copy the
    /// launcher installed takes the same tarball.
    #[test]
    fn a_system_binary_is_not_installable_even_with_a_matching_tarball() {
        let app = SuiteApp::by_id(APP).unwrap();
        let platform = Platform { os: Os::FreeBsd, arch: Arch::X64 };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        system_installs(&backend, vec![PackageKind::TarGz]);
        let options = BackendOptions::default();
        let system = copy_of("0.4.0", InstallSource::Binary, &format!("/usr/local/bin/{APP}"));
        let release = stable("0.6.0");
        assert!(release.assets.iter().any(|a| a.name.ends_with("-freebsd-x86_64.tar.gz")), "the release has a matching tarball");

        let refused = install_plan(app, Some(release.clone()), Some(platform), &options, (Some(&system), None), Asking::Install).err();
        assert_eq!(refused, install::managed_elsewhere(app, &system, false), "an install refuses it as installed outside the launcher");

        publish_detected(app, Some(system), &backend.shared, Some(platform));
        publish_latest(app, LatestRelease::Found(release), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["installable false", "detected", "installable false", "latest"]);

        let ours = copy_of("0.4.0", InstallSource::Launcher, &format!("/usr/local/bin/{APP}"));
        publish_detected(app, Some(ours), &backend.shared, Some(platform));
        assert_eq!(told(backend.poll()), ["installable true", "detected"], "the launcher's own copy takes the tarball");
    }

    /// Reports again, and returns what the UI was last told about a kept download.
    fn ready(backend: &mut NativeBackend) -> Option<bool> {
        let app = SuiteApp::by_id(APP).unwrap();
        report_partial(app, &backend.shared, &backend.paths, backend.platform);
        last_ready(backend.poll())
    }

    fn last_ready(events: Vec<Event>) -> Option<bool> {
        events.into_iter().rev().find_map(|e| match e {
            Event::ReadyToInstall { app, ready } if app == APP => Some(ready),
            _ => None,
        })
    }

    fn wait_until(mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ready() {
            assert!(Instant::now() < deadline, "the worker did not finish unwinding");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn panic_on_wake() -> (Waker, mpsc::Receiver<()>) {
        let (sent, received) = mpsc::channel();
        (
            Arc::new(move || {
                let _ = sent.send(());
                panic!("injected worker panic")
            }),
            received,
        )
    }

    #[test]
    fn a_panicking_detection_worker_releases_its_busy_flag() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        let (waker, woke) = panic_on_wake();
        Arc::get_mut(&mut backend.shared).unwrap().waker = waker;

        backend.detect_all();
        woke.recv_timeout(Duration::from_secs(2)).unwrap();
        wait_until(|| !backend.shared.detecting.load(Ordering::SeqCst));
    }

    #[test]
    fn a_panicking_check_worker_releases_its_busy_flag() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, Some(Arc::new(CountingGitHub::default())));
        backend.http = Some(Http::new().unwrap());
        let (waker, woke) = panic_on_wake();
        Arc::get_mut(&mut backend.shared).unwrap().waker = waker;

        backend.check(true);
        woke.recv_timeout(Duration::from_secs(2)).unwrap();
        wait_until(|| !backend.shared.checking.load(Ordering::SeqCst));
    }

    #[test]
    fn a_panicking_job_worker_removes_its_busy_entry() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        let (waker, woke) = panic_on_wake();
        Arc::get_mut(&mut backend.shared).unwrap().waker = waker;

        backend.start_job(APP, Work::Uninstall);
        woke.recv_timeout(Duration::from_secs(2)).unwrap();
        wait_until(|| !backend.shared.jobs.lock().unwrap().contains_key(APP));
    }

    /// "Update all" takes every place in the download line before any worker starts, so a worker
    /// can't reach a free slot ahead of a row above it that hasn't joined yet. The workers are
    /// held, not run, so this checks the order of the steps and doesn't race threads: starting
    /// each job as soon as it is prepared would find only the first app in line.
    #[test]
    fn update_all_takes_every_place_before_any_worker_starts() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        let ids: Vec<&str> = nodalia_launcher_core::SUITE.iter().filter(|app| app.runs_on(platform.os)).map(|app| app.id).take(3).collect();
        assert_eq!(ids.len(), 3);
        type Held = Vec<Box<dyn FnOnce() + Send>>;
        let held: Arc<Mutex<Held>> = Arc::default();
        let in_line_at_each_start: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
        backend.spawn = {
            let (held, seen) = (held.clone(), in_line_at_each_start.clone());
            Arc::new(move |work| {
                seen.lock().unwrap().push(install::DOWNLOADS.waiting_apps());
                held.lock().unwrap().push(work);
            })
        };

        backend.install_all(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>());

        let seen = in_line_at_each_start.lock().unwrap().clone();
        assert_eq!(seen.len(), 3, "one worker per app");
        for waiting in &seen {
            assert!(ids.iter().all(|id| waiting.iter().any(|w| w == id)), "every app is in line before any worker starts: {waiting:?}");
        }
        assert!(ids.iter().all(|id| backend.shared.jobs.lock().unwrap().contains_key(*id)), "and each one is reserved");
        drop(held.lock().unwrap().drain(..).collect::<Vec<_>>());
        let waiting = install::DOWNLOADS.waiting_apps();
        assert!(!ids.iter().any(|id| waiting.iter().any(|w| w == id)), "dropped, they leave the line: {waiting:?}");
        assert!(!ids.iter().any(|id| backend.shared.jobs.lock().unwrap().contains_key(*id)), "and the jobs are free again");
    }

    #[test]
    fn job_finished_follows_busy_cleanup_and_download_plan_publication() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        let shared: Arc<Mutex<Option<std::sync::Weak<Shared>>>> = Arc::new(Mutex::new(None));
        let shared_for_waker = shared.clone();
        let (sent, received) = mpsc::channel();
        Arc::get_mut(&mut backend.shared).unwrap().waker = Arc::new(move || {
            let Some(shared) = shared_for_waker.lock().unwrap().as_ref().and_then(std::sync::Weak::upgrade) else { return };
            let finished = shared.events.lock().unwrap().iter().any(|event| matches!(event, Event::JobFinished { app, .. } if app == APP));
            if finished {
                let busy = shared.jobs.lock().unwrap().contains_key(APP);
                let plan = shared.download_sizes.lock().unwrap().get(APP).and_then(|cached| cached.plan);
                let _ = sent.send((busy, plan));
            }
        });
        *shared.lock().unwrap() = Some(Arc::downgrade(&backend.shared));

        backend.start_job(APP, Work::Uninstall);
        let (busy, plan) = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!busy);
        assert!(matches!(plan, Some((_, 3))), "JobFinished observers need the final download plan: {plan:?}");
        assert_eq!(backend.download_size(APP), plan);
    }

    /// Puts the planned download on disk, as a failed install leaves it.
    fn keep_file(backend: &NativeBackend) -> std::path::PathBuf {
        let app = SuiteApp::by_id(APP).unwrap();
        let (dest, _, size) = planned_download(app, &backend.shared, &backend.paths, backend.platform, &BackendOptions::default()).unwrap();
        std::fs::create_dir_all(backend.paths.downloads()).unwrap();
        std::fs::write(&dest, vec![0u8; size as usize]).unwrap();
        dest
    }

    /// A report uses the settings as they are when it is worked out, not as the worker that sends
    /// it found them. A worker that started before the preferred format changed (and, before this
    /// was fixed, carried the settings it started with) waits behind a report in progress; by the
    /// time it reports, the kept MSI is no longer the file an install would use, and its report is
    /// the last word. (Windows only: it has two formats to choose between.)
    #[cfg(windows)]
    #[test]
    fn a_kept_download_report_follows_the_current_settings() {
        let platform = Platform { os: Os::Windows, arch: Arch::X64 };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        *backend.shared.options.lock().unwrap() = BackendOptions { kinds: vec![PackageKind::Msi], ..Default::default() };
        let app = SuiteApp::by_id(APP).unwrap();
        let options = backend.shared.options.lock().unwrap().clone();
        let (dest, _, size) = planned_download(app, &backend.shared, &backend.paths, backend.platform, &options).unwrap();
        std::fs::create_dir_all(backend.paths.downloads()).unwrap();
        std::fs::write(&dest, vec![0u8; size as usize]).unwrap();
        assert_eq!(ready(&mut backend), Some(true), "the kept MSI is the planned file");

        // A report is in progress; an older worker starts and waits behind it.
        let in_progress = backend.shared.planning.lock().unwrap();
        let (shared, paths) = (backend.shared.clone(), backend.paths.clone());
        let older = std::thread::spawn(move || report_partial(app, &shared, &paths, Some(platform)));
        // The preferred format changes while it waits, then the report in progress finishes.
        backend.shared.options.lock().unwrap().kinds = vec![PackageKind::PortableZip];
        drop(in_progress);
        older.join().unwrap();
        assert_eq!(last_ready(backend.poll()), Some(false), "the older worker's report has the new settings: a portable zip is planned");
    }

    #[test]
    fn download_size_reads_the_cached_plan_and_reports_clear_missing_assets() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        assert_eq!(backend.download_size(APP), None);
        ready(&mut backend);
        let (kind, size) = backend.download_size(APP).unwrap();
        assert_eq!(size, 3);
        assert_eq!(backend.download_size("unknown"), None);

        let mut latest = release("0.6.0");
        for asset in &mut latest.assets {
            asset.size = 0;
        }
        backend.shared.latest.lock().unwrap().insert(APP.to_string(), latest);
        // Reading from a frame neither discovers installers nor replans the download.
        assert_eq!(backend.download_size(APP), Some((kind, 3)));
        ready(&mut backend);
        assert_eq!(backend.download_size(APP), Some((kind, 0)));

        backend.shared.latest.lock().unwrap().insert(APP.to_string(), Release::default());
        ready(&mut backend);
        assert_eq!(backend.download_size(APP), None);
    }

    #[cfg(windows)]
    #[test]
    fn download_size_respects_preferences_and_the_installed_package_format() {
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), Platform { os: Os::Windows, arch: Arch::X64 }, None);
        let mut latest = release("0.5.0");
        for asset in &mut latest.assets {
            asset.size = if asset.name.ends_with(".msi") { 100 } else { 200 };
        }
        backend.shared.latest.lock().unwrap().insert(APP.to_string(), latest);
        // A worker started planning the default MSI before the user chose portable ZIP.
        let old_revision = backend.shared.download_revision(APP);
        let options = BackendOptions { kinds: vec![PackageKind::PortableZip], ..Default::default() };
        backend.send(Command::Configure(options));
        // Configuration now replans off the UI thread; observe the new answer before testing stale reports.
        wait_until(|| backend.download_size(APP) == Some((PackageKind::PortableZip, 200)));
        backend.poll();
        assert!(!backend.shared.report_download(APP, old_revision, Some((PackageKind::Msi, 100)), None, false));
        assert!(!backend.poll().iter().any(|event| matches!(event, Event::Paused { app, .. } | Event::ReadyToInstall { app, .. } if app == APP)));
        assert_eq!(backend.download_size(APP), Some((PackageKind::PortableZip, 200)));
        backend.shared.installed.lock().unwrap().insert(
            APP.to_string(),
            Installed {
                version: None,
                source: InstallSource::Msi,
                location: r"C:\Apps\Lienzo\lienzo.exe".into(),
                package: None,
                externally_managed: false,
                origin: None,
                installation: None,
            },
        );
        report_partial(SuiteApp::by_id(APP).unwrap(), &backend.shared, &backend.paths, backend.platform);
        assert_eq!(backend.download_size(APP), Some((PackageKind::Msi, 100)));
    }

    #[test]
    fn a_plan_report_retries_when_a_release_changes_while_it_is_worked_out() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        let shared: Arc<Mutex<Option<std::sync::Weak<Shared>>>> = Arc::new(Mutex::new(None));
        let shared_for_waker = shared.clone();
        let changed = AtomicBool::new(false);
        Arc::get_mut(&mut backend.shared).unwrap().waker = Arc::new(move || {
            // The installability publication is between capturing the revision and the plan.
            if !changed.swap(true, Ordering::SeqCst) {
                let shared = shared_for_waker.lock().unwrap().as_ref().unwrap().upgrade().unwrap();
                let mut latest = release("0.6.0");
                for asset in &mut latest.assets {
                    asset.size = 300;
                }
                shared.store_latest(APP, &latest);
            }
        });
        *shared.lock().unwrap() = Some(Arc::downgrade(&backend.shared));

        report_partial(SuiteApp::by_id(APP).unwrap(), &backend.shared, &backend.paths, Some(platform));
        assert_eq!(backend.download_size(APP).map(|(_, size)| size), Some(300));
        let events = backend.poll();
        assert!(events.iter().any(|e| matches!(e, Event::Paused { app, download: None } if app == APP)));
        assert_eq!(last_ready(events), Some(false));
    }

    #[test]
    fn a_new_release_rejects_a_slow_workers_older_download_size() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        ready(&mut backend);
        let old_plan = backend.download_size(APP);
        let old_revision = backend.shared.download_revision(APP);
        let mut latest = release("0.6.0");
        for asset in &mut latest.assets {
            asset.size = 300;
        }
        check_app(SuiteApp::by_id(APP).unwrap(), Some(LatestRelease::Found(latest)), &backend.shared, backend.platform);
        assert_eq!(backend.download_size(APP), None);
        ready(&mut backend);
        let fresh = backend.download_size(APP).unwrap();
        assert_eq!(fresh.1, 300);
        backend.poll();
        assert!(!backend.shared.report_download(APP, old_revision, old_plan, None, false));
        assert!(backend.poll().is_empty());
        assert_eq!(backend.download_size(APP), Some(fresh));
    }

    #[test]
    fn a_kept_download_is_reported_and_can_be_thrown_away() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        assert_eq!(ready(&mut backend), Some(false));
        let dest = keep_file(&backend);
        assert_eq!(ready(&mut backend), Some(true));

        backend.send(Command::Cancel { app: APP.to_string() });
        assert!(!dest.exists());
        assert_eq!(last_ready(backend.poll()), Some(false));
    }

    #[test]
    fn changing_the_settings_reports_kept_downloads_again() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        keep_file(&backend);
        assert_eq!(ready(&mut backend), Some(true));
        // The planned file changed under the UI; only a new report can tell it.
        backend.shared.latest.lock().unwrap().insert(APP.to_string(), release("0.6.0"));
        backend.send(Command::Configure(BackendOptions::default()));
        // Worked out off the UI thread: the report follows shortly.
        let mut seen = None;
        wait_until(|| {
            seen = last_ready(backend.poll()).or(seen);
            seen.is_some()
        });
        assert_eq!(seen, Some(false));
    }

    #[test]
    fn an_unchanged_refresh_keeps_the_download_size_while_other_results_wait() {
        struct Unchanged {
            saving: mpsc::Sender<()>,
            finish: Mutex<mpsc::Receiver<()>>,
        }
        impl ReleaseSource for Unchanged {
            fn latest(&self, repo: &str, _prereleases: bool) -> Answer {
                let latest = if SuiteApp::by_id(APP).is_some_and(|app| app.repo == repo) {
                    LatestRelease::Found(release("0.5.0"))
                } else {
                    LatestRelease::None
                };
                Answer { latest, source: Source::GitHubApi }
            }
            fn cached(&self, _repo: &str, _prereleases: bool) -> Option<LatestRelease> {
                None
            }
            fn rate_limited_until(&self) -> Option<i64> {
                None
            }
            fn save(&self) {
                let _ = self.saving.send(());
                // Disconnecting the sender also releases this gate if the test panics.
                let _ = self.finish.lock().unwrap().recv();
            }
        }
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let (saving, saved) = mpsc::channel();
        let (finish, finished) = mpsc::channel();
        let mut backend = backend(dir.path(), platform, Some(Arc::new(Unchanged { saving, finish: Mutex::new(finished) })));
        backend.http = Some(Http::new().unwrap());
        backend.shared.options.lock().unwrap().prereleases = true;
        ready(&mut backend);
        let before = backend.download_size(APP);
        assert!(before.is_some());
        let (wake, waking) = mpsc::channel();
        Arc::get_mut(&mut backend.shared).unwrap().waker = Arc::new(move || {
            let _ = wake.send(());
        });

        backend.send(Command::CheckForUpdates { force: true });
        saved.recv_timeout(Duration::from_secs(20)).unwrap();
        // Latest was delivered, but the batch's final report_partial loop cannot run yet.
        let during = backend.download_size(APP);
        let pending = backend.poll();
        drop(finish);
        let mut complete = false;
        while !complete {
            waking.recv_timeout(Duration::from_secs(20)).unwrap();
            complete = backend.poll().iter().any(|event| matches!(event, Event::CheckFinished { .. }));
        }
        assert!(pending.iter().any(|event| matches!(event, Event::Latest { app, latest: LatestRelease::Found(_), .. } if app == APP)));
        assert!(!pending.iter().any(|event| matches!(event, Event::CheckFinished { .. })));
        assert_eq!(during, before, "an identical release must keep its known size throughout Refresh");
    }

    #[test]
    fn unchanged_detection_keeps_the_plan_but_a_changed_copy_rejects_old_reports() {
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, None);
        ready(&mut backend);
        let plan = backend.download_size(APP);
        let revision = backend.shared.download_revision(APP);
        publish_detected(SuiteApp::by_id(APP).unwrap(), None, &backend.shared, Some(platform));
        assert_eq!(backend.download_size(APP), plan);

        publish_detected(SuiteApp::by_id(APP).unwrap(), Some(copy("0.4.0")), &backend.shared, Some(platform));
        assert_eq!(backend.download_size(APP), None);
        assert!(!backend.shared.report_download(APP, revision, plan, None, false));
        let revision = backend.shared.download_revision(APP);
        assert!(backend.shared.report_download(APP, revision, plan, None, false));
        publish_detected(SuiteApp::by_id(APP).unwrap(), Some(copy("0.4.0")), &backend.shared, Some(platform));
        assert_eq!(backend.download_size(APP), plan);

        publish_detected(SuiteApp::by_id(APP).unwrap(), Some(copy("0.5.0")), &backend.shared, Some(platform));
        assert_eq!(backend.download_size(APP), None);
        assert!(!backend.shared.report_download(APP, revision, plan, None, false));
    }

    #[test]
    fn a_check_that_finds_a_newer_release_ends_the_retry_offer() {
        struct Newer;
        impl ReleaseSource for Newer {
            fn latest(&self, repo: &str, _prereleases: bool) -> crate::source::Answer {
                let found = SuiteApp::by_id(APP).is_some_and(|a| a.repo == repo);
                let latest = if found { LatestRelease::Found(release("0.6.0")) } else { LatestRelease::None };
                crate::source::Answer { latest, source: Source::GitHubApi }
            }
            fn cached(&self, _repo: &str, _prereleases: bool) -> Option<LatestRelease> {
                None
            }
            fn rate_limited_until(&self) -> Option<i64> {
                None
            }
            fn save(&self) {}
        }
        let Some(platform) = native_platform() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut backend = backend(dir.path(), platform, Some(Arc::new(Newer)));
        // A check needs a client; with pre-releases on it asks only `Newer`, never GitHub's API.
        backend.http = Some(Http::new().unwrap());
        backend.shared.options.lock().unwrap().prereleases = true;
        keep_file(&backend);
        assert_eq!(ready(&mut backend), Some(true));

        backend.send(Command::CheckForUpdates { force: true });
        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !events.iter().any(|e| matches!(e, Event::CheckFinished { .. })) {
            assert!(Instant::now() < deadline, "the check never finished");
            std::thread::sleep(Duration::from_millis(10));
            events.extend(backend.poll());
        }
        assert_eq!(last_ready(events), Some(false));
    }

    fn copy(v: &str) -> Installed {
        Installed {
            version: version::parse(v),
            source: InstallSource::Deb,
            location: String::new(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        }
    }

    #[test]
    fn a_failed_job_counts_as_installed_only_when_the_new_version_arrived() {
        let latest = version::parse("0.5.0").unwrap();
        // One app of a shared installer call went in before another failed the call.
        assert!(landed(Some(&copy("0.4.1")), Some(&copy("0.5.0")), &latest));
        assert!(landed(None, Some(&copy("0.5.0")), &latest));
        // Nothing changed, so the error stands.
        assert!(!landed(Some(&copy("0.4.1")), Some(&copy("0.4.1")), &latest));
        assert!(!landed(None, None, &latest));
        // It was current before the job ran: the job did not install it.
        assert!(!landed(Some(&copy("0.5.0")), Some(&copy("0.5.0")), &latest));
        // A copy whose version can't be read proves nothing.
        let unreadable = Installed { version: None, ..copy("0.5.0") };
        assert!(!landed(Some(&copy("0.4.1")), Some(&unreadable), &latest));
    }

    #[test]
    fn only_a_failed_shared_call_can_count_as_installed() {
        let latest = version::parse("0.5.0").unwrap();
        let (before, after) = (Some(copy("0.4.1")), Some(copy("0.5.0")));
        let landed_after = |error: &str| landed_despite(error, before.as_ref(), after.as_ref(), &latest);
        // An app's own failure stands, even if its new version shows up (an MSI or AppImage that
        // reported an error, a cancelled or dismissed prompt, a pause).
        for error in ["the Windows Installer stopped with error 1603", "couldn't copy the AppImage", CANCELLED, INSTALL_CANCELLED, PAUSED] {
            assert!(!landed_after(error), "{error}");
        }
        // A call shared with other apps may have installed this one before it failed.
        let shared = "the system installer stopped with error 100 while handling 2 apps together. Try them one at a time.";
        assert_eq!(landed_after(shared), cfg!(target_os = "linux"));
        assert!(!landed_despite(shared, before.as_ref(), before.as_ref(), &latest));
    }

    #[test]
    fn the_launcher_gives_up_waiting_and_names_the_busy_apps() {
        let jobs = Mutex::new(HashMap::new());
        for id in [LAUNCHER_ID, "lienzo"] {
            jobs.lock().unwrap().insert(id.to_string(), Arc::new(Stop::default()));
        }
        let restarting = AtomicBool::new(false);
        let error = claim_restart(&jobs, &restarting, &Stop::default(), Duration::from_millis(300), &mut || {}).unwrap_err();
        assert!(error.contains("Lienzo was still installing") && error.contains("cancel it"), "{error}");
        assert!(!restarting.load(Ordering::SeqCst));
        assert!(restart_blocked(vec!["Toma", "Letra"]).contains("Letra, Toma were still installing"));
    }

    #[test]
    fn the_launcher_restarts_only_once_every_other_job_has_ended() {
        let jobs = Mutex::new(HashMap::new());
        for id in [LAUNCHER_ID, "lienzo", "toma"] {
            jobs.lock().unwrap().insert(id.to_string(), Arc::new(Stop::default()));
        }
        let restarting = AtomicBool::new(false);
        let stop = Stop::default();
        let (waiting, waits) = mpsc::channel();
        std::thread::scope(|scope| {
            let (jobs, restarting, stop) = (&jobs, &restarting, &stop);
            let claim = scope.spawn(move || claim_restart(jobs, restarting, stop, RESTART_WAIT, &mut || waiting.send(()).unwrap()));
            waits.recv().unwrap();
            // One install ends; another is still running or queued.
            jobs.lock().unwrap().remove("lienzo");
            std::thread::sleep(Duration::from_millis(250));
            assert!(!claim.is_finished() && !restarting.load(Ordering::SeqCst));
            jobs.lock().unwrap().remove("toma");
            assert_eq!(claim.join().unwrap(), Ok(()));
        });
        assert!(restarting.load(Ordering::SeqCst));
        assert!(waits.try_recv().is_err(), "it says it is waiting once");

        // With nothing else running there is nothing to wait for.
        let restarting = AtomicBool::new(false);
        assert_eq!(claim_restart(&jobs, &restarting, &stop, RESTART_WAIT, &mut || panic!("it never waits")), Ok(()));
        assert!(restarting.load(Ordering::SeqCst));
    }

    #[test]
    fn a_cancelled_launcher_update_stops_waiting_and_claims_nothing() {
        let jobs =
            Mutex::new(HashMap::from([(LAUNCHER_ID.to_string(), Arc::new(Stop::default())), ("lienzo".to_string(), Arc::new(Stop::default()))]));
        let restarting = AtomicBool::new(false);
        let stop = Stop::default();
        let (waiting, waits) = mpsc::channel();
        std::thread::scope(|scope| {
            let (jobs, restarting, stop) = (&jobs, &restarting, &stop);
            let claim = scope.spawn(move || claim_restart(jobs, restarting, stop, RESTART_WAIT, &mut || waiting.send(()).unwrap()));
            waits.recv().unwrap();
            stop.cancel();
            assert_eq!(claim.join().unwrap(), Err(CANCELLED.to_string()));
        });
        assert!(!restarting.load(Ordering::SeqCst));
    }

    #[test]
    fn a_launcher_release_without_a_file_for_this_copy_is_offered_as_a_download() {
        use crate::self_update::Install;
        // Nodalia Launcher v0.17.0's macOS file: the disk image only.
        let name = "nodalia-launcher-0.17.0-macos-universal.dmg";
        let release = Release {
            assets: vec![Asset { name: name.into(), size: 1, url: format!("https://example.invalid/{name}"), sha256: None }],
            ..Release::default()
        };
        let mac = Platform { os: Os::MacOs, arch: Arch::Arm64 };
        for (install, updates) in [(Install::MacBundle("/Applications/Nodalia Launcher.app".into()), true), (Install::Managed, false)] {
            let dir = tempfile::tempdir().unwrap();
            let mut backend = backend(dir.path(), mac, None);
            backend.shared.self_install.set(Some(install.clone())).unwrap();
            backend.shared.self_updating.set(true).unwrap();
            *backend.shared.launcher_release.lock().unwrap() = Some((release.clone(), String::new()));
            assert_eq!(backend.capabilities().self_update, updates, "{install:?}");
            if !updates {
                backend.update_launcher();
                let events = backend.poll();
                assert!(
                    matches!(&events[..], [Event::JobFinished { app, result: Err(_) }] if app == LAUNCHER_ID),
                    "it refuses without starting a download: {events:?}"
                );
                assert!(backend.shared.jobs.lock().unwrap().is_empty());
            }
        }
    }
}
