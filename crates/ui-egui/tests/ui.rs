//! Drives the launcher UI against a fake backend: what's shown for each status, and which
//! commands the buttons send. `NODALIA_LAUNCHER_SNAPSHOTS=<dir>` also saves screenshots; run that
//! with `-- --test-threads=1`, as rendering on several threads at once can crash Vulkan loaders
//! older than 1.4.350 (KhronosGroup/Vulkan-Loader#1863).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use nodalia_launcher_core::protocol::{
    Backend, CANCELLED, Capabilities, Command, Event, INSTALL_CANCELLED, JobPhase, LatestRelease, PausedDownload, Source,
};
use nodalia_launcher_core::release::{Asset, Release};
use nodalia_launcher_core::status::InstallSource;
use nodalia_launcher_core::{Arch, Installed, Os, Platform, SUITE, version};
use nodalia_launcher_ui_egui::settings::ViewMode;
use nodalia_launcher_ui_egui::{Host, LauncherUi, Settings};

const NOW: i64 = 1_791_500_000;

struct Fake {
    events: Vec<Event>,
    sent: Rc<RefCell<Vec<Command>>>,
    manage: bool,
    downloads: std::collections::BTreeMap<String, (nodalia_launcher_core::PackageKind, u64)>,
    clock: Rc<Cell<i64>>,
}

impl Backend for Fake {
    fn send(&mut self, command: Command) {
        self.sent.borrow_mut().push(command);
    }
    fn poll(&mut self) -> Vec<Event> {
        for event in &self.events {
            if let Event::Latest { app, latest: LatestRelease::Found(release), .. } = event
                && let Some(def) = nodalia_launcher_core::SuiteApp::by_id(app)
            {
                let platform = self.platform().unwrap();
                let stems: Vec<_> = def.stems().collect();
                if let Some((asset, kind)) = nodalia_launcher_core::platform::pick_asset(release, &stems, platform, platform.default_kinds()) {
                    self.downloads.insert(app.clone(), (kind, asset.size));
                }
            }
        }
        std::mem::take(&mut self.events)
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { manage_apps: self.manage, reveal_label: "Show in Finder", self_update: self.manage, desktop_shortcuts: false }
    }
    fn platform(&self) -> Option<Platform> {
        Some(Platform { os: Os::MacOs, arch: Arch::Arm64 })
    }
    fn now(&self) -> i64 {
        self.clock.get()
    }
    fn download_size(&self, app: &str) -> Option<(nodalia_launcher_core::PackageKind, u64)> {
        self.downloads.get(app).copied()
    }
}

struct PlatformFake {
    inner: Fake,
    platform: Platform,
}
impl Backend for PlatformFake {
    fn send(&mut self, command: Command) {
        self.inner.send(command);
    }
    fn poll(&mut self) -> Vec<Event> {
        self.inner.poll()
    }
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }
    fn platform(&self) -> Option<Platform> {
        Some(self.platform)
    }
    fn now(&self) -> i64 {
        self.inner.now()
    }
}

struct NoHost;
impl Host for NoHost {}

/// A harness that has the launcher's fonts, theme and image loaders before its first pass, and
/// never paints a synthetic cursor into saved images.
fn test_harness_builder<State>(theme: egui::Theme) -> egui_kittest::HarnessBuilder<State> {
    let choice = if theme == egui::Theme::Dark {
        nodalia_launcher_ui_egui::theme::ThemeChoice::Dark
    } else {
        nodalia_launcher_ui_egui::theme::ThemeChoice::Light
    };
    egui_kittest::HarnessBuilder::default()
        // 0.37 defaults: strict accessible names and a panic on a missing glyph. Neither is
        // relaxed here; fixed controls are painter-drawn and dynamic notes/feed text uses the
        // attributed monochrome emoji fonts.
        .with_accessibility_check(true)
        .with_render_cursor(false)
        .with_context_setup(move |ctx| {
            nodalia_launcher_ui_egui::theme::install_fonts(ctx, None);
            egui_extras::install_image_loaders(ctx);
            nodalia_launcher_ui_egui::theme::apply(ctx, choice);
        })
}

fn release(id: &str, v: &str, at: i64) -> Release {
    let name = format!("{id}-{v}-macos-universal.dmg");
    Release {
        tag: format!("v{v}"),
        version: version::parse(v),
        name: format!("v{v}"),
        published_at: Some(at),
        html_url: format!("https://github.com/getnodalia/{id}/releases/tag/v{v}"),
        notes: "## What's Changed\n* Faster things by @a in https://github.com/getnodalia/x/pull/1".into(),
        prerelease: false,
        assets: vec![Asset {
            name: name.clone(),
            size: 150 << 20,
            url: format!("https://github.com/getnodalia/{id}/releases/download/v{v}/{name}"),
            sha256: None,
        }],
    }
}

/// A realistic mix: updates, up-to-date apps, a download in progress, one app with no release.
fn events() -> Vec<Event> {
    let mut ev = Vec::new();
    let versions = [
        ("lienzo", "0.5.0"),
        ("pluma", "0.7.0"),
        ("toma", "0.4.0"),
        ("luz", "0.4.0"),
        ("papel", "0.4.0"),
        ("pulso", "0.6.0"),
        ("pliego", "0.4.0"),
        ("letra", "0.3.0"),
        ("lamina", "0.3.0"),
        ("celdas", "0.3.0"),
        ("forja", "0.3.0"),
    ];
    for (i, (id, v)) in versions.iter().enumerate() {
        ev.push(Event::Latest { app: (*id).into(), latest: LatestRelease::Found(release(id, v, NOW - 86_400 - i as i64 * 600)) });
    }
    ev.push(Event::Latest { app: "vinilo".into(), latest: LatestRelease::None });
    let installed = [("lienzo", "0.4.1"), ("luz", "0.4.0"), ("letra", "0.3.0"), ("pulso", "0.5.0")];
    for app in &SUITE {
        let inst = installed.iter().find(|(id, _)| *id == app.id).map(|(_, v)| Installed {
            version: version::parse(v),
            source: InstallSource::MacApp,
            location: format!("/Applications/{}.app", app.name),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        });
        ev.push(Event::Detected { app: app.id.into(), installed: inst });
    }
    ev.push(Event::Progress { app: "toma".into(), phase: JobPhase::Downloading { done: 48 << 20, total: 131 << 20 } });
    ev.push(Event::Paused { app: "lamina".into(), download: Some(PausedDownload { done: 30 << 20, total: 120 << 20 }) });
    ev.push(Event::Progress { app: "celdas".into(), phase: JobPhase::Reconnecting { done: 10 << 20, total: 90 << 20 } });
    ev.push(Event::ReadyToInstall { app: "forja".into(), ready: true });
    ev.push(Event::CheckFinished { at: NOW - 180, source: Source::GitHubApi, rate_limited_until: None });
    ev
}

fn harness(view: ViewMode, theme: egui::Theme, manage: bool) -> (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>) {
    harness_at(view, theme, manage, 1.0)
}

fn harness_at(view: ViewMode, theme: egui::Theme, manage: bool, scale: f32) -> (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>) {
    harness_with(events(), view, theme, manage, scale)
}

fn harness_with(
    events: Vec<Event>,
    view: ViewMode,
    theme: egui::Theme,
    manage: bool,
    scale: f32,
) -> (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>) {
    harness_full(events, view, theme, manage, scale, nodalia_launcher_ui_egui::TitleBar::Native, None)
}

fn harness_with_title_bar(
    events: Vec<Event>,
    view: ViewMode,
    theme: egui::Theme,
    manage: bool,
    scale: f32,
    bar: nodalia_launcher_ui_egui::TitleBar,
) -> (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>) {
    harness_full(events, view, theme, manage, scale, bar, None)
}

#[allow(clippy::too_many_arguments)] // Test fixture inputs vary independently across existing cases.
/// Builds a harness with the full fixture matrix (events, view, theme, manage mode, scale, title
/// bar, pictures and platform), forces English labels, and returns the recorded
/// commands.
fn harness_full(
    events: Vec<Event>,
    view: ViewMode,
    theme: egui::Theme,
    manage: bool,
    scale: f32,
    bar: nodalia_launcher_ui_egui::TitleBar,
    platform: Option<Platform>,
) -> (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>) {
    struct HeaderHost(nodalia_launcher_ui_egui::TitleBar);
    impl Host for HeaderHost {
        fn title_bar(&self) -> nodalia_launcher_ui_egui::TitleBar {
            self.0
        }
    }
    // Integration tests compile without cfg!(test), so system_lang() picks up the
    // developer's locale. Force English so labels match the assertions.
    nodalia_launcher_ui_egui::i18n::set_test_lang(nodalia_launcher_ui_egui::i18n::Lang::EN);
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent2 = sent.clone();
    let fake = Fake { events, sent: sent2, manage, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) };
    let mut backend: Option<Box<dyn Backend>> = Some(match platform {
        Some(platform) => Box::new(PlatformFake { inner: fake, platform }),
        None => Box::new(fake),
    });
    let settings = Settings { view, notify: false, ..Settings::default() };
    // No `.wgpu()`: it gives every test its own GPU device, and those segfault on parallel test threads (see above).
    // The default renderer creates one only when a test renders.
    let harness = test_harness_builder(theme).with_size(egui::vec2(1180.0, 1200.0)).with_pixels_per_point(scale).with_theme(theme).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                let mut s = settings.clone();
                s.theme = if theme == egui::Theme::Dark {
                    nodalia_launcher_ui_egui::theme::ThemeChoice::Dark
                } else {
                    nodalia_launcher_ui_egui::theme::ThemeChoice::Light
                };
                *state = Some(LauncherUi::new(ui.ctx(), b, s));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut HeaderHost(bar));
                l.ui(ui, &mut HeaderHost(bar));
            }
        },
        None,
    );
    (harness, sent)
}

/// Save a PNG under `NODALIA_LAUNCHER_SNAPSHOTS` when that is set. `name` is a test-owned file
/// stem (the test names and sizes make each one unique), never a path: a separator is rejected so
/// no caller can write outside the evidence directory.
fn snapshot(h: &mut Harness<'_, Option<LauncherUi>>, name: &str) {
    let Some(dir) = std::env::var_os("NODALIA_LAUNCHER_SNAPSHOTS") else { return };
    assert!(
        !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..",
        "snapshot name must be one pathless file stem, got {name:?}"
    );
    let img = h.render().unwrap();
    img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
}

/// A managing macOS fixture with a fresh Lienzo release and a scripted folder picker, for
/// the install-folder flow.
fn folder_harness(picked: Rc<RefCell<Option<String>>>) -> (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>) {
    struct Picker(Rc<RefCell<Option<String>>>);
    impl Host for Picker {
        fn pick_folder(&mut self, _initial: Option<&str>) -> Option<String> {
            self.0.borrow_mut().take()
        }
    }
    nodalia_launcher_ui_egui::i18n::set_test_lang(nodalia_launcher_ui_egui::i18n::Lang::EN);
    let sent = Rc::new(RefCell::new(Vec::new()));
    let mut backend = Some(Fake {
        events: vec![
            Event::Latest { app: "lienzo".into(), latest: LatestRelease::Found(release("lienzo", "0.6.0", NOW)) },
            Event::Detected { app: "lienzo".into(), installed: None },
        ],
        sent: sent.clone(),
        manage: true,
        downloads: Default::default(),
        clock: Rc::new(Cell::new(NOW)),
    });
    let mut host = Picker(picked);
    let harness = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 900.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if let Some(backend) = backend.take() {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(backend), Settings { notify: false, ..Default::default() }));
            }
            if let Some(launcher) = state {
                launcher.logic(ui.ctx(), &mut host);
                launcher.ui(ui, &mut host);
            }
        },
        None,
    );
    (harness, sent)
}

fn install_commands(sent: &RefCell<Vec<Command>>) -> Vec<Command> {
    sent.borrow().iter().filter(|c| !matches!(c, Command::CheckForUpdates { .. })).cloned().collect()
}

#[test]
fn app_and_launcher_offer_version_choices_without_starting_an_install() {
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(3);
    h.get_by_label("Choose version").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Version");
    assert!(sent.borrow().iter().any(|c| matches!(c, Command::ListVersions { app } if app == "lienzo")));
    assert!(!sent.borrow().iter().any(|c| matches!(c, Command::Install { .. })));
    h.get_by_label("Close").click();
    h.run_steps(3);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Launcher version").click();
    h.run_steps(3);
    h.get_by_label("Latest release");
    h.get_by_label("Nightly (none published yet)");
    assert!(h.query_by_label("Launcher versions").is_none(), "version selection belongs in Settings, not another dialog");
    assert!(sent.borrow().iter().any(|c| matches!(c, Command::ListVersions { app } if app == nodalia_launcher_core::catalog::LAUNCHER_ID)));
}

#[test]
fn version_choices_have_room_and_nightly_scrolls_with_the_versions() {
    let releases: Vec<_> = (1..=25)
        .rev()
        .map(|v| {
            let mut r = release("nodalia-launcher", &format!("0.{v}.0"), NOW);
            r.assets[0].sha256 = Some("0".repeat(64));
            r
        })
        .collect();
    let mut ev = events();
    ev.push(Event::Versions { app: nodalia_launcher_core::catalog::LAUNCHER_ID.into(), result: Ok(releases) });
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.set_size(egui::vec2(1180.0, 1100.0));
    h.run_steps(6);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Launcher version").click();
    h.run_steps(5);
    let nightly_before = h.get_by_label("Nightly (none published yet)").rect();
    // The newest release carries the label; there is no separate "Latest release" row.
    let first = h.get_by_label("v0.25.0 (latest release)").rect();
    assert!(h.query_by_label("Latest release").is_none(), "the latest version is labelled in place");
    assert!(nightly_before.height() >= 40.0, "dropdown rows need a comfortable target: {nightly_before:?}");
    // Rows touch, so a selected row's highlight fills its whole slot and the spacing stays even.
    assert!((first.top() - nightly_before.bottom()).abs() < 1.0, "rows sit at an even pitch: {nightly_before:?} {first:?}");
    h.event(egui::Event::PointerMoved(first.center()));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -250.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    let nightly_after = h.get_by_label("Nightly (none published yet)").rect();
    assert!(nightly_after.top() < nightly_before.top() - 80.0, "Nightly must scroll with release rows: {nightly_before:?} -> {nightly_after:?}");
}

/// A backend that answers ListVersions, unlike the shared Fake: the dialog clears the cached
/// list when it opens, so the choice only appears once the reply arrives.
struct VersionsFake {
    events: Rc<RefCell<Vec<Event>>>,
    sent: Rc<RefCell<Vec<Command>>>,
    answer: Vec<Release>,
    platform: Platform,
    launcher_kind: Option<nodalia_launcher_core::PackageKind>,
}

impl Backend for VersionsFake {
    fn send(&mut self, command: Command) {
        if let Command::ListVersions { app } = &command {
            self.events.borrow_mut().push(Event::Versions { app: app.clone(), result: Ok(self.answer.clone()) });
        }
        self.sent.borrow_mut().push(command);
    }
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut *self.events.borrow_mut())
    }
    fn launcher_kind(&self) -> Option<nodalia_launcher_core::PackageKind> {
        self.launcher_kind
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { manage_apps: true, reveal_label: "Show in Finder", self_update: true, desktop_shortcuts: false }
    }
    fn platform(&self) -> Option<Platform> {
        Some(self.platform)
    }
    fn now(&self) -> i64 {
        NOW
    }
}

/// A Lienzo release with a published checksum, so the selector offers it.
fn signed_lienzo(v: &str) -> Release {
    let mut r = release("lienzo", v, NOW);
    r.assets[0].sha256 = Some("0".repeat(64));
    r
}

type VersionsHarness = (Harness<'static, Option<LauncherUi>>, Rc<RefCell<Vec<Command>>>, Rc<RefCell<Vec<Event>>>);

/// The machine a [`VersionsFake`] harness stands for.
struct Machine {
    platform: Platform,
    theme: egui::Theme,
    preferred: Vec<nodalia_launcher_core::PackageKind>,
    /// Events after the suite's (such as how Lienzo is installed).
    more: Vec<Event>,
    /// What the backend says the running launcher was installed as.
    launcher_kind: Option<nodalia_launcher_core::PackageKind>,
}

impl Default for Machine {
    fn default() -> Self {
        Machine {
            platform: Platform { os: Os::MacOs, arch: Arch::Arm64 },
            theme: egui::Theme::Dark,
            preferred: Settings::default().preferred_kinds,
            more: vec![],
            launcher_kind: None,
        }
    }
}

/// The suite on `machine` with `latest` as Lienzo's checked release (if any) and `answer` as
/// every version list.
fn versions_harness(machine: Machine, latest: Option<Release>, answer: Vec<Release>) -> VersionsHarness {
    struct DummyHost;
    impl Host for DummyHost {}
    let Machine { platform, theme, preferred, more, launcher_kind } = machine;
    let mut ev = events();
    ev.extend(more);
    if let Some(latest) = latest {
        ev.push(Event::Latest { app: "lienzo".into(), latest: LatestRelease::Found(latest) });
    }
    let sent = Rc::new(RefCell::new(Vec::new()));
    let queue = Rc::new(RefCell::new(ev));
    let mut backend = Some(VersionsFake { events: queue.clone(), sent: sent.clone(), answer, platform, launcher_kind });
    let mut h = test_harness_builder(theme).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if let Some(backend) = backend.take() {
                let settings = Settings { notify: false, preferred_kinds: preferred.clone(), ..Default::default() };
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(backend), settings));
            }
            if let Some(launcher) = state {
                launcher.logic(ui.ctx(), &mut DummyHost);
                launcher.ui(ui, &mut DummyHost);
            }
        },
        None,
    );
    h.run_steps(6);
    (h, sent, queue)
}

/// The suite (Lienzo 0.4.1 installed) with `latest` as Lienzo's checked release and
/// `answer` as its published versions, opened on Lienzo's expanded version menu.
fn version_menu(latest: Release, answer: Vec<Release>) -> VersionsHarness {
    version_menu_on(Machine::default(), latest, answer)
}

/// [`version_menu`] on `machine`.
fn version_menu_on(machine: Machine, latest: Release, answer: Vec<Release>) -> VersionsHarness {
    let (mut h, sent, queue) = versions_harness(machine, Some(latest), answer);
    // Not installed, Lienzo's card is further down.
    h.get_by_label("More actions for Lienzo").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(3);
    h.get_by_label("Choose version").click();
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Version").click();
    h.run_steps(4);
    (h, sent, queue)
}

/// The launcher's own version list in Settings on `machine`, expanded, with `answer` published.
fn launcher_menu_on(machine: Machine, answer: Vec<Release>) -> VersionsHarness {
    let (mut h, sent, queue) = versions_harness(machine, None, answer);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Launcher version").click();
    h.run_steps(5);
    (h, sent, queue)
}

#[test]
fn closing_the_dialog_without_installing_drops_the_chosen_version() {
    // As on a real machine: the checked release is still the newest when the older one is picked.
    let (mut h, sent, _) = version_menu(signed_lienzo("0.5.0"), vec![signed_lienzo("0.5.0"), signed_lienzo("0.4.0")]);
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, "v0.4.0").next().unwrap().click();
    h.run_steps(4);
    assert!(h.query_all_by_label("Install version v0.4.0").count() > 0, "the dialog offers the chosen version");
    sent.borrow_mut().clear();
    h.get_by_label("Close").click();
    h.run_steps(4);
    assert!(h.query_by_label("Version").is_none(), "Close closes the dialog");
    assert!(h.query_by_label("Install version v0.4.0").is_none(), "the card goes back to its own action");
    let configured = sent.borrow().iter().rev().find_map(|c| match c {
        Command::Configure(o) => Some(o.versions.get("lienzo").cloned()),
        _ => None,
    });
    assert_eq!(configured, Some(None), "the choice from before (latest release) is restored");
    assert!(sent.borrow().iter().any(|c| matches!(c, Command::CheckForUpdates { force: true })), "the restored choice is checked again");
}

#[test]
fn choosing_an_app_version_turns_its_main_action_into_install() {
    // A newer release takes the "latest release" label, so v0.4.0 is a plain pin.
    let pin = signed_lienzo("0.4.0");
    let (mut h, sent, queue) = version_menu(pin.clone(), vec![signed_lienzo("0.5.0"), pin]);
    assert!(h.query_all_by_role_and_label(egui::accesskit::Role::Button, "v0.5.0 (latest release)").next().is_some());
    sent.borrow_mut().clear();
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, "v0.4.0").next().unwrap().click();
    h.run_steps(4);
    assert!(
        sent.borrow().iter().any(|c| matches!(c, Command::CheckForUpdates { force: true })),
        "selection must refresh the release used by Install"
    );
    assert!(!sent.borrow().iter().any(|c| matches!(c, Command::Install { .. })), "selection itself must not install");
    h.run_steps(4);
    assert!(h.query_by_label("Version").is_some(), "choosing a version keeps the dialog open");
    // The dialog's own Install sits on its Close button's row; the card shows the same label.
    let row = h.get_by_label("Close").rect().center().y;
    h.get_all_by_label("Install version v0.4.0")
        .find(|n| (n.rect().center().y - row).abs() < 4.0)
        .expect("the dialog offers Install for the chosen version")
        .click();
    h.run_steps(4);
    assert!(sent.borrow().iter().any(|c| matches!(c, Command::Install { app } if app == "lienzo")));
    assert!(!sent.borrow().iter().any(|c| matches!(c, Command::Launch { app, .. } if app == "lienzo")));
    assert!(h.query_by_label("Version").is_none(), "installing closes the dialog");
    assert!(h.query_by_label("Install version v0.4.0").is_some(), "installing from the dialog keeps the chosen version");
    // Once the pinned version is the installed copy, the card opens it again.
    let opens = h.query_all_by_label("Open").count();
    queue.borrow_mut().push(Event::Detected {
        app: "lienzo".into(),
        installed: Some(Installed {
            version: version::parse("0.4.0"),
            source: InstallSource::MacApp,
            location: "/Applications/Lienzo.app".into(),
            package: None,
            externally_managed: false,
            origin: None,
            installation: None,
        }),
    });
    h.run_steps(4);
    assert!(h.query_by_label("Install version v0.4.0").is_none(), "an installed pin is not offered again");
    assert_eq!(h.query_all_by_label("Open").count(), opens + 1, "Lienzo shows Open");
}

/// The date and size painted beside the version row named `label`, and their colour. `None` when
/// the row paints nothing beside its name.
fn painted_facts(h: &Harness<'static, Option<LauncherUi>>, label: &str) -> Option<(String, egui::Color32)> {
    fn texts(shape: &egui::Shape, out: &mut Vec<(egui::Pos2, String, egui::Color32)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| texts(s, out)),
            egui::Shape::Text(t) => out.push((t.pos, t.galley.text().to_string(), t.override_text_color.unwrap_or(t.fallback_color))),
            _ => {}
        }
    }
    let row = h.query_all_by_role_and_label(egui::accesskit::Role::Button, label).next()?.rect();
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|clipped| texts(&clipped.shape, &mut found));
    // Shapes come in paint order: what the row paints beside its name comes right after the name,
    // which keeps out the text under the open menu.
    let name = found.iter().rposition(|(pos, text, _)| row.contains(*pos) && text == label).expect("the row paints its name");
    found.into_iter().skip(name + 1).find(|(pos, _, _)| row.contains(*pos)).map(|(_, text, color)| (text, color))
}

/// The accessible description of the version row named `label` (its date and size).
fn row_facts(h: &Harness<'static, Option<LauncherUi>>, label: &str) -> Option<String> {
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, label).next()?.accesskit_node().description()
}

/// WCAG contrast ratio of two opaque colours.
fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
    let lum = |c: egui::Color32| {
        let ch = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn version_rows_show_the_release_date_and_download_size() {
    // A release that publishes neither a date nor its size keeps a plain row.
    let mut undated = signed_lienzo("0.4.0");
    undated.published_at = None;
    undated.assets[0].size = 0;
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        let answer = vec![signed_lienzo("0.5.0"), undated.clone(), signed_lienzo("0.3.0")];
        let (mut h, _, _) = version_menu_on(Machine { theme, ..Machine::default() }, signed_lienzo("0.5.0"), answer);
        // The row's name stays the version; its date and the size of this Mac's download ride beside
        // it, drawn and as its description. The day itself depends on the zone the test runs in.
        let facts = row_facts(&h, "v0.5.0 (latest release)").unwrap_or_default();
        assert!(facts.starts_with("Oct ") && facts.ends_with(", 2026 · 150 MB"), "{theme:?}: {facts:?}");
        let (painted, on_accent) = painted_facts(&h, "v0.5.0 (latest release)").expect("the selected row paints its date and size");
        assert_eq!(painted, facts);
        // The selected row sits on the accent, where the muted grey can't be read: its date and
        // size take the row's own text colour there.
        let style = h.ctx.global_style();
        let (accent, muted) = (style.visuals.selection.bg_fill, nodalia_launcher_ui_egui::theme::Tokens::get(&h.ctx).muted);
        assert_eq!(on_accent, style.visuals.selection.stroke.color, "{theme:?}: the selected row's text colour");
        assert!(contrast(on_accent, accent) >= 4.5, "{theme:?}: {on_accent:?} on {accent:?}");
        assert!(contrast(on_accent, accent) >= contrast(muted, accent) - 0.5, "{theme:?}: {on_accent:?} vs {muted:?} on {accent:?}");
        // Other rows keep the muted colour.
        let (_, plain) = painted_facts(&h, "v0.3.0").expect("an older row paints its date and size");
        assert_eq!(plain, muted, "{theme:?}");
        assert_eq!(row_facts(&h, "v0.4.0"), None, "nothing to add beside a bare tag");
        assert_eq!(painted_facts(&h, "v0.4.0"), None);
        h.check_accessibility();
        if theme == egui::Theme::Dark {
            snapshot(&mut h, "version-list-dates-and-sizes");
        }
    }
}

/// A release named `stem`-`v` with one signed asset per `(name tail, MB)`.
fn release_with(stem: &str, v: &str, assets: &[(&str, u64)]) -> Release {
    let mut r = release(stem, v, NOW);
    r.assets = assets
        .iter()
        .map(|(tail, mb)| {
            let name = format!("{stem}-{v}-{tail}");
            Asset { url: format!("https://x/{name}"), name, size: mb << 20, sha256: Some("0".repeat(64)) }
        })
        .collect();
    r
}

/// A Lienzo release for Windows with an MSI of 120 MB and a portable ZIP of 90 MB.
fn windows_lienzo(v: &str) -> Release {
    release_with("lienzo", v, &[("windows-x64.msi", 120), ("windows-x64-portable.zip", 90)])
}

fn lienzo_installed(source: InstallSource) -> Event {
    let installed = Installed {
        version: version::parse("0.4.1"),
        source,
        location: "C:/Users/me/Lienzo".into(),
        package: None,
        externally_managed: false,
        origin: None,
        installation: None,
    };
    Event::Detected { app: "lienzo".into(), installed: Some(installed) }
}

fn lienzo_not_installed() -> Event {
    Event::Detected { app: "lienzo".into(), installed: None }
}

/// The size is that of the package the backend would install: the installed copy's format first,
/// then the preferred one, so a portable copy shows the ZIP even where the MSI is preferred.
#[test]
fn a_version_row_sizes_the_package_this_install_would_use() {
    use nodalia_launcher_core::PackageKind::{Msi, PortableZip};
    let windows = Platform { os: Os::Windows, arch: Arch::X64 };
    let cases = [
        (Some(InstallSource::Launcher), vec![Msi], "90 MB", "the launcher's portable copy updates from the ZIP"),
        (Some(InstallSource::Msi), vec![PortableZip], "120 MB", "an MSI install updates from the MSI"),
        (None, vec![PortableZip], "90 MB", "a fresh install takes the preferred ZIP"),
        (None, vec![], "120 MB", "a fresh install takes the MSI by default"),
    ];
    for (source, preferred, size, why) in cases {
        let more = vec![source.map_or_else(lienzo_not_installed, lienzo_installed)];
        let machine = Machine { platform: windows, preferred, more, ..Machine::default() };
        let (h, _, _) = version_menu_on(machine, windows_lienzo("0.5.0"), vec![windows_lienzo("0.5.0")]);
        let facts = row_facts(&h, "v0.5.0 (latest release)").unwrap_or_default();
        assert!(facts.ends_with(&format!(" · {size}")), "{why}: {facts:?}");
        assert_eq!(painted_facts(&h, "v0.5.0 (latest release)").map(|(text, _)| text), Some(facts), "{why}");
    }
}

/// On Linux a fresh install's format, and a package's, depend on the package managers the backend
/// finds, which the UI can't see: the row gives the date and no size rather than a guess. An
/// AppImage, which the backend always installs, updates as one, so its size is certain.
#[test]
fn a_linux_version_row_sizes_only_a_certain_package() {
    let linux = Platform { os: Os::Linux, arch: Arch::X64 };
    let r = release_with("lienzo", "0.5.0", &[("linux-x86_64.AppImage", 150), ("linux-x86_64.deb", 100)]);
    let cases = [
        (lienzo_not_installed(), None, "a fresh install depends on the package managers"),
        (lienzo_installed(InstallSource::Deb), None, "a package depends on the package managers"),
        (lienzo_installed(InstallSource::AppImage), Some("150 MB"), "an AppImage updates as an AppImage"),
    ];
    for (detected, size, why) in cases {
        let machine = Machine { platform: linux, more: vec![detected], ..Machine::default() };
        let (h, _, _) = version_menu_on(machine, r.clone(), vec![r.clone()]);
        let facts = row_facts(&h, "v0.5.0 (latest release)").unwrap_or_default();
        let tail = size.map_or_else(|| ", 2026".to_string(), |size| format!(", 2026 · {size}"));
        assert!(facts.starts_with("Oct ") && facts.ends_with(&tail), "{why}: {facts:?}");
    }
}

/// FreeBSD installs only the tarball, so its size is certain.
#[test]
fn a_freebsd_version_row_sizes_the_tarball() {
    let freebsd = Platform { os: Os::FreeBsd, arch: Arch::X64 };
    let r = release_with("lienzo", "0.5.0", &[("freebsd-x86_64.tar.gz", 140)]);
    let machine = Machine { platform: freebsd, more: vec![lienzo_not_installed()], ..Machine::default() };
    let (h, _, _) = version_menu_on(machine, r.clone(), vec![r]);
    let facts = row_facts(&h, "v0.5.0 (latest release)").unwrap_or_default();
    assert!(facts.ends_with(", 2026 · 140 MB"), "{facts:?}");
}

/// The launcher's own list in Settings sizes the launcher's download for this machine.
#[test]
fn the_launcher_version_list_sizes_the_launchers_download() {
    let mut r = release("nodalia-launcher", "0.9.0", NOW);
    r.assets[0].sha256 = Some("0".repeat(64));
    let mut ev = events();
    ev.push(Event::Versions { app: nodalia_launcher_core::catalog::LAUNCHER_ID.into(), result: Ok(vec![r]) });
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(6);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Launcher version").click();
    h.run_steps(5);
    let facts = row_facts(&h, "v0.9.0 (latest release)").unwrap_or_default();
    assert!(facts.starts_with("Oct ") && facts.ends_with(", 2026 · 150 MB"), "{facts:?}");
    assert_eq!(painted_facts(&h, "v0.9.0 (latest release)").map(|(text, _)| text), Some(facts));
}

/// The launcher updates itself from the format it was installed as; until the backend knows it,
/// the manual download stands in, but not on Linux, where any format could be running.
#[test]
fn the_launcher_version_list_sizes_what_its_self_update_downloads() {
    use nodalia_launcher_core::PackageKind::{AppImage, PortableZip};
    let windows = Platform { os: Os::Windows, arch: Arch::X64 };
    let linux = Platform { os: Os::Linux, arch: Arch::X64 };
    let on_windows = release_with("nodalia-launcher", "0.9.0", &[("windows-x64.msi", 120), ("windows-x64-portable.zip", 90)]);
    let on_linux = release_with("nodalia-launcher", "0.9.0", &[("linux-x86_64.AppImage", 150), ("linux-x86_64.deb", 100)]);
    let cases = [
        (windows, Some(PortableZip), &on_windows, Some("90 MB"), "a portable launcher updates from the ZIP"),
        (windows, None, &on_windows, Some("120 MB"), "not known yet: the manual download, the MSI"),
        (linux, Some(AppImage), &on_linux, Some("150 MB"), "an AppImage launcher updates from the AppImage"),
        (linux, None, &on_linux, None, "not known on Linux: no guess"),
    ];
    for (platform, launcher_kind, r, size, why) in cases {
        let (h, _, _) = launcher_menu_on(Machine { platform, launcher_kind, ..Machine::default() }, vec![r.clone()]);
        let facts = row_facts(&h, "v0.9.0 (latest release)").unwrap_or_default();
        let tail = size.map_or_else(|| ", 2026".to_string(), |size| format!(", 2026 · {size}"));
        assert!(facts.starts_with("Oct ") && facts.ends_with(&tail), "{why}: {facts:?}");
    }
}

#[test]
/// The version dialog's Install follows the same answer as the card's "Install version X": for a
/// copy the backend can't replace with the chosen version (installed outside the launcher, say),
/// it is greyed out, says why on hover, and clicking it sends no install.
fn the_version_dialog_wont_install_a_version_the_install_would_refuse() {
    let pin = signed_lienzo("0.4.0");
    let (mut h, sent, queue) = version_menu(pin.clone(), vec![signed_lienzo("0.5.0"), pin]);
    queue.borrow_mut().push(Event::Installable { app: "lienzo".into(), installable: false });
    h.query_all_by_role_and_label(egui::accesskit::Role::Button, "v0.4.0").next().unwrap().click();
    h.run_steps(8);
    sent.borrow_mut().clear();
    let row = h.get_by_label("Close").rect().center().y;
    let install = h
        .get_all_by_label("Install version v0.4.0")
        .find(|n| (n.rect().center().y - row).abs() < 4.0)
        .expect("the dialog shows Install for the chosen version");
    assert!(install.accesskit_node().is_disabled(), "the install would be refused");
    install.hover();
    h.run_steps(4);
    assert!(h.query_all_by_label_contains("can't install version v0.4.0 over this copy").next().is_some(), "it says why");
    h.get_all_by_label("Install version v0.4.0").find(|n| (n.rect().center().y - row).abs() < 4.0).unwrap().click();
    h.run_steps(4);
    assert!(!sent.borrow().iter().any(|c| matches!(c, Command::Install { .. })), "{:?}", sent.borrow());
}

#[test]
/// A custom-folder install opens a confirmation dialog with no command until Install; the
/// chosen root and per-app folder are persisted, Configure precedes Install, and Cancel keeps
/// no override.
fn a_fresh_install_confirms_the_folder_and_configures_before_installing() {
    let picked = Rc::new(RefCell::new(None));
    let (mut h, sent) = folder_harness(picked.clone());
    h.run_steps(8);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    h.get_by_label("Browse…").scroll_to_me();
    h.run_steps(4);
    *picked.borrow_mut() = Some("/Volumes/Nodalia Apps".into());
    h.get_by_label("Browse…").click();
    h.run_steps(3);
    assert_eq!(h.state().as_ref().unwrap().settings.install_locations.root.as_deref(), Some("/Volumes/Nodalia Apps"));
    h.get_by_label("Done").click();
    h.run_steps(3);
    sent.borrow_mut().clear();
    h.get_by_label("Install").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Install").click();
    h.run_steps(4);
    assert!(!sent.borrow().iter().any(|c| matches!(c, Command::Install { .. })));
    h.get_by_label("Apps are placed inside this folder.");
    h.check_accessibility();
    snapshot(&mut h, "install-folder-confirmation");
    *picked.borrow_mut() = Some("/Volumes/Photo".into());
    h.get_by_label("Browse…").click();
    h.run_steps(3);
    assert!(h.state().as_ref().unwrap().settings.install_locations.per_app.is_empty());
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(h.state().as_ref().unwrap().settings.install_locations.per_app.is_empty());
    h.get_by_label("Install").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Install").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Install folder").click();
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Install folder").type_text("/Volumes/Typed Photo");
    h.run_steps(3);
    h.get_by_label("Browse…").click();
    h.run_steps(3);
    assert_eq!(h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Install folder").value().as_deref(), Some("/Volumes/Typed Photo"));
    sent.borrow_mut().clear();
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "Install").last().unwrap().click();
    h.run_steps(2);
    let commands = install_commands(&sent);
    assert!(matches!(commands.first(), Some(Command::Configure(options))
        if options.install_locations.root.as_deref() == Some("/Volumes/Nodalia Apps")
        && options.install_locations.folder("lienzo") == Some("/Volumes/Typed Photo")));
    assert_eq!(commands.get(1), Some(&Command::Install { app: "lienzo".into() }));
}

#[test]
fn use_default_removes_an_app_override_and_settings_can_reset_the_root() {
    let picked = Rc::new(RefCell::new(Some("/Volumes/Photo".into())));
    let (mut h, sent) = folder_harness(picked);
    h.run_steps(8);
    h.state_mut().as_mut().unwrap().settings.install_locations.root = Some("/Volumes/Nodalia Apps".into());
    h.state_mut().as_mut().unwrap().settings.install_locations.per_app.insert("lienzo".into(), "/Volumes/Old Photo".into());
    h.run_steps(2);
    h.get_by_label("Install").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Install").click();
    h.run_steps(3);
    h.get_by_label("Use default").click();
    h.run_steps(2);
    sent.borrow_mut().clear();
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "Install").last().unwrap().click();
    h.run_steps(2);
    assert!(h.state().as_ref().unwrap().settings.install_locations.per_app.is_empty());
    assert!(
        matches!(install_commands(&sent).first(), Some(Command::Configure(options)) if options.install_locations.folder("lienzo") == Some("/Volumes/Nodalia Apps"))
    );
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(3);
    h.get_by_label("Use default").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Use default").click();
    h.run_steps(2);
    assert_eq!(h.state().as_ref().unwrap().settings.install_locations.root, None);
}

/// The install-folder fields' default comes from the host, so a person sees the real folder a
/// fresh install uses (here `/Applications`) instead of only "System default", and a saved root
/// override replaces it. The default is the same value the empty field's hint shows.
#[test]
fn the_install_folder_fields_default_to_the_hosts_answer() {
    struct DefaultHost {
        picked: Rc<RefCell<Option<String>>>,
        initial: Rc<RefCell<Option<Option<String>>>>,
    }
    impl Host for DefaultHost {
        fn pick_folder(&mut self, initial: Option<&str>) -> Option<String> {
            *self.initial.borrow_mut() = Some(initial.map(str::to_string));
            self.picked.borrow_mut().take()
        }
        fn default_install_folder(&mut self, _kind: nodalia_launcher_core::PackageKind, _arch: Option<Arch>) -> Option<String> {
            Some("/Applications".into())
        }
    }
    nodalia_launcher_ui_egui::i18n::set_test_lang(nodalia_launcher_ui_egui::i18n::Lang::EN);
    let sent = Rc::new(RefCell::new(Vec::new()));
    let mut backend = Some(Fake {
        downloads: Default::default(),
        events: vec![
            Event::Latest { app: "lienzo".into(), latest: LatestRelease::Found(release("lienzo", "0.6.0", NOW)) },
            Event::Detected { app: "lienzo".into(), installed: None },
        ],
        sent: sent.clone(),
        manage: true,
        clock: Rc::new(Cell::new(NOW)),
    });
    let picked = Rc::new(RefCell::new(None));
    let initial = Rc::new(RefCell::new(None));
    let mut host = DefaultHost { picked: picked.clone(), initial: initial.clone() };
    let harness = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 900.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if let Some(backend) = backend.take() {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(backend), Settings { notify: false, ..Default::default() }));
            }
            if let Some(launcher) = state {
                launcher.logic(ui.ctx(), &mut host);
                launcher.ui(ui, &mut host);
            }
        },
        None,
    );
    let mut h = harness;
    h.run_steps(8);
    // Settings' install folder starts at the host's default.
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    h.get_by_label("Browse…").scroll_to_me();
    h.run_steps(4);
    h.get_by_label("Browse…").click();
    h.run_steps(3);
    assert_eq!(initial.borrow().as_ref(), Some(&Some("/Applications".into())));
    h.get_by_label("Done").click();
    h.run_steps(3);
    // So does the per-app install dialog's folder, and a saved root override replaces it.
    h.get_by_label("Install").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Install").click();
    h.run_steps(3);
    h.get_by_label("Browse…").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Browse…").click();
    h.run_steps(3);
    assert_eq!(initial.borrow().as_ref(), Some(&Some("/Applications".into())));
    h.state_mut().as_mut().unwrap().settings.install_locations.root = Some("/Volumes/Nodalia Apps".into());
    h.run_steps(3);
    h.get_by_label("Browse…").click();
    h.run_steps(3);
    assert_eq!(initial.borrow().as_ref(), Some(&Some("/Volumes/Nodalia Apps".into())));
    h.check_accessibility();
}

/// A host whose default folder names the package it was asked about (`Msi X86`), and answers
/// nothing when the architecture is unknown, as the desktop host does for an MSI.
/// What Browse was last opened at: `None` until it is clicked.
type BrowsedFrom = Rc<RefCell<Option<Option<String>>>>;

struct PackageHost {
    initial: BrowsedFrom,
}
impl Host for PackageHost {
    fn pick_folder(&mut self, initial: Option<&str>) -> Option<String> {
        *self.initial.borrow_mut() = Some(initial.map(str::to_string));
        None
    }
    fn default_install_folder(&mut self, kind: nodalia_launcher_core::PackageKind, arch: Option<Arch>) -> Option<String> {
        Some(format!("{kind:?} {:?}", arch?))
    }
}

/// A Windows launcher whose apps publish only the named assets, with `preferred` formats saved.
/// Returns the harness and what Browse was last opened at.
fn windows_package_harness(
    apps: &[(&str, &[&str])],
    preferred: Vec<nodalia_launcher_core::PackageKind>,
) -> (Harness<'static, Option<LauncherUi>>, BrowsedFrom) {
    nodalia_launcher_ui_egui::i18n::set_test_lang(nodalia_launcher_ui_egui::i18n::Lang::EN);
    let mut events = Vec::new();
    for (id, assets) in apps {
        let mut latest = release(id, "0.6.0", NOW);
        latest.assets = assets.iter().map(|name| Asset { name: (*name).into(), size: 1, url: format!("https://x/{name}"), sha256: None }).collect();
        events.push(Event::Latest { app: (*id).into(), latest: LatestRelease::Found(latest) });
        events.push(Event::Detected { app: (*id).into(), installed: None });
    }
    let fake = Fake { downloads: Default::default(), events, sent: Rc::default(), manage: true, clock: Rc::new(Cell::new(NOW)) };
    let mut backend: Option<Box<dyn Backend>> = Some(Box::new(PlatformFake { inner: fake, platform: Platform { os: Os::Windows, arch: Arch::X64 } }));
    let initial = Rc::new(RefCell::new(None));
    let mut host = PackageHost { initial: initial.clone() };
    let harness = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 900.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if let Some(backend) = backend.take() {
                let settings = Settings { notify: false, preferred_kinds: preferred.clone(), ..Default::default() };
                *state = Some(LauncherUi::new(ui.ctx(), backend, settings));
            }
            if let Some(launcher) = state {
                launcher.logic(ui.ctx(), &mut host);
                launcher.ui(ui, &mut host);
            }
        },
        None,
    );
    (harness, initial)
}

/// The install dialog's default follows the package the backend will really install: Portable
/// ZIP is preferred, but this release ships only a 32-bit MSI, so the app goes into the 32-bit
/// Program Files and not the launcher's `apps` folder.
#[test]
fn the_install_folder_default_follows_the_package_the_backend_falls_back_to() {
    let (mut h, initial) =
        windows_package_harness(&[("lienzo", &["lienzo-0.6.0-windows-x86.msi"])], vec![nodalia_launcher_core::PackageKind::PortableZip]);
    h.run_steps(8);
    h.get_by_label("Install").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Install").click();
    h.run_steps(3);
    h.get_by_label("Browse…").click();
    h.run_steps(3);
    assert_eq!(initial.borrow().as_ref(), Some(&Some("Msi X86".into())));
}

/// Settings covers every app, so it shows a folder only when every app's package shares one: a
/// 32-bit-only app beside a 64-bit one leaves it at "System default".
#[test]
fn settings_shows_a_default_folder_only_when_every_app_shares_it() {
    let open_settings_browse = |apps: &[(&str, &[&str])]| {
        let (mut h, initial) = windows_package_harness(apps, vec![]);
        h.run_steps(8);
        h.get_by_label("Main menu").click();
        h.run_steps(2);
        h.get_by_label("Settings").click();
        h.run_steps(4);
        h.get_by_label("Browse…").scroll_to_me();
        h.run_steps(4);
        h.get_by_label("Browse…").click();
        h.run_steps(3);
        initial.take()
    };
    let x64 = ["lienzo-0.6.0-windows-x64.msi", "lienzo-0.6.0-windows-x86.msi"];
    let shared = open_settings_browse(&[("lienzo", &x64), ("pluma", &["pluma-0.6.0-windows-x64.msi"])]);
    assert_eq!(shared, Some(Some("Msi X64".into())));
    let mixed = open_settings_browse(&[("lienzo", &x64), ("pluma", &["pluma-0.6.0-windows-x86.msi"])]);
    assert_eq!(mixed, Some(None));
}

#[cfg(windows)]
#[test]
fn windows_package_choice_can_select_portable_and_configures_the_backend() {
    let (mut h, sent) = harness_full(
        events(),
        ViewMode::Grid,
        egui::Theme::Light,
        true,
        1.0,
        nodalia_launcher_ui_egui::TitleBar::Native,
        Some(Platform { os: Os::Windows, arch: Arch::X64 }),
    );
    h.run_steps(8);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    h.get_by_value("Installer (.msi)").click();
    h.run_steps(2);
    h.get_by_label("Portable (.zip)").click();
    h.run_steps(3);
    assert_eq!(h.state().as_ref().unwrap().settings.preferred_kinds, [nodalia_launcher_core::PackageKind::PortableZip]);
    assert!(
        sent.borrow()
            .iter()
            .any(|command| matches!(command, Command::Configure(options) if options.kinds == [nodalia_launcher_core::PackageKind::PortableZip]))
    );
}

/// The narrow 1000 x 700 native window that first showed it: a ComboBox grows to the width its
/// layout offers, so an unconstrained one started at the label's last glyph and painted over
/// "Install new apps as". The test reads the real widget rects, not a snapshot.
#[test]
fn a_narrow_settings_row_keeps_its_control_clear_of_its_label() {
    let (mut h, _sent) = harness_full(
        events(),
        ViewMode::Grid,
        egui::Theme::Light,
        true,
        1.0,
        nodalia_launcher_ui_egui::TitleBar::Native,
        Some(Platform { os: Os::Windows, arch: Arch::X64 }),
    );
    h.set_size(egui::vec2(1000.0, 700.0));
    h.run_steps(6);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(6);
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Install new apps as").scroll_to_me();
    h.run_steps(3);
    let label = h.get_by_role_and_label(egui::accesskit::Role::Label, "Install new apps as").rect();
    // Several combo boxes are on the settings page; the format chooser is the one with this label.
    let combo = h
        .query_all_by_role(egui::accesskit::Role::ComboBox)
        .find(|node| node.accesskit_node().label().as_deref() == Some("Install new apps as"))
        .expect("the install-format chooser is on screen")
        .rect();
    assert!(!label.intersects(combo), "the label {label:?} overlaps its combo box {combo:?}");
    assert!(
        (label.center().y - combo.center().y).abs() < 2.0 && combo.left() >= label.right() + 4.0 || combo.top() >= label.bottom() + 4.0,
        "the label {label:?} and its combo box {combo:?} must keep a clear gap, on one row or stacked"
    );
}

/// Drives the real launcher lifecycle (`logic` then `ui`) in one `Context::run_ui` pass, optionally
/// asking egui for a discarded second pass over the same input.
fn drive_launcher(launcher: &mut LauncherUi, ctx: &egui::Context, events: Vec<egui::Event>, time: f64, two_passes: bool) {
    let mut first = two_passes;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1180.0, 1200.0))),
        time: Some(time),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        if first {
            first = false;
            root.ctx().request_discard("test: force a second pass");
        }
        launcher.logic(&root.ctx().clone(), &mut NoHost);
        launcher.ui(root, &mut NoHost);
    });
    // Headless: no renderer consumes texture deltas (pin icons, fonts). Clear before drop.
    out.textures_delta.clear();
}

/// Drives one pointer gesture as move, press and release over separate frames, optionally
/// asking for a discarded second pass on release.
fn click_steps(launcher: &mut LauncherUi, ctx: &egui::Context, at: egui::Pos2, time: f64, two_passes_on_release: bool) {
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    drive_launcher(launcher, ctx, vec![egui::Event::PointerMoved(at)], time, false);
    drive_launcher(launcher, ctx, vec![button(true)], time + 0.1, false);
    drive_launcher(launcher, ctx, vec![button(false)], time + 0.2, two_passes_on_release);
}

/// Counts the `Install` commands among the recorded backend commands.
fn install_count(sent: &RefCell<Vec<Command>>) -> usize {
    sent.borrow().iter().filter(|c| matches!(c, Command::Install { .. })).count()
}

/// The AccessKit target of a visible widget, as an assistive technology would address it.
fn accesskit_target(h: &Harness<'_, Option<LauncherUi>>, label: &str) -> (egui::accesskit::NodeId, egui::accesskit::TreeId) {
    h.get_by_label(label).accesskit_node().locate()
}

/// The semantic click a screen reader emits for `target`, addressed exactly as the tree did.
fn accesskit_click(target: (egui::accesskit::NodeId, egui::accesskit::TreeId)) -> egui::Event {
    egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        target_node: target.0,
        target_tree: target.1,
        action: egui::accesskit::Action::Click,
        data: None,
    })
}

/// Runs one visible frame with a semantic (AccessKit) click delivered on every egui pass, or over
/// one pass only when `two_passes` is false. This is the actual UI path: delivering the same click
/// on a discarded pass lets a button's own `clicked()` handler queue its action twice, so a call
/// site that bypasses `act_command`/`act` is caught instead of being hidden by egui's
/// first-pass-only pointer events.
///
/// An AccessKit request belongs to the `RawInput` of a pass, exactly as kittest queues it;
/// inserting it with `Context::input_mut` after `begin_pass` is too late in 0.37. `run_ui` also
/// spends the raw events on its first pass and re-runs the second pass without them, so the passes
/// are driven explicitly. They share one `cumulative_frame_nr` (the action boundary's visible-frame
/// identity): `run_ui` advances that counter once per visible frame, while `begin_pass`/`end_pass`
/// do not. The empty priming frame starts a fresh frame without drawing anything.
fn drive_accesskit_click(
    launcher: &mut LauncherUi,
    ctx: &egui::Context,
    target: (egui::accesskit::NodeId, egui::accesskit::TreeId),
    time: f64,
    two_passes: bool,
) {
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1180.0, 1200.0));
    let mut priming = ctx.run_ui(egui::RawInput { screen_rect: Some(screen_rect), time: Some(time), ..Default::default() }, |_| {});
    priming.textures_delta.clear();
    let passes = if two_passes { 2 } else { 1 };
    for pass in 0..passes {
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(screen_rect),
            time: Some(time),
            events: vec![accesskit_click(target)],
            ..Default::default()
        });
        ctx.root_ui(|root| {
            if two_passes && pass == 0 {
                root.ctx().request_discard("test: repeat the semantic click in this frame");
            }
            // Match Harness::build_ui_state's scope and frame: auto widget IDs include both.
            // Drawing directly in root_ui addresses different nodes than the captured tree.
            root.scope_builder(egui::UiBuilder::new(), |ui| {
                egui::Frame::central_panel(ui.style()).outer_margin(8.0).inner_margin(0.0).show(ui, |ui| {
                    launcher.logic(&ui.ctx().clone(), &mut NoHost);
                    launcher.ui(ui, &mut NoHost);
                });
            });
        });
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
    }
}

/// The end-to-end shape of one pointer gesture per frame. A discarded second pass has no pointer
/// events (egui clears them after the first pass), so this alone cannot catch a call site that
/// bypasses the action boundary. The AccessKit-click tests below re-deliver the same semantic click
/// on every pass and therefore exercise the actual UI call sites twice in one visible frame.
#[test]
fn the_same_download_is_sent_once_across_discarded_passes_but_again_next_frame() {
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    let at = h.get_by_label("Update to 0.5.0").rect().center();
    let ctx = h.ctx.clone();
    let mut launcher = h.state_mut().take().expect("the harness built the launcher");

    sent.borrow_mut().clear();
    click_steps(&mut launcher, &ctx, at, 100.0, true);
    assert_eq!(install_count(&sent), 1, "one visible frame must send one install, not one per pass: {:?}", sent.borrow());

    // A new frame is a new user gesture: the guard from the previous frame must not swallow it.
    sent.borrow_mut().clear();
    click_steps(&mut launcher, &ctx, at, 200.0, false);
    assert_eq!(install_count(&sent), 1, "the next frame must accept the same action again: {:?}", sent.borrow());
}

#[test]
/// Clicking Open across a discarded second pass increments the launch counter once for the
/// visible frame.
fn opening_an_app_counts_once_across_discarded_passes() {
    let (mut h, _sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    let at = h.get_all_by_label("Open").next().expect("an up-to-date app has an Open button").rect().center();
    let ctx = h.ctx.clone();
    let mut launcher = h.state_mut().take().expect("the harness built the launcher");
    let before = launcher.settings.launches.values().copied().sum::<u32>();
    click_steps(&mut launcher, &ctx, at, 100.0, true);
    let after = launcher.settings.launches.values().copied().sum::<u32>();
    assert_eq!(after, before + 1, "one launch gesture increments the count once");
}

#[test]
/// The real Update button, clicked through AccessKit on every pass, crosses the action boundary
/// once per frame and again on the next frame.
fn the_actual_update_button_queues_once_across_a_discarded_pass() {
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    let target = accesskit_target(&h, "Update to 0.5.0");
    let ctx = h.ctx.clone();
    let mut launcher = h.state_mut().take().expect("the harness built the launcher");
    sent.borrow_mut().clear();
    drive_accesskit_click(&mut launcher, &ctx, target, 100.0, true);
    assert_eq!(install_count(&sent), 1, "the real Update button must cross the action boundary once: {:?}", sent.borrow());
    // The next visible frame is a new gesture, not a repeated pass.
    sent.borrow_mut().clear();
    drive_accesskit_click(&mut launcher, &ctx, target, 200.0, false);
    assert_eq!(install_count(&sent), 1, "the next frame must accept the same action again: {:?}", sent.borrow());
}

#[test]
/// The real Open button, clicked through AccessKit on every pass, delivers one `Launch` and
/// increments the count once.
fn the_actual_open_button_launches_once_across_a_discarded_pass() {
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    let target = h.get_all_by_label("Open").next().expect("an up-to-date app has an Open button").accesskit_node().locate();
    let ctx = h.ctx.clone();
    let mut launcher = h.state_mut().take().expect("the harness built the launcher");
    let before = launcher.settings.launches.values().copied().sum::<u32>();
    sent.borrow_mut().clear();
    drive_accesskit_click(&mut launcher, &ctx, target, 100.0, true);
    let launches = sent.borrow().iter().filter(|c| matches!(c, Command::Launch { .. })).count();
    assert_eq!(launches, 1, "the real Open button must deliver one launch, not one per pass: {:?}", sent.borrow());
    assert_eq!(launcher.settings.launches.values().copied().sum::<u32>(), before + 1, "the launch count increments once");
}

/// What the Open half beside Lienzo's update reads out (0.4.1 installed, 0.5.0 out).
const KEEP_LIENZO: &str = "Open 0.4.1 without updating. 0.5.0 stays available.";

/// The commands `sent` holds for `app`: how many launches and how many installs, counting a
/// batched `InstallAll` (always used on Linux) that includes it.
fn launches_and_installs(sent: &RefCell<Vec<Command>>, app: &str) -> (usize, usize) {
    let sent = sent.borrow();
    let launches = sent.iter().filter(|c| matches!(c, Command::Launch { app: a, .. } if a == app)).count();
    let installs = sent
        .iter()
        .filter(|c| match c {
            Command::Install { app: a } => a == app,
            Command::InstallAll { apps } => apps.iter().any(|a| a == app),
            _ => false,
        })
        .count();
    (launches, installs)
}

#[test]
/// While an update waits, the main button is split: Open keeps the installed copy and only opens
/// it, Update installs. Open, Update and the `⋯` menu sit in that order on one line in the grid
/// and in the table's narrower action column.
fn an_update_can_open_the_installed_copy_without_updating() {
    for view in [ViewMode::Grid, ViewMode::Table] {
        let (mut h, sent) = harness(view, egui::Theme::Dark, true);
        h.run_steps(8);
        let open = h.get_by_label(KEEP_LIENZO).rect();
        let update = h.get_by_label("Update to 0.5.0").rect();
        let menu = h.get_by_label("More actions for Lienzo").rect();
        assert!((open.center().y - update.center().y).abs() < 1.0, "{view:?}: one line: {open:?} {update:?}");
        assert!(open.right() < update.left() && update.right() < menu.left(), "{view:?}: Open, Update, menu: {open:?} {update:?} {menu:?}");

        sent.borrow_mut().clear();
        h.get_by_label(KEEP_LIENZO).click();
        h.run_steps(2);
        assert_eq!(launches_and_installs(&sent, "lienzo"), (1, 0), "{view:?}: Open launches and installs nothing: {:?}", sent.borrow());

        sent.borrow_mut().clear();
        h.get_by_label("Update to 0.5.0").click();
        h.run_steps(2);
        assert_eq!(launches_and_installs(&sent, "lienzo"), (0, 1), "{view:?}: Update installs: {:?}", sent.borrow());
    }
}

#[test]
/// Only an update offers the split: up-to-date apps keep their single Open, and an app not
/// installed has nothing to open. An update whose download is already here (Retry update) is
/// split too.
fn only_a_waiting_update_splits_the_main_button() {
    let mut ev = events();
    ev.push(Event::ReadyToInstall { app: "pulso".into(), ready: true });
    let (mut h, sent) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(8);
    let splits: Vec<String> = h.query_all_by_label_contains("without updating").filter_map(|n| n.accesskit_node().label()).collect();
    assert_eq!(splits.len(), 2, "Lienzo and Pulso wait for updates: {splits:?}");
    assert!(splits.iter().any(|l| l == "Open 0.5.0 without updating. 0.6.0 stays available."), "{splits:?}");
    // Luz and Letra are up to date: each still has one plain Open.
    assert!(h.query_all_by_label("Open").count() >= 2);
    h.get_by_label("Retry install");

    sent.borrow_mut().clear();
    h.get_by_label("Retry update to 0.6.0").click();
    h.run_steps(2);
    assert_eq!(launches_and_installs(&sent, "pulso"), (0, 1), "{:?}", sent.borrow());
}

#[test]
/// A newer release with nothing for this computer is no update: the app opens as it is, isn't
/// counted in Update all, and has no Update button that would fail with "has no download".
fn a_newer_release_without_a_download_here_is_not_offered_as_an_update() {
    let ev = no_lienzo_download_here(events());
    let (mut h, sent) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(8);
    assert!(h.query_by_label("Update to 0.5.0").is_none(), "nothing to install here");
    assert!(h.query_by_label(KEEP_LIENZO).is_none(), "no split without an update");
    h.get_by_label("Update to 0.6.0");
    h.get_by_label("Update 1 app");
    h.get_by_label_contains("0.4.1 · latest 0.5.0");
    // Lienzo's main button is a plain Open, which launches it.
    // Cards share a row, so Lienzo's Open is the one right before its own menu.
    let menu = h.get_by_label("More actions for Lienzo").rect();
    sent.borrow_mut().clear();
    h.get_all_by_label("Open")
        .filter(|n| (n.rect().center().y - menu.center().y).abs() < 1.0 && n.rect().right() <= menu.left())
        .max_by(|a, b| a.rect().right().total_cmp(&b.rect().right()))
        .expect("Lienzo's Open")
        .click();
    h.run_steps(2);
    assert_eq!(launches_and_installs(&sent, "lienzo"), (1, 0), "{:?}", sent.borrow());
}

#[test]
/// The backend's answer outranks the UI's guess: a release with a Mac download that can't replace
/// how Lienzo was installed (say an AppImage when only a .deb is out) is no update either.
fn the_backends_installability_answer_decides_an_update() {
    let mut ev = events();
    ev.push(Event::Installable { app: "lienzo".into(), installable: false });
    let (mut h, _sent) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(8);
    assert!(h.query_by_label("Update to 0.5.0").is_none(), "the backend can't install it");
    assert!(h.query_by_label(KEEP_LIENZO).is_none());
    h.get_by_label("Update 1 app");
    // Still a sign the release is out: a quiet note beside the installed version, saying why.
    h.get_by_label_contains("0.4.1 · latest 0.5.0").hover();
    h.run_steps(4);
    h.get_by_label_contains(OUT_OF_REACH_TIP);
}

/// The tooltip on the note beside a copy behind a release it can't take.
const OUT_OF_REACH_TIP: &str = "0.5.0 is out, but Nodalia Launcher can't install it over this copy.";

#[test]
/// The note beside a copy behind a release it can't take shows in the table too.
fn the_table_notes_a_newer_release_that_cant_install_here() {
    let mut ev = events();
    ev.push(Event::Installable { app: "lienzo".into(), installable: false });
    let (mut h, _) = harness_with(ev, ViewMode::Table, egui::Theme::Dark, true, 1.0);
    h.run_steps(8);
    h.get_by_label("0.4.1 · latest 0.5.0").hover();
    h.run_steps(4);
    h.get_by_label_contains(OUT_OF_REACH_TIP);
}

#[test]
/// A copy its repository updates (a Flatpak from FlatPark) knows nothing of GitHub's latest, so a
/// newer release there gets no "can't install" note: its repository may well have it.
fn a_repository_copy_gets_no_out_of_reach_note() {
    let mut ev = events();
    ev.push(Event::Detected {
        app: "luz".into(),
        installed: Some(Installed {
            version: version::parse("0.3.0"),
            source: InstallSource::Flatpak,
            location: "dev.danielmiguelt.Luz".into(),
            package: None,
            externally_managed: true,
            origin: Some("flatpark".into()),
            installation: Some("user".into()),
        }),
    });
    ev.push(Event::Installable { app: "luz".into(), installable: false });
    let (mut h, _) = harness_with(ev, ViewMode::Table, egui::Theme::Dark, true, 1.0);
    h.run_steps(8);
    assert!(h.query_all_by_label("0.3.0").next().is_some(), "Luz's version shows");
    assert!(h.query_by_label_contains("0.3.0 · latest").is_none(), "no note for a repository copy");
}

#[test]
/// Kept on an exact version, the release on screen can be the old track's until the check
/// finishes, so an install of it is refused for that alone: no "can't install it" note then. And
/// "Install version X" for a version that can't replace this copy is greyed out with the reason.
fn a_kept_version_gets_no_out_of_reach_note_and_says_why_it_cant_install() {
    let mut ev = events();
    ev.push(Event::Installable { app: "lienzo".into(), installable: false });
    let (mut h, sent) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(8);
    h.get_by_label_contains("0.4.1 · latest 0.5.0");
    h.state_mut()
        .as_mut()
        .expect("the harness built the launcher")
        .settings
        .versions
        .insert("lienzo".into(), nodalia_launcher_core::protocol::VersionTrack::Pinned("v0.5.0".into()));
    h.run_steps(4);
    assert!(h.query_by_label_contains("latest 0.5.0").is_none(), "no note while a version is kept");
    let install = h.get_by_label("Install version v0.5.0");
    assert!(install.accesskit_node().is_disabled(), "it can't replace this copy");
    install.hover();
    h.run_steps(4);
    h.get_by_label_contains("can't install version v0.5.0 over this copy");
    sent.borrow_mut().clear();
    h.get_by_label("Install version v0.5.0").click();
    h.run_steps(2);
    assert_eq!(launches_and_installs(&sent, "lienzo"), (0, 0), "a greyed-out Install sends nothing: {:?}", sent.borrow());
}

#[test]
/// A Flatpak its repository updates keeps Reinstall, the launcher's way to run `flatpak update`
/// on it, where the backend says that update would run.
fn a_repository_flatpak_keeps_reinstall() {
    let mut ev = events();
    ev.push(Event::Detected {
        app: "luz".into(),
        installed: Some(Installed {
            version: version::parse("0.4.0"),
            source: InstallSource::Flatpak,
            location: "dev.danielmiguelt.Luz".into(),
            package: None,
            externally_managed: true,
            origin: Some("flatpark".into()),
            installation: Some("user".into()),
        }),
    });
    ev.push(Event::Installable { app: "luz".into(), installable: true });
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(8);
    h.get_by_label("More actions for Luz").click();
    h.run_steps(3);
    h.get_by_label("Reinstall");
}

#[test]
/// Reinstall only shows where an install would go ahead: an up-to-date copy the backend can't
/// replace (installed outside the launcher, say) has none, so it can't end in "has no download".
fn reinstall_needs_an_install_that_would_go_ahead() {
    for installable in [true, false] {
        let mut ev = events();
        ev.push(Event::Installable { app: "luz".into(), installable });
        let (mut h, sent) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
        h.run_steps(8);
        h.get_by_label("More actions for Luz").click();
        h.run_steps(3);
        assert_eq!(h.query_by_label("Reinstall").is_some(), installable, "installable: {installable}");
        if installable {
            sent.borrow_mut().clear();
            h.get_by_label("Reinstall").click();
            h.run_steps(2);
            assert_eq!(launches_and_installs(&sent, "luz"), (0, 1), "Reinstall installs: {:?}", sent.borrow());
        }
    }
}

/// `events`, with Lienzo's 0.5.0 published for Windows only: nothing the test's Mac can install.
fn no_lienzo_download_here(mut ev: Vec<Event>) -> Vec<Event> {
    for e in &mut ev {
        if let Event::Latest { app, latest: LatestRelease::Found(r), .. } = e
            && app == "lienzo"
        {
            for a in &mut r.assets {
                a.name = "lienzo-0.5.0-windows-x64.msi".into();
                a.url = "https://github.com/getnodalia/lienzo/releases/download/v0.5.0/lienzo-0.5.0-windows-x64.msi".into();
            }
        }
    }
    ev
}

#[test]
/// The details drawer's actions split the same way, and its Open half only opens the app.
fn the_details_drawer_splits_a_waiting_update_too() {
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    let card = h.get_by_label(KEEP_LIENZO).rect();
    h.get_by_label("Lienzo details").click();
    h.run_steps(10);
    assert_eq!(h.query_all_by_label(KEEP_LIENZO).count(), 2, "the card's and the drawer's");
    sent.borrow_mut().clear();
    h.get_all_by_label(KEEP_LIENZO).find(|n| n.rect() != card).expect("the drawer's Open half").click();
    h.run_steps(2);
    assert_eq!(launches_and_installs(&sent, "lienzo"), (1, 0), "{:?}", sent.borrow());
}

#[test]
/// A toast created by logic while no window ran is shown once on the first drawing pass and
/// expires relative to that pass.
fn a_job_finished_while_hidden_shows_one_fresh_toast_on_the_next_frame() {
    nodalia_launcher_ui_egui::i18n::set_test_lang(nodalia_launcher_ui_egui::i18n::Lang::EN);
    let sent = Rc::new(RefCell::new(Vec::new()));
    let backend = Fake {
        downloads: Default::default(),
        events: vec![Event::JobFinished { app: "lienzo".into(), result: Ok("Lienzo updated to 0.5.0".into()) }],
        sent: sent.clone(),
        manage: true,
        clock: Rc::new(Cell::new(NOW)),
    };
    // Build the harness with no launcher first: its construction pass runs at time 0, before a test
    // can set `RawInput::time`, and would otherwise consume the hidden message.
    let mut attached = false;
    // Ask for a discarded second pass on the first drawing frame: the toast path must be
    // idempotent across passes, not just across steps.
    let mut discard_once = true;
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if let Some(l) = state {
                if !attached {
                    attached = true;
                    l.attach(ui.ctx());
                }
                if discard_once {
                    discard_once = false;
                    ui.ctx().request_discard("test: discard the first drawing pass");
                }
                l.logic(&ui.ctx().clone(), &mut NoHost);
                l.ui(ui, &mut NoHost);
            }
        },
        None,
    );
    // The tray path: `logic` runs without any UI pass, with the input time of the last shown frame.
    let windowless = egui::Context::default();
    let mut launcher = LauncherUi::new(&windowless, Box::new(backend), Settings { notify: false, language: "en".into(), ..Settings::default() });
    launcher.logic(&windowless, &mut NoHost);
    *h.state_mut() = Some(launcher);
    // The visible window's clock is far ahead of the logic-only tick's stale input time. The toast
    // must be fresh on the first drawing pass and must expire relative to it.
    h.input_mut().time = Some(1000.0);
    h.run_steps(2);
    assert_eq!(h.query_all_by_label("Lienzo updated to 0.5.0").count(), 1, "the hidden result is shown once");
    h.run_steps(2);
    assert_eq!(h.query_all_by_label("Lienzo updated to 0.5.0").count(), 1, "a discarded/next pass must not duplicate it");
    // A stale expiry (old logic time + 4.5 s) would already have dropped it; at 1004.6 the real
    // 1000.0 + 4.5 deadline has passed, so the toast goes.
    h.input_mut().time = Some(1004.6);
    h.run_steps(2);
    assert!(h.query_by_label("Lienzo updated to 0.5.0").is_none(), "the toast expires relative to the drawing pass");
}

#[test]
/// At 560 px the toolbar's search, refresh, settings, sort and per-app menu keep their
/// accessible names, with no unnamed input widgets and a strict accessibility pass.
fn a_narrow_toolbar_names_its_controls_and_passes_strict_accessibility() {
    let (mut h, _sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.set_size(egui::vec2(560.0, 900.0));
    h.run_steps(6);
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Search apps");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Check for updates");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Main menu");
    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "More actions for Lienzo");
    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "narrow toolbar has unnamed input widgets: {unnamed:?}");
    h.check_accessibility();
}

#[test]
fn install_locations_survive_settings_save_and_older_settings_use_system_default() {
    let old: Settings = serde_json::from_str(r#"{"auto_update":true}"#).unwrap();
    assert!(old.auto_update);
    assert_eq!(old.install_locations, Default::default());
    let mut settings = old;
    settings.install_locations.root = Some("/Volumes/Nodalia Apps".into());
    settings.install_locations.per_app.insert("lienzo".into(), "/Volumes/Photo".into());
    let restored: Settings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(restored.install_locations.folder("lienzo"), Some("/Volumes/Photo"));
    assert_eq!(restored.install_locations.folder("toma"), Some("/Volumes/Nodalia Apps"));
}

#[test]
/// Logic without a window still consumes backend events; attaching to a new window then draws
/// the update cards.
fn keeps_working_with_no_window_then_draws_in_a_new_one() {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let backend = Fake { events: events(), sent: sent.clone(), manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) };
    let windowless = egui::Context::default();
    let mut ui = LauncherUi::new(&windowless, Box::new(backend), Settings { notify: false, ..Settings::default() });
    // No window: the backend's news is still taken in.
    ui.logic(&windowless, &mut NoHost);
    assert!(sent.borrow().contains(&Command::CheckForUpdates { force: false }));
    assert_eq!(ui.update_count(), 2);
    assert_eq!(ui.settings.last_check, Some(NOW - 180));

    let mut attached = false;
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |egui_ui, state: &mut LauncherUi| {
            if !attached {
                attached = true;
                state.attach(egui_ui.ctx());
            }
            state.logic(&egui_ui.ctx().clone(), &mut NoHost);
            state.ui(egui_ui, &mut NoHost);
        },
        ui,
    );
    h.run_steps(4);
    h.get_by_label("Update to 0.5.0");
    h.get_by_label("Update to 0.6.0");
}

#[test]
fn grid_shows_every_status_and_installs_on_click() {
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    snapshot(&mut h, "grid-dark");
    // Statuses the user asked for: available, installed and up to date, update with version.
    assert!(h.query_all_by_label("Available to install").count() >= 3);
    assert_eq!(h.query_all_by_label("Installed · up to date").count(), 0, "an installed app carries no badge");
    h.get_by_label("Update to 0.5.0");
    h.get_by_label("Update to 0.6.0");
    // The version sits in the card's top strip, and the date under the description.
    h.get_by_label("0.4.1 → 0.5.0");
    assert!(h.query_all_by_label("v0.4.0").count() >= 2);
    // Written like every other date ("Oct 7, 2026"); the day itself depends on the zone the test runs in.
    let updated: Vec<String> = h.query_all_by_label_contains("Last updated: ").filter_map(|n| n.accesskit_node().value()).collect();
    assert!(updated.len() >= 2, "{updated:?}");
    assert!(updated.iter().all(|l| l.starts_with("Last updated: Oct ") && l.ends_with(", 2026")), "{updated:?}");
    // The window has no title bar of its own here (the web build, and the tests).
    assert!(h.query_by_label("Close").is_none());
    assert!(h.query_all_by_label("Support").count() >= 1); // the footer's donation link
    assert!(h.query_all_by_label("Coming soon").count() >= 1);
    h.get_by_label_contains("48 MB of 131 MB");
    h.get_by_label("Update all (2)").click();
    h.run_steps(2);
    assert_update_commands(&sent, &["lienzo", "pulso"]);
}

/// With `batch_updates` off (the default): one `InstallAll` for `apps` on Linux, where updates
/// are always batched, and one `Install` per app elsewhere.
fn assert_update_commands(sent: &RefCell<Vec<Command>>, apps: &[&str]) {
    let mut installs: Vec<Command> =
        sent.borrow().iter().filter(|c| matches!(c, Command::Install { .. } | Command::InstallAll { .. })).cloned().collect();
    for c in &mut installs {
        if let Command::InstallAll { apps } = c {
            apps.sort();
        }
    }
    installs.sort_by_key(|c| format!("{c:?}"));
    let mut apps: Vec<String> = apps.iter().map(|a| a.to_string()).collect();
    apps.sort();
    let expected =
        if cfg!(target_os = "linux") { vec![Command::InstallAll { apps }] } else { apps.into_iter().map(|app| Command::Install { app }).collect() };
    assert_eq!(installs, expected);
}

#[test]
fn downloads_can_be_paused_resumed_and_discarded() {
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    // Tall enough that every not-installed card is on screen, however many apps the catalog has.
    h.set_size(egui::vec2(1180.0, 1700.0));
    h.run_steps(4);
    // Bring the paused and reconnecting apps on screen.
    if let Some(l) = h.state_mut().as_mut() {
        l.settings.filter = nodalia_launcher_ui_egui::settings::Filter::NotInstalled;
    }
    h.run_steps(6);
    snapshot(&mut h, "grid-downloads-dark");
    h.get_by_label("Download paused");
    h.get_by_label("Reconnecting…");
    h.get_by_label_contains("· 25%").click();
    h.run_steps(3);
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "Install").last().unwrap().click();
    h.run_steps(2);
    assert!(sent.borrow().contains(&Command::Install { app: "lamina".into() }));
    h.get_by_label("Discard download").click();
    h.run_steps(2);
    assert!(sent.borrow().contains(&Command::Cancel { app: "lamina".into() }));
    h.get_all_by_label("Pause download").next().unwrap().click();
    h.run_steps(2);
    assert!(sent.borrow().iter().any(|c| matches!(c, Command::Pause { .. })));
}

#[test]
/// A kept download offers Retry install (which only opens install) and menu Download again
/// (Cancel then Install); apps without a kept download do not offer it.
fn a_kept_download_offers_retry_install_and_download_again() {
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(4);
    if let Some(l) = h.state_mut().as_mut() {
        l.settings.filter = nodalia_launcher_ui_egui::settings::Filter::NotInstalled;
    }
    h.run_steps(6);
    // Below the fold in the restyled grid: bring the card on screen first.
    h.get_by_label("Retry install").scroll_to_me();
    h.run_steps(4);
    snapshot(&mut h, "grid-retry-install-dark");
    sent.borrow_mut().clear();
    let retry = h.get_by_label("Retry install");
    let row = retry.rect();
    retry.click();
    h.run_steps(3);
    assert!(sent.borrow().is_empty());
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "Install").last().unwrap().click();
    h.run_steps(2);
    assert_eq!(*sent.borrow(), [Command::Install { app: "forja".into() }]);
    sent.borrow_mut().clear();
    // The … menu beside it.
    let beside = |r: egui::Rect| row.y_range().contains(r.center().y) && (row.right()..row.right() + 60.0).contains(&r.left());
    h.get_all_by_label_contains("More actions").find(|n| beside(n.rect())).unwrap().click();
    h.run_steps(3);
    snapshot(&mut h, "grid-retry-install-menu-dark");
    h.get_by_label("Download again").click();
    h.run_steps(3);
    assert_eq!(*sent.borrow(), [Command::Cancel { app: "forja".into() }]);
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "Install").last().unwrap().click();
    h.run_steps(2);
    assert_eq!(*sent.borrow(), [Command::Cancel { app: "forja".into() }, Command::Install { app: "forja".into() }]);
    // Apps without a kept download don't offer it.
    assert_eq!(h.query_all_by_label("Retry install").count(), 1);
}

#[test]
fn a_dismissed_install_prompt_is_not_called_a_cancelled_download() {
    let finished = |app: &str, why: &str| Event::JobFinished { app: app.into(), result: Err(why.into()) };
    // The password prompt was dismissed with the download kept (forja) and without (papel);
    // a download was cancelled (pluma).
    let mut ev = events();
    ev.push(finished("forja", INSTALL_CANCELLED));
    ev.push(finished("papel", INSTALL_CANCELLED));
    ev.push(finished("pluma", CANCELLED));
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(6);
    snapshot(&mut h, "grid-install-cancelled-dark");
    for toast in ["Install cancelled. The download is kept for the next try.", "Install cancelled", "Download cancelled"] {
        assert_eq!(h.query_all_by_label(toast).count(), 1, "{toast}");
    }
}

#[test]
/// A ready update installs on Retry (the table's narrow button leaves the version to its column,
/// while its accessible name keeps it), while an up-to-date app offers Reinstall instead of
/// Download again.
fn a_kept_update_installs_on_retry_and_an_up_to_date_app_is_not_downloaded_again() {
    let mut ev = events();
    for app in ["pulso", "letra"] {
        ev.push(Event::ReadyToInstall { app: app.into(), ready: true });
    }
    let (mut h, sent) = harness_with(ev, ViewMode::Table, egui::Theme::Dark, true, 1.0);
    h.run_steps(8);
    snapshot(&mut h, "table-retry-update-dark");
    sent.borrow_mut().clear();
    assert!(h.query_by_label("Update to 0.6.0").is_none());
    h.get_by_label("Retry update to 0.6.0").click();
    h.run_steps(2);
    assert_eq!(*sent.borrow(), [Command::Install { app: "pulso".into() }]);
    // Letra is up to date: its menu offers Reinstall, which uses the kept file.
    let row = h.get_by_role_and_label(egui::accesskit::Role::Label, "Letra").rect().center().y;
    h.get_all_by_label_contains("More actions").find(|n| (n.rect().center().y - row).abs() < 25.0).unwrap().click();
    h.run_steps(3);
    h.get_by_label("Reinstall");
    h.check_accessibility();
    assert!(h.query_by_label("Download again").is_none());
}

#[test]
/// A Retry update button too wide for its slot drops the version instead of pushing the menu out
/// of the row (#956): it is no wider than an Install button in the same view. Its accessible name
/// keeps the full label either way.
fn a_retry_update_button_keeps_to_its_slot_and_leaves_room_for_the_menu() {
    for view in [ViewMode::Table, ViewMode::Grid] {
        let mut ev = events();
        // Two-digit minor, as in the report: "Retry update to 0.12.0".
        ev.push(Event::Latest { app: "pulso".into(), latest: LatestRelease::Found(release("pulso", "0.12.0", NOW)) });
        ev.push(Event::ReadyToInstall { app: "pulso".into(), ready: true });
        let (mut h, _) = harness_with(ev, view, egui::Theme::Dark, true, 1.0);
        h.run_steps(8);
        let retry = h.get_by_label("Retry update to 0.12.0").rect();
        let install = h.query_all_by_label("Install").next().unwrap().rect();
        let menu = h.get_by_label("More actions for Pulso").rect();
        assert!(retry.width() <= install.width() + 0.5, "{view:?}: retry {retry:?} wider than install {install:?}");
        assert!(retry.right() <= menu.left(), "{view:?}: retry {retry:?} overlaps menu {menu:?}");
    }
}

#[test]
fn an_install_waiting_for_another_says_so_and_can_be_cancelled() {
    let mut events = events();
    // Two updates reached the system's installer together: one runs, the other waits its turn.
    events.push(Event::Progress { app: "lienzo".into(), phase: JobPhase::Installing });
    events.push(Event::Progress { app: "pulso".into(), phase: JobPhase::Waiting });
    let (mut h, sent) = harness_with(events, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(6);
    snapshot(&mut h, "grid-waiting-dark");
    h.get_by_label("Installing…");
    h.get_by_label("Waiting for another app…");
    // Only the waiting one: the install that is running can't be called off.
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    let cancels: Vec<_> = sent.borrow().iter().filter(|c| matches!(c, Command::Cancel { .. })).cloned().collect();
    assert_eq!(cancels, [Command::Cancel { app: "pulso".into() }]);
}

#[test]
fn the_launcher_update_waiting_for_other_installs_can_be_cancelled() {
    let mut ev = events();
    ev.push(Event::LauncherUpdate(release("nodalia-launcher", "0.2.0", NOW)));
    ev.push(Event::Progress { app: "nodalia-launcher".into(), phase: JobPhase::Waiting });
    let (mut h, sent) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(6);
    snapshot(&mut h, "grid-launcher-waiting-dark");
    h.get_by_label("Waiting for other installs…");
    sent.borrow_mut().clear();
    h.get_by_label("Cancel update").click();
    h.run_steps(2);
    assert_eq!(*sent.borrow(), [Command::Cancel { app: "nodalia-launcher".into() }]);
}

#[test]
/// The launcher banner's Cancel crosses the action boundary once per visible frame.
fn the_launcher_banner_cancel_is_sent_once_across_discarded_passes() {
    let mut ev = events();
    ev.push(Event::LauncherUpdate(release("nodalia-launcher", "0.2.0", NOW)));
    ev.push(Event::Progress { app: "nodalia-launcher".into(), phase: JobPhase::Waiting });
    let (mut h, sent) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(6);
    let at = h.get_by_label("Cancel update").rect().center();
    let ctx = h.ctx.clone();
    let mut launcher = h.state_mut().take().expect("the harness built the launcher");
    sent.borrow_mut().clear();
    click_steps(&mut launcher, &ctx, at, 100.0, true);
    let cancels = sent.borrow().iter().filter(|c| matches!(c, Command::Cancel { .. })).count();
    assert_eq!(cancels, 1, "the banner's Cancel must cross the action boundary once: {:?}", sent.borrow());
}

#[test]
fn each_cancellation_says_what_it_called_off() {
    let finished = |app: &str, why: &str| Event::JobFinished { app: app.into(), result: Err(why.into()) };
    let mut ev = events();
    // An uninstall called off while it waited for the installer.
    ev.push(Event::Progress { app: "luz".into(), phase: JobPhase::Removing });
    ev.push(Event::Progress { app: "luz".into(), phase: JobPhase::Waiting });
    ev.push(finished("luz", CANCELLED));
    // The launcher's own update, cancelled while it waited.
    ev.push(Event::LauncherUpdate(release("nodalia-launcher", "0.2.0", NOW)));
    ev.push(Event::Progress { app: "nodalia-launcher".into(), phase: JobPhase::Waiting });
    ev.push(finished("nodalia-launcher", CANCELLED));
    // A download cancelled mid-way.
    ev.push(finished("toma", CANCELLED));
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(6);
    for toast in ["Uninstall cancelled", "Update cancelled", "Download cancelled"] {
        assert_eq!(h.query_all_by_label(toast).count(), 1, "{toast}");
    }
}

#[test]
fn table_view_and_light_theme() {
    let (mut h, sent) = harness(ViewMode::Table, egui::Theme::Light, true);
    h.run_steps(8);
    snapshot(&mut h, "table-light");
    h.get_by_label("Update to 0.5.0").click();
    h.run_steps(2);
    assert!(sent.borrow().contains(&Command::Install { app: "lienzo".into() }));
    h.get_all_by_label("Open").next().unwrap().click();
    h.run_steps(2);
    assert!(sent.borrow().iter().any(|c| matches!(c, Command::Launch { .. })));
}

#[test]
/// Pin floats an app above Name sort; reorder via settings is reflected; Hide clears the pin.
fn pinning_an_app_keeps_it_on_top_until_unpinned() {
    let (mut h, _) = harness(ViewMode::Table, egui::Theme::Dark, true);
    h.run_steps(6);
    // Name sort would put Forja before Lienzo; a pin must still win.
    h.state_mut().as_mut().unwrap().settings.sort = nodalia_launcher_ui_egui::settings::SortKey::Name;
    h.state_mut().as_mut().unwrap().settings.sort_descending = false;
    h.run_steps(2);
    // Card/row pin button (menu Pin to top is covered by pinned_app_menu_offers_unpin).
    h.get_by_label("Pin Lienzo to top").click();
    h.run_steps(4);
    assert_eq!(h.state().as_ref().unwrap().settings.pinned, vec!["lienzo".to_string()]);
    let ids = h.state().as_ref().unwrap().visible_ids_for_test();
    assert_eq!(ids.first().copied(), Some("lienzo"), "pinned app stays first under Name sort: {ids:?}");
    h.state_mut().as_mut().unwrap().settings.pin_app("toma");
    h.state_mut().as_mut().unwrap().settings.reorder_pinned_to("lienzo", 2);
    h.run_steps(2);
    assert_eq!(h.state().as_ref().unwrap().settings.pinned, vec!["toma".to_string(), "lienzo".to_string()]);
    assert_eq!(&h.state().as_ref().unwrap().visible_ids_for_test()[..2], &["toma", "lienzo"]);
    h.get_all_by_label_contains("Reorder").next().expect("pinned rows expose reorder handles");
}

#[test]
/// Hiding a pinned app also removes it from the pin list.
fn hide_clears_pin() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.state_mut().as_mut().unwrap().settings.pin_app("lienzo");
    h.run_steps(2);
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(3);
    h.get_by_label("Hide App").click();
    h.run_steps(4);
    assert!(h.state().as_ref().unwrap().settings.pinned.is_empty());
    assert!(h.state().as_ref().unwrap().settings.hidden.contains("lienzo"));
}

#[test]
/// After pinning from the menu, opening it again offers Unpin and clears the pin.
fn pinned_app_menu_offers_unpin() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(3);
    h.get_by_label("Pin to top").click();
    h.run_steps(4);
    assert_eq!(h.state().as_ref().unwrap().settings.pinned, vec!["lienzo".to_string()]);
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(3);
    assert!(h.query_by_label("Pin to top").is_none(), "pinned apps must not keep offering Pin to top");
    h.get_by_label("Unpin").click();
    h.run_steps(4);
    assert!(h.state().as_ref().unwrap().settings.pinned.is_empty());
}

#[test]
/// Escape during a pin drag restores the order from drag start (live hover already mutated pins).
fn escape_cancels_pin_drag_and_restores_order() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    {
        let ui = h.state_mut().as_mut().unwrap();
        ui.settings.pin_app("lienzo");
        ui.settings.pin_app("toma");
        ui.settings.pin_app("letra");
        assert_eq!(ui.settings.pinned, ["lienzo", "toma", "letra"]);
        ui.begin_pin_drag_for_test("lienzo");
        // Mid-drag: pointer has already reordered via drop targets.
        ui.settings.pinned = vec!["toma".into(), "lienzo".into(), "letra".into()];
    }
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    let ui = h.state().as_ref().unwrap();
    assert!(!ui.pin_dragging_for_test());
    assert_eq!(ui.settings.pinned, ["lienzo", "toma", "letra"]);
}

#[test]
/// Escape still restores pin order when search hides every app (grid/table never run).
fn escape_cancels_pin_drag_with_empty_results() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    {
        let ui = h.state_mut().as_mut().unwrap();
        ui.settings.pin_app("lienzo");
        ui.settings.pin_app("toma");
        ui.begin_pin_drag_for_test("lienzo");
        ui.settings.pinned = vec!["toma".into(), "lienzo".into()];
        // Apply search without an intervening empty frame (pointer-up would end the drag).
        ui.set_search_for_test("zzzz-no-match");
        assert!(ui.visible_ids_for_test().is_empty());
        assert!(ui.pin_dragging_for_test());
    }
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    let ui = h.state().as_ref().unwrap();
    assert!(!ui.pin_dragging_for_test());
    assert_eq!(ui.settings.pinned, ["lienzo", "toma"]);
}

#[test]
/// Grid shows a Pinned section label once at least one app is pinned.
fn grid_shows_pinned_section_header() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    assert!(h.query_by_label("Pinned").is_none());
    h.state_mut().as_mut().unwrap().settings.pin_app("lienzo");
    h.run_steps(4);
    h.get_by_label("Pinned");
    h.get_all_by_label_contains("Reorder Lienzo").next().expect("pinned card exposes a reorder handle");
}

#[test]
/// A pinned table row's grip leads its actions without taking room from them: its Open button is
/// as wide as an unpinned row's, and the menus stay in one line (#956: a squeezed button pushes
/// the menu out of the column).
fn a_pinned_table_row_keeps_its_buttons_in_line() {
    let (mut h, _) = harness(ViewMode::Table, egui::Theme::Dark, true);
    h.run_steps(6);
    // Luz and Letra are both installed and up to date: each row offers Open.
    h.state_mut().as_mut().unwrap().settings.pin_app("luz");
    h.run_steps(4);
    let grip = h.get_all_by_label_contains("Reorder Luz").next().expect("pinned row has a grip").rect();
    let open_in_row = |menu: egui::Rect| {
        let row = menu.y_range();
        h.query_all_by_label("Open").map(|n| n.rect()).find(|r| row.contains(r.center().y)).expect("an Open button in the row")
    };
    let (pinned_menu, other_menu) = (h.get_by_label("More actions for Luz").rect(), h.get_by_label("More actions for Letra").rect());
    let (pinned_open, other_open) = (open_in_row(pinned_menu), open_in_row(other_menu));
    assert!(grip.right() <= pinned_open.left(), "grip {grip:?} leads the row's Open {pinned_open:?}");
    assert!((pinned_open.width() - other_open.width()).abs() < 0.5, "pinned {pinned_open:?} vs unpinned {other_open:?}");
    assert!((pinned_open.left() - other_open.left()).abs() < 0.5, "pinned {pinned_open:?} vs unpinned {other_open:?}");
    assert!((pinned_menu.right() - other_menu.right()).abs() < 0.5, "pinned {pinned_menu:?} vs unpinned {other_menu:?}");
}

#[test]
fn pin_drag_ignores_targets_clipped_by_the_scroll_viewport() {
    for view in [ViewMode::Grid, ViewMode::Table] {
        let (mut h, _) = harness(view, egui::Theme::Dark, true);
        h.run_steps(6);
        let pins = vec!["lienzo".to_string(), "luz".to_string(), "letra".to_string()];
        h.state_mut().as_mut().unwrap().settings.pinned = pins.clone();
        h.run_steps(4);
        let grip = h.get_by_label("Reorder Lienzo").rect().center();
        let target = h.get_by_label("Letra details").rect();
        let at = match view {
            ViewMode::Grid => egui::pos2(target.right() - 10.0, target.bottom() - 20.0),
            ViewMode::Table => egui::pos2(h.get_by_label("Reorder Letra").rect().center().x, target.bottom() - 10.0),
        };
        h.hover_at(grip);
        h.run_steps(2);
        h.drag_at(grip);
        h.run_steps(2);
        h.hover_at(grip + egui::vec2(0.0, -8.0));
        h.run_steps(2);
        assert!(h.state().as_ref().unwrap().pin_dragging_for_test(), "{view:?}: real grip starts drag");
        // Keep the target's layout position, but clip it behind the window footer.
        h.set_size(egui::vec2(1180.0, at.y + 2.0));
        h.hover_at(at);
        h.run_steps(2);
        assert_eq!(h.state().as_ref().unwrap().settings.pinned, pins, "{view:?}: clipped hover");
        h.drop_at(at);
        h.run_steps(2);
        assert_eq!(h.state().as_ref().unwrap().settings.pinned, pins, "{view:?}: clipped release");
        assert!(!h.state().as_ref().unwrap().pin_dragging_for_test());
    }
}

#[test]
fn pin_drag_accepts_the_table_grip_column() {
    let (mut h, _) = harness(ViewMode::Table, egui::Theme::Dark, true);
    h.run_steps(6);
    h.state_mut().as_mut().unwrap().settings.pinned = vec!["lienzo".into(), "luz".into(), "letra".into()];
    h.run_steps(4);
    let from = h.get_by_label("Reorder Lienzo").rect().center();
    let target = h.get_by_label("Reorder Letra").rect();
    h.hover_at(from);
    h.run_steps(2);
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(from + egui::vec2(0.0, -8.0));
    h.run_steps(2);
    assert!(h.state().as_ref().unwrap().pin_dragging_for_test());
    // Release directly in the last column, outside the first app cell's clip rectangle.
    h.drop_at(egui::pos2(target.center().x, target.bottom() - 2.0));
    h.run_steps(2);
    assert_eq!(h.state().as_ref().unwrap().settings.pinned, ["luz", "letra", "lienzo"]);
    assert!(!h.state().as_ref().unwrap().pin_dragging_for_test());
}

#[test]
/// The card pin button pins without opening the … menu, then offers Unpin {name}.
fn pin_toggle_button_pins_and_unpins() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.get_by_label("Pin Lienzo to top").click();
    h.run_steps(4);
    assert_eq!(h.state().as_ref().unwrap().settings.pinned, vec!["lienzo".to_string()]);
    h.get_by_label("Pinned");
    assert!(h.query_by_label("Pin Lienzo to top").is_none());
    h.get_by_label("Unpin Lienzo").click();
    h.run_steps(4);
    assert!(h.state().as_ref().unwrap().settings.pinned.is_empty());
    h.get_by_label("Pin Lienzo to top");
}

#[test]
/// Hidden apps do not show a pin toggle (Pin to top stays out of their menu too).
fn hidden_app_has_no_pin_toggle_button() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.state_mut().as_mut().unwrap().settings.hidden.insert("lienzo".into());
    h.state_mut().as_mut().unwrap().settings.filter = nodalia_launcher_ui_egui::settings::Filter::Hidden;
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Lienzo");
    assert!(h.query_by_label("Pin Lienzo to top").is_none());
    assert!(h.query_by_label("Unpin Lienzo").is_none());
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(3);
    assert!(h.query_by_label("Pin to top").is_none());
}

#[test]
/// Hide removes an app from All into Hidden Apps; Unhide restores it, clears the hidden set and
/// returns the filter to All.
fn hiding_an_app_removes_it_until_it_is_unhidden() {
    let (mut h, _) = harness(ViewMode::Table, egui::Theme::Dark, true);
    h.run_steps(6);
    let row = h.get_by_role_and_label(egui::accesskit::Role::Label, "Lienzo").rect().center().y;
    h.get_all_by_label_contains("More actions").find(|n| (n.rect().center().y - row).abs() < 25.0).unwrap().click();
    h.run_steps(3);
    h.get_by_label("Hide App").click();
    h.run_steps(4);
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Label, "Lienzo").is_none());
    assert!(h.state().as_ref().unwrap().settings.hidden.contains("lienzo"));
    h.get_by_label("Hidden Apps 1").click();
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Lienzo");
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Label, "Letra").is_none());
    let row = h.get_by_role_and_label(egui::accesskit::Role::Label, "Lienzo").rect().center().y;
    h.get_all_by_label_contains("More actions").find(|n| (n.rect().center().y - row).abs() < 25.0).unwrap().click();
    h.run_steps(3);
    h.get_by_label("Unhide App").click();
    h.run_steps(4);
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Lienzo");
    assert!(h.query_by_label("Hidden Apps 1").is_none());
    assert!(h.state().as_ref().unwrap().settings.hidden.is_empty());
    assert_eq!(h.state().as_ref().unwrap().settings.filter, nodalia_launcher_ui_egui::settings::Filter::All);
}

/// Clicks the "Open" item of the menu that hangs from the `anchor` button: the nearest "Open" below it
/// (the rows' own Open buttons sit on their rows).
fn click_menu_open(h: &mut Harness<'static, Option<LauncherUi>>, anchor: &str) {
    let below = h.get_by_label(anchor).rect().bottom();
    let open = h
        .get_all_by_label("Open")
        .filter(|n| n.rect().top() > below)
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .expect("the menu has an Open item");
    open.click();
}

#[test]
fn launch_options_are_saved_per_app_and_passed_when_it_opens() {
    use egui::accesskit::Role;
    let (mut h, sent) = harness(ViewMode::Table, egui::Theme::Dark, true);
    h.run_steps(6);
    let menu = |h: &mut Harness<'static, Option<LauncherUi>>| {
        h.get_by_label("More actions for Letra").click();
        h.run_steps(3);
    };
    menu(&mut h);
    h.get_by_label("Launch options…").click();
    h.run_steps(3);
    let field = "Flags added each time the app is opened";
    h.get_by_role_and_label(Role::TextInput, field).click();
    h.get_by_role_and_label(Role::TextInput, field).type_text(r#"--dir "My Docs" --open"#);
    h.run_steps(3);
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert_eq!(h.state().as_ref().unwrap().settings.launch_flags.get("letra").map(String::as_str), Some(r#"--dir "My Docs" --open"#));

    menu(&mut h);
    click_menu_open(&mut h, "More actions for Letra");
    h.run_steps(3);
    let args: Vec<String> = ["--dir", "My Docs", "--open"].map(String::from).into();
    assert!(sent.borrow().contains(&Command::Launch { app: "letra".into(), args }), "{:?}", sent.borrow());
    // The other apps open with nothing added.
    assert!(!h.state().as_ref().unwrap().settings.launch_flags.contains_key("luz"));
}

#[test]
fn launch_options_with_an_unclosed_quote_are_not_saved_or_launched() {
    use egui::accesskit::Role;
    let (mut h, sent) = harness(ViewMode::Table, egui::Theme::Dark, true);
    h.run_steps(6);
    h.get_by_label("More actions for Letra").click();
    h.run_steps(3);
    h.get_by_label("Launch options…").click();
    h.run_steps(3);
    let field = "Flags added each time the app is opened";
    h.get_by_role_and_label(Role::TextInput, field).click();
    h.get_by_role_and_label(Role::TextInput, field).type_text(r#"--dir "My Docs"#);
    h.run_steps(3);
    h.get_by_label("A quote isn't closed.");
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert!(h.state().as_ref().unwrap().settings.launch_flags.is_empty());
    h.get_by_label("Cancel").click();
    h.run_steps(3);

    // Flags edited by hand in the settings file: the app isn't opened with half of them.
    h.state_mut().as_mut().unwrap().settings.launch_flags.insert("letra".into(), "'oops".into());
    sent.borrow_mut().clear();
    h.get_by_label("More actions for Letra").click();
    h.run_steps(3);
    click_menu_open(&mut h, "More actions for Letra");
    h.run_steps(3);
    assert!(!sent.borrow().iter().any(|c| matches!(c, Command::Launch { .. })));
}

#[test]
/// The view switch stores Table, and the table names each app once in its wordmark rather than
/// again on the decorative icon.
fn switching_views() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.get_by_label("Show apps as a table").click();
    h.run_steps(4);
    assert_eq!(h.state().as_ref().unwrap().settings.view, ViewMode::Table);
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Lienzo");
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Letra");
    // The row icon is decorative; the wordmark carries the name exactly once.
    assert_eq!(h.query_all_by_role_and_label(egui::accesskit::Role::Image, "Lienzo").count(), 0, "the table icon must not repeat the wordmark");
    assert_eq!(h.query_all_by_role_and_label(egui::accesskit::Role::Label, "Lienzo").count(), 1, "the table wordmark names Lienzo exactly once");
}

/// How far the middle of these lines of text is below the middle of the app icon beside them, in points.
fn below_icon_centre(h: &Harness<'_, Option<LauncherUi>>, lines: &[&str]) -> f32 {
    let text = lines.iter().map(|l| h.get_by_role_and_label(egui::accesskit::Role::Label, l).rect()).reduce(|a, b| a.union(b)).unwrap();
    let icon = h
        .query_all(egui_kittest::kittest::by().role(egui::accesskit::Role::Image))
        .map(|n| n.rect())
        .filter(|r| r.right() <= text.left() && r.top() < text.bottom() && r.bottom() > text.top())
        .max_by(|a, b| a.right().total_cmp(&b.right()))
        .unwrap();
    assert!(text.left() - icon.right() < 24.0, "no icon beside {lines:?}");
    text.center().y - icon.center().y
}

#[test]
fn name_and_tagline_are_centred_on_the_icon() {
    for (scale, suffix) in [(1.0, "-1x"), (1.5, "-1.5x"), (2.0, "-2x")] {
        let (mut h, _) = harness_at(ViewMode::Grid, egui::Theme::Dark, true, scale);
        h.run_steps(8);
        snapshot(&mut h, &format!("grid-dark{suffix}"));
        let off = below_icon_centre(&h, &["Lienzo", "Image editing"]);
        assert!(off.abs() < 0.25, "grid at {scale}x: {off}");
        let (mut h, _) = harness_at(ViewMode::Table, egui::Theme::Light, true, scale);
        h.run_steps(8);
        snapshot(&mut h, &format!("table-light{suffix}"));
        let off = below_icon_centre(&h, &["Lienzo", "Image editing"]);
        assert!(off.abs() < 0.25, "table at {scale}x: {off}");
    }
}

/// The app names in the order the table shows them.
fn order(h: &Harness<'_, Option<LauncherUi>>) -> Vec<String> {
    let names: Vec<&str> = SUITE.iter().map(|a| a.name).collect();
    let mut found: Vec<(f32, String)> = h
        .query_all(egui_kittest::kittest::by())
        .filter_map(|n| {
            let node = n.accesskit_node();
            let label = node.label().or_else(|| node.value())?;
            names.contains(&label.as_str()).then(|| (n.rect().top(), label))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut names: Vec<String> = found.into_iter().map(|(_, l)| l).collect();
    // A label and its text can both carry the name.
    names.dedup();
    names
}

#[test]
fn every_table_column_sorts_both_ways() {
    let (mut h, _) = harness(ViewMode::Table, egui::Theme::Dark, true);
    h.run_steps(6);
    // Status: apps with an update first (suite order breaks the tie), then the up to date ones.
    h.get_by_label("Status").click();
    h.run_steps(3);
    let o = order(&h);
    assert_eq!(&o[..4], ["Lienzo", "Pulso", "Letra", "Luz"], "{o:?}");
    // Reversed: the apps not installed first, then up to date, then the updates last. The table
    // keeps its column's order; only the grid puts installed apps above the rest.
    h.get_by_label("Status").click();
    h.run_steps(3);
    let o = order(&h);
    assert_eq!(&o[o.len().saturating_sub(4)..], ["Letra", "Luz", "Lienzo", "Pulso"], "{o:?}");
    // The old Installed and Latest columns are gone; Version shows the number, and from and to for an update.
    assert!(h.query_by_label("Latest").is_none());
    h.get_by_label("Version");
    assert!(h.query_all_by_label("0.4.0").count() >= 2);
    h.get_by_label("0.4.1 → 0.5.0");
    h.get_by_label("0.5.0 → 0.6.0");
    snapshot(&mut h, "table-sorted-status");
    // App: Z to A on the second click.
    h.get_by_label("App").click();
    h.run_steps(3);
    h.get_by_label("App").click();
    h.run_steps(3);
    let o = order(&h);
    assert_eq!(o.first().map(String::as_str), Some("Vortia"), "{o:?}");
}

/// The first close to the tray is explained once, by a system notification (#45, #215).
#[test]
fn the_first_close_to_the_tray_is_explained_once() {
    struct Notices(Vec<(String, String)>);
    impl Host for Notices {
        fn notify(&mut self, title: &str, body: &str) {
            self.0.push((title.to_string(), body.to_string()));
        }
    }
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    let mut host = Notices(Vec::new());
    let l = h.state_mut().as_mut().unwrap();
    assert!(!l.settings.tray_hint_shown);
    l.hidden_to_tray(&mut host);
    l.hidden_to_tray(&mut host);
    assert!(l.settings.tray_hint_shown);
    assert_eq!(host.0.len(), 1, "{:?}", host.0);
    assert_eq!(host.0[0].0, "Nodalia Launcher is still running");
    assert!(host.0[0].1.contains("tray icon"), "{}", host.0[0].1);
}

#[test]
/// After the taller Settings dialog, the uninstall prompt keeps its own content-sized modal with
/// its buttons close under the text (#88), and Cancel closes it.
fn the_uninstall_prompt_keeps_its_own_size_after_a_taller_dialog() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    // Settings is the tallest dialog: open it and close it again.
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    h.get_by_label("Done").click();
    h.run_steps(3);
    // Lienzo is installed, so its … menu offers Uninstall.
    h.get_by_role_and_label(egui::accesskit::Role::Button, "More actions for Lienzo").click();
    h.run_steps(3);
    h.get_by_label("Uninstall…").click();
    h.run_steps(4);
    h.check_accessibility();
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Uninstall Lienzo?");
    assert_eq!(h.query_all_by_label_contains("Uninstall Lienzo?").count(), 1, "the title icon is decorative, not a second copy of the heading");
    snapshot(&mut h, "uninstall-prompt-dark");
    // The buttons follow the text closely, not somewhere down the height Settings had (#88).
    let text = h.get_by_label("The app will be moved to the Trash. Your documents and settings stay.").rect();
    let button = h.get_by_label("Uninstall").rect();
    let gap = button.top() - text.bottom();
    assert!((0.0..48.0).contains(&gap), "the buttons sit {gap} px below the text");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(h.query_by_label("Uninstall").is_none());
}

#[test]
/// The footer links website and GitHub, cards link only GitHub, and the table menu adds the
/// app's website and repository.
fn links_to_the_website_github_and_each_apps_pages() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    // Footer: website, GitHub, the issue tracker and the donation page.
    for label in ["Website", "GitHub", "Report an issue", "Support"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label}");
    }
    // Cards: every app links to its repo and to its own page on getnodalia.com (plus the footer's).
    let shown_cards = h.query_all_by_label("GitHub").count() - 1;
    let shown_sites = h.query_all_by_label("Website").count() - 1;
    assert!(shown_cards >= 13, "{shown_cards}");
    assert_eq!(shown_sites, 0, "the cards only link to GitHub; the website is the footer's icon");
    // The table's … menu has the app's website and GitHub repo.
    let (mut h, _) = harness(ViewMode::Table, egui::Theme::Dark, true);
    h.run_steps(6);
    h.get_all_by_label_contains("More actions").nth(8).unwrap().click(); // Letra
    h.run_steps(3);
    h.get_by_label("GitHub repository");
    // The footer's Website link and Letra's now-live one in the menu.
    assert!(h.query_all_by_label_contains("Website").count() >= 2);
}

/// Clicks the `n`th widget labelled `label` and returns the URL the app asked to open.
fn click_nth_opens(h: &mut Harness<'_, Option<LauncherUi>>, label: &str, n: usize) -> Option<String> {
    h.get_all_by_label(label).nth(n)?.click();
    for _ in 0..3 {
        h.step();
        let url = h.output().platform_output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::OpenUrl(o) => Some(o.url.clone()),
            _ => None,
        });
        if url.is_some() {
            return url;
        }
    }
    None
}

fn click_opens(h: &mut Harness<'_, Option<LauncherUi>>, label: &str) -> Option<String> {
    click_nth_opens(h, label, 0)
}

/// Every URL the widgets labelled `label` open.
fn all_opened(h: &mut Harness<'_, Option<LauncherUi>>, label: &str) -> Vec<String> {
    let n = h.query_all_by_label(label).count();
    (0..n).filter_map(|i| click_nth_opens(h, label, i)).collect()
}

#[test]
/// Footer and card links open their expected pages; one card menu's All releases action
/// opens a repository releases page.
fn links_open_the_right_pages() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    // The footer's Nodalia links and each card's own pages.
    assert_eq!(click_opens(&mut h, "Report an issue").as_deref(), Some("https://github.com/getnodalia/nodalia-launcher/issues"));
    assert_eq!(click_opens(&mut h, "Support").as_deref(), Some("https://buymeacoffee.com/danielmigueltejedor"));
    let github = all_opened(&mut h, "GitHub");
    assert!(github.contains(&"https://github.com/getnodalia".to_string()), "{github:?}");
    assert!(github.contains(&"https://github.com/getnodalia/lienzo".to_string()), "{github:?}");
    let sites = all_opened(&mut h, "Website");
    assert_eq!(sites, vec!["https://getnodalia.com".to_string()], "{sites:?}");
    // The … menu.
    h.get_all_by_label_contains("More actions").next().unwrap().click();
    h.run_steps(3);
    // The first card is the first installed app by name.
    let url = click_opens(&mut h, "All releases").unwrap_or_default();
    assert!(url.starts_with("https://github.com/getnodalia/") && url.ends_with("/releases"), "{url}");
}

/// An asset URL can come straight from GitHub's release JSON; it must not reach the opener when
/// it carries an opener metacharacter, whether it is shown on a card or in the launcher dialog.
#[test]
fn hostile_release_asset_urls_never_reach_the_opener() {
    const HOSTILE: &str = "https://github.com/getnodalia/lienzo/releases/download/v0.5.0/lienzo-0.5.0-macos-universal.dmg|calc.exe";

    let mut ev = events();
    for event in &mut ev {
        if let Event::Latest { app, latest: LatestRelease::Found(release), .. } = event
            && app.as_str() == "lienzo"
            && let Some(asset) = release.assets.first_mut()
        {
            asset.url = HOSTILE.to_string();
        }
    }
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, false, 1.0);
    h.run_steps(8);
    assert!(h.query_all_by_label("Download for macOS").count() >= 8, "the cards must offer their asset downloads");
    let opened = all_opened(&mut h, "Download for macOS");
    assert!(!opened.iter().any(|url| url.as_str() == HOSTILE), "a hostile card asset URL reached the opener: {opened:?}");

    let mut ev = events();
    let mut launcher_release = release("nodalia-launcher", "0.2.0", NOW);
    launcher_release.assets[0].url = HOSTILE.to_string();
    ev.push(Event::LauncherUpdate(launcher_release));
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, false, 1.0);
    h.run_steps(6);
    assert_eq!(h.query_all_by_label("Download").count(), 1, "the launcher update dialog must be open");
    let opened = all_opened(&mut h, "Download");
    assert!(opened.is_empty(), "a hostile launcher-dialog asset URL reached the opener: {opened:?}");
}

#[test]
/// Background app detection remains on its own five-minute cadence; release polling is separate.
fn looks_again_at_installed_apps_every_few_minutes_not_every_frame() {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let clock = Rc::new(Cell::new(NOW));
    let mut backend = Some(Fake { events: events(), sent: sent.clone(), manage: true, downloads: Default::default(), clock: clock.clone() });
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { notify: false, ..Settings::default() }));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut NoHost);
                l.ui(ui, &mut NoHost);
            }
        },
        None,
    );
    let detects = |sent: &Rc<RefCell<Vec<Command>>>| sent.borrow().iter().filter(|c| **c == Command::Detect).count();
    h.run_steps(10);
    assert_eq!(detects(&sent), 0, "the backend's own startup detection is enough");
    clock.set(NOW + 4 * 60);
    h.run_steps(10);
    assert_eq!(detects(&sent), 0, "not before five minutes");
    clock.set(NOW + 5 * 60);
    h.run_steps(10);
    assert_eq!(detects(&sent), 1, "once at five minutes, not once per frame");
}

#[test]
/// Update and restart queues `UpdateLauncher`, progress hides the button, and
/// `LauncherRestarting` asks the host to quit; the update title is announced once.
fn the_launcher_updates_itself_and_quits_to_restart() {
    struct QuitHost(Rc<Cell<bool>>);
    impl Host for QuitHost {
        fn quit(&mut self) {
            self.0.set(true);
        }
    }
    let sent = Rc::new(RefCell::new(Vec::new()));
    let mut events = events();
    events.push(Event::LauncherUpdate(release("nodalia-launcher", "0.2.0", NOW)));
    let shared_events = Rc::new(RefCell::new(Vec::new()));
    struct Scripted {
        inner: Fake,
        later: Rc<RefCell<Vec<Event>>>,
    }
    impl Backend for Scripted {
        fn send(&mut self, c: Command) {
            self.inner.send(c);
        }
        fn poll(&mut self) -> Vec<Event> {
            let mut out = self.inner.poll();
            out.extend(self.later.borrow_mut().drain(..));
            out
        }
        fn capabilities(&self) -> Capabilities {
            self.inner.capabilities()
        }
        fn platform(&self) -> Option<Platform> {
            self.inner.platform()
        }
        fn now(&self) -> i64 {
            self.inner.now()
        }
    }
    let quit = Rc::new(Cell::new(false));
    let mut host = QuitHost(quit.clone());
    let mut backend = Some(Scripted {
        inner: Fake { events, sent: sent.clone(), manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) },
        later: shared_events.clone(),
    });
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { notify: false, ..Settings::default() }));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut host);
                l.ui(ui, &mut host);
            }
        },
        None,
    );
    h.run_steps(6);
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Nodalia Launcher 0.2.0 is available.");
    assert_eq!(
        h.query_all_by_label_contains("Nodalia Launcher 0.2.0 is available").count(),
        1,
        "the update title is announced once, not once by the icon and once by the heading"
    );
    h.get_by_label("Update and restart").click();
    h.run_steps(2);
    assert!(sent.borrow().contains(&Command::UpdateLauncher));
    // Downloading: the banner shows progress instead of the button.
    shared_events
        .borrow_mut()
        .push(Event::Progress { app: "nodalia-launcher".into(), phase: JobPhase::Downloading { done: 5 << 20, total: 10 << 20 } });
    h.run_steps(3);
    assert!(h.query_by_label("Update and restart").is_none());
    assert!(!quit.get());
    // In place: the launcher quits so the helper can start the new one.
    shared_events.borrow_mut().push(Event::LauncherRestarting);
    h.run_steps(3);
    assert!(quit.get());
}

#[test]
/// A found launcher update opens the relaunch prompt once; Later dismisses it and leaves the
/// toolbar banner.
fn launcher_update_prompts_to_relaunch_with_updated_version() {
    let mut ev = events();
    ev.push(Event::LauncherUpdate(release("nodalia-launcher", "0.2.0", NOW)));
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(6);
    // Dialog pops up on launch with the update prompt
    h.get_by_role_and_label(egui::accesskit::Role::Label, "Nodalia Launcher 0.2.0 is available.");
    assert_eq!(
        h.query_all_by_label_contains("Nodalia Launcher 0.2.0 is available").count(),
        1,
        "the update title is announced once, not once by the icon and once by the heading"
    );
    h.get_by_label("A new version is available. Would you like to relaunch with the updated version?");
    h.get_by_label("What's new");
    // Clicking "Later" dismisses the modal dialog
    h.get_by_label("Later").click();
    h.run_steps(3);
    assert!(h.query_by_label("A new version is available. Would you like to relaunch with the updated version?").is_none());
    // Toolbar update banner is now visible with Update and restart
    h.get_by_label("Update and restart");
}

#[test]
fn the_current_desktop_setting_is_only_offered_where_the_os_has_desktops() {
    // NoHost has no desktops (Windows, Linux, web): no setting.
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    assert!(h.query_by_label("Show the window on the current desktop").is_none());
    // On by default for everyone; macOS shows it in Settings (DesktopHost::has_desktops).
    assert!(Settings::default().show_on_current_desktop);
}

#[test]
fn automatic_checks_run_every_twelve_minutes() {
    use nodalia_launcher_ui_egui::settings::CheckEvery;
    assert_eq!(Settings::default().check_every.seconds(), Some(720));
    // Older saved choices still load, and mean "automatically" now.
    assert_eq!(CheckEvery::Day.seconds(), Some(720));
    assert_eq!(CheckEvery::Never.seconds(), None);
}

#[test]
/// With auto-update on, outdated apps are queued for Install and the launcher for
/// `UpdateLauncher`.
fn automatic_updates_queue_installs_for_outdated_apps_and_launcher() {
    let mut ev = events();
    ev.push(Event::LauncherUpdate(release("nodalia-launcher", "0.2.0", NOW)));
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent2 = sent.clone();
    let mut backend = Some(Fake { events: ev, sent: sent2, manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) });
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { auto_update: true, notify: false, ..Settings::default() }));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut NoHost);
                l.ui(ui, &mut NoHost);
            }
        },
        None,
    );
    h.run_steps(4);
    assert_update_commands(&sent, &["lienzo", "pulso"]);
    let commands = sent.borrow().clone();
    assert!(commands.contains(&Command::UpdateLauncher), "launcher update should be queued: {commands:?}");
}

#[test]
/// Automatic updates and update notices skip a newer release with nothing for this computer: its
/// install could only fail. The other update still goes ahead.
fn automatic_updates_skip_a_release_without_a_download_here() {
    let ev = no_lienzo_download_here(events());
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent2 = sent.clone();
    let mut backend = Some(Fake { events: ev, sent: sent2, manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) });
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { auto_update: true, notify: false, ..Settings::default() }));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut NoHost);
                l.ui(ui, &mut NoHost);
            }
        },
        None,
    );
    h.run_steps(4);
    assert_eq!(launches_and_installs(&sent, "lienzo"), (0, 0), "{:?}", sent.borrow());
    assert_eq!(launches_and_installs(&sent, "pulso"), (0, 1), "{:?}", sent.borrow());
    let notified = &h.state().as_ref().expect("the harness built the launcher").settings.notified;
    assert!(!notified.contains_key("lienzo") && notified.contains_key("pulso"), "{notified:?}");
}

#[test]
/// FreeBSD: Lienzo 0.4.1 sits in /usr/local/bin, installed outside the launcher, and 0.5.0
/// publishes a matching FreeBSD tarball. The launcher won't replace a system binary, and its
/// backend says so, so with notifications and automatic updates on there is no Update button or
/// count, no notice (and 0.5.0 isn't recorded as announced), no install queued; Open still opens it.
fn a_freebsd_system_binary_is_not_offered_an_update_it_would_refuse() {
    struct Notices(Rc<RefCell<Vec<String>>>);
    impl Host for Notices {
        fn notify(&mut self, title: &str, _body: &str) {
            self.0.borrow_mut().push(title.to_string());
        }
    }
    let name = "lienzo-0.5.0-freebsd-x86_64.tar.gz";
    let tarball = Release {
        assets: vec![Asset {
            name: name.into(),
            size: 150 << 20,
            url: format!("https://github.com/getnodalia/lienzo/releases/download/v0.5.0/{name}"),
            sha256: None,
        }],
        ..release("lienzo", "0.5.0", NOW - 86_400)
    };
    let system = Installed {
        version: version::parse("0.4.1"),
        source: InstallSource::Binary,
        location: "/usr/local/bin/lienzo".into(),
        package: None,
        externally_managed: false,
        origin: None,
        installation: None,
    };
    let mut ev = vec![Event::Latest { app: "lienzo".into(), latest: LatestRelease::Found(tarball) }];
    for app in &SUITE {
        ev.push(Event::Detected { app: app.id.into(), installed: (app.id == "lienzo").then(|| system.clone()) });
    }
    // What the native backend answers for this copy: an install would refuse it.
    ev.push(Event::Installable { app: "lienzo".into(), installable: false });
    ev.push(Event::CheckFinished { at: NOW - 180, source: Source::GitHubApi, rate_limited_until: None });

    let sent = Rc::new(RefCell::new(Vec::new()));
    let notices = Rc::new(RefCell::new(Vec::new()));
    let fake = Fake { events: ev, sent: sent.clone(), manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) };
    let mut backend = Some(PlatformFake { inner: fake, platform: Platform { os: Os::FreeBsd, arch: Arch::X64 } });
    let host_notices = notices.clone();
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { auto_update: true, notify: true, ..Settings::default() }));
            }
            let mut host = Notices(host_notices.clone());
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut host);
                l.ui(ui, &mut host);
            }
        },
        None,
    );
    h.run_steps(8);

    assert!(h.query_by_label("Update to 0.5.0").is_none(), "no Update button");
    assert!(h.query_by_label_contains("without updating").is_none(), "no split");
    assert!(h.query_by_label_contains("Update all").is_none() && h.query_by_label("Update 1 app").is_none(), "no update count");
    assert!(notices.borrow().is_empty(), "no update notice: {:?}", notices.borrow());
    let launcher = h.state().as_ref().expect("the harness built the launcher");
    assert!(!launcher.settings.notified.contains_key("lienzo"), "0.5.0 isn't recorded as announced");
    assert_eq!(launches_and_installs(&sent, "lienzo"), (0, 0), "no automatic install: {:?}", sent.borrow());

    let menu = h.get_by_label("More actions for Lienzo").rect();
    h.get_all_by_label("Open")
        .filter(|n| (n.rect().center().y - menu.center().y).abs() < 1.0 && n.rect().right() <= menu.left())
        .max_by(|a, b| a.rect().right().total_cmp(&b.rect().right()))
        .expect("Lienzo's Open")
        .click();
    h.run_steps(2);
    assert_eq!(launches_and_installs(&sent, "lienzo"), (1, 0), "Open still opens it: {:?}", sent.borrow());
}

#[test]
/// With auto-update off, no app install or launcher update is queued.
fn automatic_updates_are_disabled_by_default() {
    let mut ev = events();
    ev.push(Event::LauncherUpdate(release("nodalia-launcher", "0.2.0", NOW)));
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent2 = sent.clone();
    let mut backend = Some(Fake { events: ev, sent: sent2, manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) });
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { auto_update: false, notify: false, ..Settings::default() }));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut NoHost);
                l.ui(ui, &mut NoHost);
            }
        },
        None,
    );
    h.run_steps(4);
    let commands = sent.borrow().clone();
    assert!(!commands.contains(&Command::Install { app: "lienzo".into() }), "lienzo should not be auto-installed when auto_update is false");
    assert!(!commands.contains(&Command::UpdateLauncher), "launcher should not be auto-updated when auto_update is false");
}

#[test]
/// The footer says when the apps were last checked, with or without a release found.
fn the_footer_says_when_the_apps_were_last_checked() {
    let footer = |extra: Vec<Event>| {
        let mut evs = events();
        evs.extend(extra);
        let sent = Rc::new(RefCell::new(Vec::new()));
        let mut backend = Some(Fake { events: evs, sent, manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) });
        let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
            move |ui, state: &mut Option<LauncherUi>| {
                if state.is_none()
                    && let Some(b) = backend.take()
                {
                    *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { notify: false, ..Settings::default() }));
                }
                if let Some(l) = state.as_mut() {
                    l.logic(&ui.ctx().clone(), &mut NoHost);
                    l.ui(ui, &mut NoHost);
                }
            },
            None,
        );
        h.run_steps(6);
        let label =
            h.get_by_label_contains("Checked ").accesskit_node().label().or_else(|| h.get_by_label_contains("Checked ").accesskit_node().value());
        label.unwrap_or_default()
    };
    assert_eq!(footer(Vec::new()), "Checked 3 min ago");
    let found = vec![Event::Latest { app: "letra".into(), latest: LatestRelease::Found(release("letra", "0.3.1", NOW)) }];
    assert_eq!(footer(found), "Checked 3 min ago");
}

#[test]
fn web_build_offers_downloads() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, false);
    h.run_steps(8);
    snapshot(&mut h, "web-grid-dark");
    assert!(h.query_all_by_label("Download for macOS").count() >= 8);
    assert!(h.query_by_label("Install").is_none());
    assert!(h.query_by_label_contains("without updating").is_none(), "nothing installed to open on the web");
}

#[test]
/// A custom title bar shows Minimize, Maximize and Close, and Close requests the viewport close
/// command.
fn custom_title_bar_has_window_buttons() {
    struct FramelessHost;
    impl Host for FramelessHost {
        fn title_bar(&self) -> nodalia_launcher_ui_egui::TitleBar {
            nodalia_launcher_ui_egui::TitleBar::Custom
        }
    }
    let mut backend = Some(Fake {
        events: events(),
        sent: Rc::new(RefCell::new(Vec::new())),
        manage: true,
        downloads: Default::default(),
        clock: Rc::new(Cell::new(NOW)),
    });
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { notify: false, ..Settings::default() }));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut FramelessHost);
                l.ui(ui, &mut FramelessHost);
            }
        },
        None,
    );
    h.run_steps(6);
    snapshot(&mut h, "grid-title-bar-dark");
    h.get_by_label("Minimize");
    h.get_by_label("Maximize");
    h.get_by_label("Close").click();
    h.step();
    let closes = h.output().viewport_output.values().any(|v| v.commands.contains(&egui::ViewportCommand::Close));
    assert!(closes, "the close button asks the window to close (the shell decides whether that hides it to the tray)");
}

/// With a dialog open, one click on the custom title bar's Close closes the window: the caption
/// buttons sit above the dialog's backdrop, as a system title bar does, so the click isn't spent
/// dismissing the dialog first (#853). The header's own controls beside them still take clicks.
#[test]
fn close_button_closes_with_a_dialog_open() {
    let (mut h, _) = harness_with_title_bar(events(), ViewMode::Grid, egui::Theme::Dark, true, 1.0, nodalia_launcher_ui_egui::TitleBar::Custom);
    h.run_steps(6);
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(2);
    h.get_by_label("Done");
    h.get_by_label("Close").click();
    h.step();
    let closes = h.output().viewport_output.values().any(|v| v.commands.contains(&egui::ViewportCommand::Close));
    assert!(closes, "the first click on Close closes the window");
    h.run_steps(2);
    h.get_by_label("Done");
}

/// Presses and releases at `pos`, and returns what the window was asked to do.
fn click_at(h: &mut Harness<'_, Option<LauncherUi>>, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
    let mut asked = Vec::new();
    for pressed in [true, false] {
        let input = h.input_mut();
        input.events.push(egui::Event::PointerMoved(pos));
        input.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.step();
        asked.extend(h.output().viewport_output.values().flat_map(|v| v.commands.iter().cloned()));
    }
    h.run_steps(2);
    asked
}

/// Above a dialog the caption buttons keep their place in Tab order (NDL's window controls take
/// focus), and a click just beside them is still the backdrop's (#853).
#[test]
fn the_lifted_caption_buttons_leave_the_backdrop_to_the_dialog() {
    let (mut h, _) = harness_with_title_bar(events(), ViewMode::Grid, egui::Theme::Dark, true, 1.0, nodalia_launcher_ui_egui::TitleBar::Custom);
    h.run_steps(6);
    let tab = |h: &mut Harness<'_, Option<LauncherUi>>| -> String {
        h.key_press(egui::Key::Tab);
        h.run_steps(2);
        let focused = h.root().query_all(egui_kittest::kittest::by()).find(|n| n.accesskit_node().is_focused());
        focused.map_or(String::new(), |n| n.accesskit_node().label().unwrap_or_default().to_string())
    };
    let caption = ["Minimize", "Maximize", "Close"];
    let reached: Vec<String> = (0..6).map(|_| tab(&mut h)).filter(|l| caption.contains(&l.as_str())).collect();
    assert_eq!(reached, caption, "without a dialog Tab reaches them in the order they're drawn");
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(3);
    let close = h.get_by_label("Close").rect();
    let asked = click_at(&mut h, egui::pos2(close.center().x, close.bottom() + 3.0));
    assert!(asked.is_empty(), "below Close: {asked:?}");
    assert!(h.query_by_label("Done").is_none(), "a click below Close dismisses the dialog");
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(3);
    let minimize = h.get_by_label("Minimize").rect();
    let asked = click_at(&mut h, egui::pos2(minimize.left() - 3.0, minimize.center().y));
    assert!(asked.is_empty(), "left of Minimize: {asked:?}");
    assert!(h.query_by_label("Done").is_none(), "a click left of Minimize dismisses the dialog");
}

/// The details drawer covers the header, and the window's Close stays under it.
#[test]
fn the_details_drawer_keeps_the_caption_buttons_under_it() {
    let (mut h, _) = harness_with_title_bar(events(), ViewMode::Grid, egui::Theme::Dark, true, 1.0, nodalia_launcher_ui_egui::TitleBar::Custom);
    h.run_steps(8);
    let close = h.get_by_label("Close").rect().center();
    h.get_by_label("Lienzo details").click();
    h.run_steps(10);
    let asked = click_at(&mut h, close);
    assert!(!asked.contains(&egui::ViewportCommand::Close), "the click reached the window's Close: {asked:?}");
}

/// A quick drag on an edge only resizes: egui sees it as one spot, so nothing nearby may click (#571).
#[test]
fn a_press_on_the_window_edge_only_resizes() {
    use egui::ViewportCommand as Asked;
    struct FramelessHost;
    impl Host for FramelessHost {
        fn title_bar(&self) -> nodalia_launcher_ui_egui::TitleBar {
            nodalia_launcher_ui_egui::TitleBar::Custom
        }
    }
    nodalia_launcher_ui_egui::i18n::set_test_lang(nodalia_launcher_ui_egui::i18n::Lang::EN);
    let mut backend = Some(Fake {
        downloads: Default::default(),
        events: events(),
        sent: Rc::new(RefCell::new(Vec::new())),
        manage: true,
        clock: Rc::new(Cell::new(NOW)),
    });
    let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 1200.0)).build_ui_state(
        move |ui, state: &mut Option<LauncherUi>| {
            if state.is_none()
                && let Some(b) = backend.take()
            {
                *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { notify: false, ..Settings::default() }));
            }
            if let Some(l) = state.as_mut() {
                l.logic(&ui.ctx().clone(), &mut FramelessHost);
                // The harness keeps its app 8 px in; a real header starts at the window's top.
                let window = ui.ctx().content_rect();
                ui.scope_builder(egui::UiBuilder::new().max_rect(window), |ui| l.ui(ui, &mut FramelessHost));
            }
        },
        None,
    );
    h.run_steps(6);
    let press = |h: &mut Harness<'_, Option<LauncherUi>>, pos: egui::Pos2| -> Vec<Asked> {
        let mut asked = Vec::new();
        for pressed in [true, false] {
            let input = h.input_mut();
            input.events.push(egui::Event::PointerMoved(pos));
            input.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            h.step();
            asked.extend(h.output().viewport_output.values().flat_map(|v| v.commands.iter().cloned()));
        }
        h.run_steps(2);
        asked
    };
    let settings_open =
        |h: &Harness<'_, Option<LauncherUi>>| h.query_by_role_and_label(egui::accesskit::Role::ComboBox, "Launcher version").is_some();
    let menu_open = |h: &Harness<'_, Option<LauncherUi>>| h.query_by_label("Keyboard shortcuts").is_some();
    for label in ["Minimize", "Maximize", "Close", "Main menu"] {
        let x = h.get_by_label(label).rect().center().x;
        let asked = press(&mut h, egui::pos2(x, 2.0));
        assert!(asked.iter().any(|c| matches!(c, Asked::BeginResize(_))), "{label}: {asked:?}");
        assert!(!asked.iter().any(|c| matches!(c, Asked::Minimized(_) | Asked::Maximized(_) | Asked::Close)), "{label}: {asked:?}");
    }
    assert!(!menu_open(&h), "the menu button was pressed through the edge");
    // The first frame at a new size already covers the new edge, or Close slips out from under it.
    h.set_size(egui::vec2(1300.0, 1200.0));
    h.step();
    let x = h.get_by_label("Close").rect().center().x;
    let asked = press(&mut h, egui::pos2(x, 2.0));
    assert!(!asked.iter().any(|c| matches!(c, Asked::Close)), "after growing: {asked:?}");
    // A key press still works with the pointer resting on the band, and so does a click below it.
    let x = h.get_by_label("Minimize").rect().center().x;
    press(&mut h, egui::pos2(x, 4.5));
    h.get_by_label("Minimize").focus();
    h.run_steps(2);
    h.key_down(egui::Key::Enter);
    h.step();
    let asked: Vec<_> = h.output().viewport_output.values().flat_map(|v| v.commands.iter().cloned()).collect();
    assert!(asked.contains(&Asked::Minimized(true)), "by key: {asked:?}");
    h.key_up(egui::Key::Enter);
    h.step();
    for (label, want) in [("Minimize", Asked::Minimized(true)), ("Maximize", Asked::Maximized(true)), ("Close", Asked::Close)] {
        let centre = h.get_by_label(label).rect().center();
        let asked = press(&mut h, centre);
        assert!(asked.contains(&want), "{label} on the button: {asked:?}");
        assert!(!asked.iter().any(|c| matches!(c, Asked::BeginResize(_))), "{label} on the button: {asked:?}");
    }
    // A dialog's backdrop reaches the edges too; the press doesn't dismiss it.
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(4);
    assert!(settings_open(&h));
    let asked = press(&mut h, egui::pos2(590.0, 1198.0));
    assert!(asked.iter().any(|c| matches!(c, Asked::BeginResize(_))), "{asked:?}");
    assert!(settings_open(&h), "the dialog closed on a resize");
    // Above the dialog the caption buttons have a layer of their own (#853); the band over them still only resizes.
    for label in ["Minimize", "Maximize", "Close"] {
        let x = h.get_by_label(label).rect().center().x;
        let asked = press(&mut h, egui::pos2(x, 2.0));
        assert!(asked.iter().any(|c| matches!(c, Asked::BeginResize(_))), "{label} over the dialog: {asked:?}");
        assert!(!asked.iter().any(|c| matches!(c, Asked::Minimized(_) | Asked::Maximized(_) | Asked::Close)), "{label} over the dialog: {asked:?}");
    }
    assert!(settings_open(&h), "the dialog closed on a resize");
}

#[test]
/// Details reuses the action row but must not show a reorder grip — there are no sibling pins to
/// drop onto in the drawer (pin toggle still appears).
fn details_drawer_hides_reorder_grip_for_pinned_apps() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.state_mut().as_mut().unwrap().settings.pin_app("lienzo");
    h.run_steps(4);
    h.get_all_by_label_contains("Reorder Lienzo").next().expect("grid card still has a reorder grip");
    h.get_by_label("Lienzo details").click();
    h.run_steps(10);
    // Exactly one Reorder control remains (the grid card behind the drawer), not a second in details.
    assert_eq!(h.query_all_by_label_contains("Reorder Lienzo").count(), 1, "details must not add a reorder grip");
    // Pin toggle still appears in both the card and the drawer.
    assert!(h.query_all_by_label("Unpin Lienzo").count() >= 2);
    // Prefer the details … control (last in the tree) over the grid card's.
    h.get_all_by_label("More actions for Lienzo").last().expect("details more menu").click();
    h.run_steps(3);
    // Details … menu can Unpin, but Move up/down belong only on the list.
    h.get_by_label("Unpin");
    assert!(h.query_by_label("Move up").is_none());
    assert!(h.query_by_label("Move down").is_none());
}

#[test]
/// Clicking a card opens its details drawer with the page links and Esc-to-close.
fn clicking_a_card_opens_its_details_drawer() {
    let (mut h, _sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    // Every app has a page: a card of each generation opens one.
    h.get_by_label("Pluma details");
    h.get_by_label("Letra details");
    h.get_by_label("Lienzo details").click();
    h.run_steps(10);
    h.check_accessibility();
    assert_eq!(
        h.query_all_by_role_and_label(egui::accesskit::Role::Image, "Lienzo").count(),
        0,
        "the hero icon is decorative; the wordmark carries the app name"
    );
    assert_eq!(
        h.query_all_by_role_and_label(egui::accesskit::Role::Label, "Lienzo").count(),
        2,
        "exactly the grid card's wordmark and the details drawer's wordmark carry the app name"
    );
    snapshot(&mut h, "details-dark");
    h.get_all_by_label_contains("The image editor you already know how to use").last().unwrap();
    h.get_by_label_contains("Layers, masks, adjustment layers");
    // The links the page offers as buttons of their own.
    h.get_by_label("Website ↗");
    h.get_by_label("GitHub ↗");
    h.get_by_label("Releases ↗");
    // Esc closes it and the page is gone once it has slid out.
    h.key_press(egui::Key::Escape);
    h.run_steps(10);
    // Only the card's own line is left once the page has slid out.
    assert_eq!(h.query_all_by_label_contains("The image editor you already know").count(), 1);
}

#[test]
fn details_drawer_in_the_light_theme() {
    let (mut h, _sent) = harness(ViewMode::Grid, egui::Theme::Light, true);
    h.run_steps(8);
    h.get_by_label("Lienzo details").click();
    h.run_steps(10);
    snapshot(&mut h, "details-light");
    h.get_all_by_label_contains("The image editor you already know how to use").last().unwrap();
}

#[test]
fn a_page_that_lists_no_pictures_has_no_gallery() {
    let (mut h, _sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    // No Nodalia app lists pictures yet, so no page has a gallery.
    h.get_by_label("Papel details").scroll_to_me();
    h.run_steps(3);
    h.get_by_label("Papel details").click();
    h.run_steps(10);
    snapshot(&mut h, "details-no-pictures-dark");
    h.get_all_by_label_contains("Read, organize and protect your PDFs.").last().unwrap();
    h.get_by_label_contains("View, annotate, fill, merge and sign PDF files");
    assert!(h.query_by_label("Enlarge picture").is_none());
    assert!(h.query_by_label("Next picture (→)").is_none());
}

/// The drawer at sizes far below what the desktop window allows (its minimum is
/// 720 x 500, but the web build has no such floor) and far above, resized while they are open.
#[test]
fn the_drawer_survives_any_window_size() {
    let (mut h, _sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(8);
    h.get_by_label("Lienzo details").click();
    h.run_steps(10);
    let sizes = [(1180.0, 900.0), (720.0, 500.0), (480.0, 320.0), (320.0, 200.0), (120.0, 90.0), (40.0, 30.0), (4000.0, 2400.0)];
    for (w, ht) in sizes {
        h.set_size(egui::vec2(w, ht));
        h.run_steps(3);
    }
    // Back to a normal size, still working.
    h.set_size(egui::vec2(1180.0, 900.0));
    h.run_steps(4);
    h.get_all_by_label_contains("The image editor you already know how to use").last().unwrap();
}

#[test]
/// At 720–1180 px the rate-limit notice stays readable and clear of the footer's checked time
/// and eight right-side links.
fn the_rate_limit_notice_never_runs_under_the_footer_links() {
    for width in [720.0, 960.0, 1180.0] {
        let mut evs = events();
        evs.push(Event::CheckFinished { at: NOW - 180, source: Source::GitHubApi, rate_limited_until: Some(NOW + 1800) });
        let mut backend = Some(Fake {
            events: evs,
            sent: Rc::new(RefCell::new(Vec::new())),
            manage: true,
            downloads: Default::default(),
            clock: Rc::new(Cell::new(NOW)),
        });
        let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(width, 900.0)).build_ui_state(
            move |ui, state: &mut Option<LauncherUi>| {
                if state.is_none()
                    && let Some(b) = backend.take()
                {
                    *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { notify: false, ..Settings::default() }));
                }
                if let Some(l) = state.as_mut() {
                    l.logic(&ui.ctx().clone(), &mut NoHost);
                    l.ui(ui, &mut NoHost);
                }
            },
            None,
        );
        h.run_steps(6);
        snapshot(&mut h, &format!("footer-rate-limited-{width}"));
        // Full or short, the notice says when GitHub can be asked again.
        let notice = h.get_by_label_contains("in 30 min").rect();
        let checked = h.get_by_label_contains("Checked ").rect();
        // Cut short in a narrow window, but enough of it left to read.
        assert!(notice.width() >= 120.0 && notice.right() <= width, "{width}: notice {notice:?}");
        // Everything on the footer's right: the website, GitHub, issues and donation links.
        let mut right = Vec::new();
        for label in ["Website", "GitHub", "Report an issue", "Support"] {
            right.extend(h.query_all_by_label(label).map(|n| n.rect()).filter(|r| (r.center().y - notice.center().y).abs() < 12.0));
        }
        assert_eq!(right.len(), 4, "{width}: {right:?}");
        assert!(h.query_by_label_contains("Installing for").is_none(), "{width}: the footer no longer names the platform");
        for r in right {
            assert!(!r.intersects(notice), "{width}: notice {notice:?} overlaps {r:?}");
            assert!(!r.intersects(checked), "{width}: check time {checked:?} overlaps {r:?}");
        }
    }
}

#[test]
/// At 720–1180 px refresh, search, sort and view share one unclipped toolbar row without
/// overlaps.
fn toolbar_keeps_navigation_refresh_search_and_view_on_one_unclipped_row() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    for width in [720.0, 800.0, 1180.0] {
        h.set_size(egui::vec2(width, 900.0));
        h.run_steps(6);
        let refresh = h.get_by_label("Check for updates").rect();
        let search = h.query_all_by_role(egui::accesskit::Role::TextInput).next().unwrap().rect();
        let view = h.get_by_label("Show apps as cards").rect();
        let sort = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Sort").rect();
        assert!(refresh.top() > 48.0, "{width}: refresh belongs in the toolbar, not the header: {refresh:?}");
        assert!((refresh.center().y - search.center().y).abs() < 2.0, "{width}: refresh and search must share a row");
        assert!(refresh.right() <= search.left() + 1.0, "{width}: refresh overlaps search: {refresh:?}, {search:?}");
        assert!(refresh.right() <= sort.left() + 1.0, "{width}: sort overlaps refresh: {refresh:?}, {sort:?}");
        assert!(sort.right() <= search.left() + 1.0, "{width}: sort overlaps search: {sort:?}, {search:?}");
        assert!(search.right() <= view.left() + 1.0 && view.right() <= width, "{width}: search overlaps view or view is clipped");
        for (label, rect) in [("refresh", refresh), ("search", search), ("sort", sort), ("view", view)] {
            assert!(rect.left() >= 0.0 && rect.right() <= width, "{width}: {label} is outside the window: {rect:?}");
        }
        assert!(!sort.intersects(search) && !view.intersects(search), "{width}: toolbar controls overlap");
        h.check_accessibility();
    }
}

#[test]
fn the_toolbar_has_no_filter_dropdown() {
    use nodalia_launcher_ui_egui::settings::Filter;
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    for label in ["All 13", "Installed 4", "Updates 2", "Not installed 9"] {
        assert!(h.query_by_value(label).is_none() && h.query_by_label(label).is_none(), "{label}");
    }
    // Only the sort dropdown is left.
    assert_eq!(h.query_all_by_role(egui::accesskit::Role::ComboBox).count(), 1);
    assert_eq!(h.state().as_ref().unwrap().settings.filter, Filter::All);
}

#[test]
fn saved_filters_are_migrated_on_ui_construction() {
    use nodalia_launcher_ui_egui::settings::Filter;

    for (saved, expected) in [
        (r#"{"filter":"All"}"#, Filter::All),
        (r#"{"filter":"Installed"}"#, Filter::All),
        (r#"{"filter":"Updates"}"#, Filter::All),
        (r#"{"filter":"NotInstalled"}"#, Filter::All),
        (r#"{"filter":"Hidden"}"#, Filter::Hidden),
    ] {
        let mut settings: Settings = serde_json::from_str(saved).unwrap();
        settings.language = "en".into(); // Avoid the global system-locale probe in parallel tests.
        settings.last_check = Some(NOW);
        let backend = Fake { events: Vec::new(), sent: Rc::default(), manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) };
        let ui = LauncherUi::new(&egui::Context::default(), Box::new(backend), settings);
        assert_eq!(ui.settings.filter, expected, "{saved}");
        assert_eq!(ui.settings.last_check, Some(NOW));
    }
}

/// Checks that, at one window width, every header control stays inside the window and clear of
/// the brand text, that a custom title bar's buttons are there and inside, and snapshots the
/// three widths that matter visually.
fn assert_header_at_width(h: &mut Harness<'_, Option<LauncherUi>>, bar: nodalia_launcher_ui_egui::TitleBar, theme: egui::Theme, width: f32) {
    for label in ["Show apps as cards", "Show apps as a table", "Main menu", "Check for updates"] {
        let rect = h.get_by_label(label).rect();
        assert!(rect.left() >= 0.0 && rect.right() <= width, "{bar:?} {width}: {label} {rect:?}");
        if let Some(text) = h.query_by_label("Nodalia apps") {
            assert!(!rect.intersects(text.rect()), "{bar:?} {width}: Nodalia apps overlaps {label} {rect:?}");
        }
    }
    if bar == nodalia_launcher_ui_egui::TitleBar::Custom {
        for label in ["Minimize", "Maximize", "Close"] {
            let rect = h.get_by_label(label).rect();
            assert!(rect.left() >= 0.0 && rect.right() <= width, "{width}: {label} {rect:?}");
        }
    }
    if width == 1180.0 {
        assert!(h.query_by_label("Launcher").is_none());
        h.get_by_label("Nodalia apps");
    }
    if [720.0, 800.0, 1180.0].contains(&width) {
        snapshot(h, &format!("header-{theme:?}-{bar:?}-{width}"));
    }
}

#[test]
fn narrow_header_keeps_the_view_switch_clear_of_brand_text() {
    use nodalia_launcher_ui_egui::TitleBar;
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for bar in [TitleBar::Native, TitleBar::MacOs, TitleBar::Custom] {
            let (mut h, _) = harness_with_title_bar(events(), ViewMode::Grid, theme, true, 1.0, bar);
            for width in (720..=1180).step_by(10).chain([720, 1180]).map(|w| w as f32) {
                h.set_size(egui::vec2(width, 900.0));
                h.run_steps(6);
                assert_header_at_width(&mut h, bar, theme, width);
            }
            h.get_by_label("Show apps as a table").click();
            h.run_steps(2);
            assert_eq!(h.state().as_ref().unwrap().settings.view, ViewMode::Table);
        }
    }
}

/// A shell on an OS whose animation setting the test switches.
struct OsMotion(Rc<Cell<bool>>);

impl Host for OsMotion {
    fn reduce_motion(&self) -> bool {
        self.0.get()
    }
}

/// The grid in a window too short for it, at 60 frames a second, on an OS asking for fewer
/// animations (`reduce`) or not. Nothing is downloading, so the window is still unless something
/// animates.
fn short_window(reduce: Rc<Cell<bool>>) -> Harness<'static, Option<LauncherUi>> {
    nodalia_launcher_ui_egui::i18n::set_test_lang(nodalia_launcher_ui_egui::i18n::Lang::EN);
    let events = events().into_iter().filter(|e| !matches!(e, Event::Progress { .. })).collect();
    let mut backend = Some(Fake { events, sent: Rc::default(), manage: true, downloads: Default::default(), clock: Rc::new(Cell::new(NOW)) });
    let mut host = OsMotion(reduce);
    let mut h =
        test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 500.0)).with_step_dt(1.0 / 60.0).with_max_steps(120).build_ui_state(
            move |ui, state: &mut Option<LauncherUi>| {
                if state.is_none()
                    && let Some(b) = backend.take()
                {
                    let settings = Settings { view: ViewMode::Grid, notify: false, ..Settings::default() };
                    *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), settings));
                }
                if let Some(l) = state.as_mut() {
                    l.logic(&ui.ctx().clone(), &mut host);
                    l.ui(ui, &mut host);
                }
            },
            None,
        );
    h.run_steps(8);
    h
}

/// How far down the window the grid's first card is.
fn grid_top(h: &Harness<'_, Option<LauncherUi>>) -> f32 {
    h.get_by_label("Image editing").rect().top()
}

/// Turns the mouse wheel `lines` down over the grid, by way of the shell's `raw_input_hook`. The
/// next frame takes it in.
fn turn_wheel(h: &mut Harness<'_, Option<LauncherUi>>, lines: f32) {
    h.hover_at(egui::pos2(590.0, 300.0));
    h.run_steps(2);
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -lines),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
        source: egui::MouseWheelSource::Wheel,
    };
    let mut raw = egui::RawInput { events: vec![wheel], ..Default::default() };
    if let Some(l) = h.state() {
        l.raw_input(&h.ctx, &mut raw);
    }
    for event in raw.events {
        h.event(event);
    }
}

#[test]
fn the_os_animation_setting_is_followed_as_it_changes() {
    let reduce = Rc::new(Cell::new(true));
    let mut h = short_window(reduce.clone());
    assert_eq!(h.ctx.global_style().animation_time, 0.0);
    // Switched back on while the launcher runs: noticed within a couple of seconds.
    reduce.set(false);
    h.run_steps(150);
    assert_eq!(h.ctx.global_style().animation_time, egui::Style::default().animation_time);
    reduce.set(true);
    h.run_steps(150);
    assert_eq!(h.ctx.global_style().animation_time, 0.0);
}

#[test]
fn with_animations_off_the_wheel_scrolls_at_once() {
    let mut h = short_window(Rc::new(Cell::new(true)));
    let notch = 3.0 * h.ctx.options(|o| o.input_options.line_scroll_speed);
    let start = grid_top(&h);
    turn_wheel(&mut h, 3.0);
    // The frame the wheel turned in, and the one that draws where it scrolled to.
    h.run_steps(2);
    let moved = start - grid_top(&h);
    assert!((moved - notch).abs() < 0.5, "moved {moved} of {notch}");
    // Nothing is left to glide.
    h.run_steps(30);
    assert_eq!(start - grid_top(&h), moved);
}

#[test]
fn with_animations_on_the_wheel_glides() {
    let mut h = short_window(Rc::new(Cell::new(false)));
    let notch = 3.0 * h.ctx.options(|o| o.input_options.line_scroll_speed);
    let start = grid_top(&h);
    turn_wheel(&mut h, 3.0);
    h.run_steps(2);
    let moved = start - grid_top(&h);
    assert!(moved > 0.0 && moved < notch / 2.0, "moved {moved} of {notch}");
    // And is there within the second.
    h.run_steps(60);
    let moved = start - grid_top(&h);
    assert!((moved - notch).abs() < 0.5, "moved {moved} of {notch}");
}

/// Moves the pointer from one card to the next. Returns how many frames are drawn before the
/// window is still again.
fn frames_to_hover_the_next_card(h: &mut Harness<'_, Option<LauncherUi>>) -> u64 {
    let cards: Vec<egui::Pos2> = ["Image editing", "Vector illustration"].map(|t| h.get_by_label(t).rect().center()).to_vec();
    // Still to begin with: the icons are in, the first card is lit and the scroll bar is showing.
    h.hover_at(cards[0]);
    h.run();
    h.hover_at(cards[1]);
    h.run()
}

#[test]
fn with_animations_off_hovering_a_card_shows_at_once() {
    let mut h = short_window(Rc::new(Cell::new(true)));
    let frames = frames_to_hover_the_next_card(&mut h);
    assert!(frames <= 5, "{frames} frames");
}

#[test]
fn with_animations_on_hovering_a_card_animates() {
    let mut h = short_window(Rc::new(Cell::new(false)));
    // The rule wipes in and the icon rises over 0.3 s, at 60 frames a second.
    let frames = frames_to_hover_the_next_card(&mut h);
    assert!(frames >= 10, "{frames} frames");
}

#[test]
fn selected_version_action_previews_its_checked_download_plan() {
    for view in [ViewMode::Grid, ViewMode::Table] {
        let (mut h, _) = harness(view, egui::Theme::Dark, true);
        h.run_steps(4);
        h.state_mut()
            .as_mut()
            .unwrap()
            .settings
            .versions
            .insert("lienzo".into(), nodalia_launcher_core::protocol::VersionTrack::Pinned("v0.5.0".into()));
        // Keep Lienzo's action visible in either view.
        h.get_by_role(egui::accesskit::Role::TextInput).click();
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("Lienzo");
        h.run_steps(4);
        let pos = h.get_by_label("Install version v0.5.0").rect().center();
        assert!(h.ctx.content_rect().contains(pos), "the selected-version action must be visible");
        h.hover_at(pos);
        h.run_steps(40);
        h.get_by_label("Disk image (.dmg) · 150 MB");
    }
}

#[test]
fn install_and_update_actions_preview_download_sizes() {
    for view in [ViewMode::Grid, ViewMode::Table] {
        for label in ["Install", "Update to 0.5.0"] {
            let (mut h, _) = harness(view, egui::Theme::Dark, true);
            h.run_steps(4);
            if view == ViewMode::Grid {
                assert!(h.query_all_by_label("Download size: 150 MB").count() >= 2);
            }
            // Keep the action visible as the suite gains more cards.
            h.get_by_role(egui::accesskit::Role::TextInput).click();
            h.get_by_role(egui::accesskit::Role::TextInput).type_text(if label == "Install" { "Pluma" } else { "Lienzo" });
            h.run_steps(4);
            let pos = h.get_by_label(label).rect().center();
            assert!(h.ctx.content_rect().contains(pos), "the download action must be visible before hovering");
            h.hover_at(pos);
            h.run_steps(40);
            h.get_by_label("Disk image (.dmg) · 150 MB");
            if label == "Update to 0.5.0" {
                snapshot(&mut h, if view == ViewMode::Grid { "download-size-grid" } else { "download-size-table" });
            }
        }
    }
}

#[test]
fn unknown_download_sizes_are_not_presented_as_one_kilobyte() {
    let mut ev = events();
    for event in &mut ev {
        if let Event::Latest { latest: LatestRelease::Found(r), .. } = event {
            for asset in &mut r.assets {
                asset.size = 0;
            }
        }
    }
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(4);
    h.hover_at(h.get_by_label("Update to 0.5.0").rect().center());
    h.run_steps(40);
    assert!(h.query_by_label_contains("Download size:").is_none());
    assert!(h.query_by_label_contains("Disk image (.dmg) ·").is_none());
}

#[test]
fn download_size_preview_uses_the_managed_plan_or_manual_platform_formats() {
    use nodalia_launcher_core::PackageKind;
    struct Windows(Fake, Option<(PackageKind, u64)>);
    impl Backend for Windows {
        fn send(&mut self, c: Command) {
            self.0.send(c);
        }
        fn poll(&mut self) -> Vec<Event> {
            self.0.poll()
        }
        fn capabilities(&self) -> Capabilities {
            self.0.capabilities()
        }
        fn platform(&self) -> Option<Platform> {
            Some(Platform { os: Os::Windows, arch: Arch::X64 })
        }
        fn now(&self) -> i64 {
            self.0.now()
        }
        fn download_size(&self, app: &str) -> Option<(PackageKind, u64)> {
            (app == "lienzo").then_some(self.1).flatten()
        }
    }
    for (manage, installer_size, has_plan) in
        [(false, 100 << 20, true), (false, 0, true), (true, 100 << 20, true), (true, 0, true), (true, 100 << 20, false)]
    {
        let mut r = release("lienzo", "0.5.0", NOW);
        r.assets = [("windows-x64.msi", installer_size), ("windows-x64-portable.zip", 2 << 30), ("linux-x86_64.AppImage", 3 << 20)]
            .into_iter()
            .map(|(suffix, size)| Asset {
                name: format!("lienzo-0.5.0-{suffix}"),
                size,
                url: format!("https://github.com/getnodalia/lienzo/releases/download/v0.5.0/lienzo-0.5.0-{suffix}"),
                sha256: None,
            })
            .collect();
        nodalia_launcher_ui_egui::i18n::set_test_lang(nodalia_launcher_ui_egui::i18n::Lang::EN);
        let mut backend = Some(Windows(
            Fake {
                events: vec![
                    Event::Latest { app: "lienzo".into(), latest: LatestRelease::Found(r) },
                    Event::Detected { app: "lienzo".into(), installed: None },
                    Event::CheckFinished { at: NOW, source: Source::GitHubApi, rate_limited_until: None },
                ],
                sent: Rc::default(),
                manage,
                downloads: Default::default(),
                clock: Rc::new(Cell::new(NOW)),
            },
            has_plan.then_some((PackageKind::Msi, installer_size)),
        ));
        let mut h = test_harness_builder(egui::Theme::Dark).with_size(egui::vec2(1180.0, 900.0)).build_ui_state(
            move |ui, state: &mut Option<LauncherUi>| {
                if state.is_none()
                    && let Some(b) = backend.take()
                {
                    *state = Some(LauncherUi::new(ui.ctx(), Box::new(b), Settings { notify: false, ..Settings::default() }));
                }
                if let Some(l) = state.as_mut() {
                    l.logic(&ui.ctx().clone(), &mut NoHost);
                    l.ui(ui, &mut NoHost);
                }
            },
            None,
        );
        h.run_steps(4);
        // Keep the target card in view regardless of the suite's alphabetical order.
        h.get_by_role(egui::accesskit::Role::TextInput).click();
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("Lienzo");
        h.run_steps(4);
        if manage && has_plan && installer_size > 0 {
            h.get_by_label("Download size: 100 MB");
        } else if !manage && installer_size > 0 {
            h.get_by_label("Download size: 100 MB – 2.0 GB");
        } else {
            assert!(
                h.query_by_label_contains("Download size:").is_none(),
                "an unknown managed plan or partial manual range must not imply a known size"
            );
        }
        let pos = h.get_by_label(if manage { "Install" } else { "Download for Windows" }).rect().center();
        assert!(h.ctx.content_rect().contains(pos), "the download action must be visible before hovering");
        h.hover_at(pos);
        h.run_steps(40);
        if installer_size > 0 && (!manage || has_plan) {
            h.get_by_label_contains("Installer (.msi) · 100 MB");
        } else {
            assert!(h.query_by_label_contains("Installer (.msi) ·").is_none());
        }
        if manage {
            assert!(h.query_by_label_contains("Portable (.zip) · 2.0 GB").is_none(), "managed installs download only the planned asset");
        } else {
            h.get_by_label_contains("Portable (.zip) · 2.0 GB");
        }
        assert!(h.query_by_label_contains("AppImage · 3 MB").is_none());
        if installer_size > 0 && has_plan {
            snapshot(&mut h, if manage { "download-size-windows-managed" } else { "download-size-windows-manual" });
        }
    }
}

#[test]
fn web_download_actions_also_preview_known_sizes() {
    for view in [ViewMode::Grid, ViewMode::Table] {
        let (mut h, _) = harness(view, egui::Theme::Dark, false);
        h.run_steps(4);
        let pos = h.get_all_by_label("Download for macOS").next().unwrap().rect().center();
        h.hover_at(pos);
        h.run_steps(40);
        h.get_by_label("Disk image (.dmg) · 150 MB");
    }
}

#[test]
/// Command-plus-wheel over the main UI raises the global zoom factor.
fn control_wheel_zooms_the_main_ui() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(4);
    let before = h.ctx.zoom_factor();
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        phase: egui::TouchPhase::Move,
        delta: egui::vec2(0.0, 100.0),
        modifiers: egui::Modifiers::COMMAND,
        source: egui::MouseWheelSource::Unknown,
    });
    h.step();
    h.run_steps(2);
    assert!(h.ctx.zoom_factor() > before);
}

#[test]
/// With release notes open, keyboard and wheel zoom change only the notes text (reset works)
/// while the global zoom stays put and the release heading appears once.
fn changelog_shortcuts_zoom_only_the_notes_text() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(4);
    // The grid's default order is name A-Z, so pick the app explicitly: the first card is not
    // Lienzo, and the heading assertion below is about this release's own page.
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(2);
    h.get_by_label("Release notes").click();
    h.run_steps(4);
    assert_eq!(
        h.query_all_by_role_and_label(egui::accesskit::Role::Image, "Lienzo 0.5.0").count(),
        0,
        "the Notes hero icon is decorative and must not repeat the release heading"
    );
    assert_eq!(h.query_all_by_role_and_label(egui::accesskit::Role::Label, "Lienzo 0.5.0").count(), 1, "the release heading appears exactly once");
    let before = h.get_by_label("What's Changed").rect().height();
    let global = h.ctx.zoom_factor();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
    h.run_steps(3);
    assert_eq!(h.ctx.zoom_factor(), global, "zooming notes must not resize the main UI");
    assert!(h.get_by_label("What's Changed").rect().height() > before);

    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num0);
    h.run_steps(3);
    assert!((h.get_by_label("What's Changed").rect().height() - before).abs() < 0.5);
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        phase: egui::TouchPhase::Move,
        delta: egui::vec2(0.0, 100.0),
        modifiers: egui::Modifiers::COMMAND,
        source: egui::MouseWheelSource::Unknown,
    });
    h.step();
    h.run_steps(2);
    assert_eq!(h.ctx.zoom_factor(), global);
    let larger = h.get_by_label("What's Changed").rect().height();
    assert!(larger > before);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
    h.run_steps(3);
    assert!(h.get_by_label("What's Changed").rect().height() < larger);
    snapshot(&mut h, "notes-zoomed");
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert_eq!(h.ctx.zoom_factor(), global);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Equals);
    h.run_steps(3);
    assert!(h.ctx.zoom_factor() > global, "main-UI keyboard zoom should still work after closing notes");
}

#[test]
/// The notes card grows with the zoomed text, stops at the window, and comes back on reset (#594).
fn the_notes_card_follows_the_zoom_within_the_window() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(4);
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(2);
    h.get_by_label("Release notes").click();
    h.run_steps(4);
    let span = |h: &Harness<'_, Option<LauncherUi>>| h.get_by_label("Close").rect().right() - h.get_by_label("Open on GitHub").rect().left();
    let usual = span(&h);
    for _ in 0..3 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
        h.run_steps(3);
    }
    assert!(span(&h) > usual * 1.2, "the card kept its width: {usual} -> {}", span(&h));
    for _ in 0..30 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
        h.run_steps(3);
    }
    let in_window = |h: &Harness<'_, Option<LauncherUi>>| {
        let window = h.ctx.content_rect();
        for label in ["Close", "Open on GitHub"] {
            assert!(window.contains_rect(h.get_by_label(label).rect()), "{label} left {window:?}");
        }
    };
    in_window(&h);
    // The launcher's smallest window.
    let full = h.ctx.content_rect().size();
    h.set_size(egui::vec2(720.0, 500.0));
    h.run_steps(3);
    in_window(&h);
    h.set_size(full);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num0);
    h.run_steps(3);
    assert!((span(&h) - usual).abs() < 0.5, "reset left the card at {}, was {usual}", span(&h));
    // Below 100% only the text shrinks.
    for _ in 0..3 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Minus);
        h.run_steps(3);
    }
    assert!((span(&h) - usual).abs() < 0.5, "zooming out left the card at {}, was {usual}", span(&h));
}

#[test]
/// Notes too long to fit take the zoom's extra height too (#594).
fn long_notes_get_a_taller_card_when_zoomed() {
    let notes: String = (0..200).map(|n| format!("Line {n}\n\n")).collect();
    let mut ev = events();
    for e in &mut ev {
        if let Event::Latest { latest: LatestRelease::Found(r), .. } = e {
            r.notes.clone_from(&notes);
        }
    }
    let (mut h, _) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(4);
    h.get_by_label("More actions for Lienzo").click();
    h.run_steps(2);
    h.get_by_label("Release notes").click();
    h.run_steps(4);
    let bottom = |h: &Harness<'_, Option<LauncherUi>>| h.get_by_label("Close").rect().bottom();
    let usual = bottom(&h);
    for _ in 0..3 {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Plus);
        h.run_steps(3);
    }
    assert!(bottom(&h) > usual + 60.0, "the card kept its height: {usual} -> {}", bottom(&h));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num0);
    h.run_steps(3);
    assert!((bottom(&h) - usual).abs() < 0.5, "reset left the card at {}, was {usual}", bottom(&h));
}

#[test]
fn command_m_minimizes_the_mac_window_only() {
    use nodalia_launcher_ui_egui::TitleBar;
    for bar in [TitleBar::MacOs, TitleBar::Native, TitleBar::Custom] {
        let (mut h, _) = harness_with_title_bar(events(), ViewMode::Grid, egui::Theme::Dark, true, 1.0, bar);
        h.run_steps(4);
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::M,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::MAC_CMD,
        });
        h.step();
        let minimized = h.output().viewport_output.values().any(|v| v.commands.iter().any(|c| matches!(c, egui::ViewportCommand::Minimized(true))));
        assert_eq!(minimized, bar == TitleBar::MacOs, "{bar:?}");
    }
}

#[test]
fn an_occupied_folder_asks_before_replacing_it() {
    let path = "/Volumes/Nodalia Apps/Lienzo.app";
    let mut ev = events();
    ev.push(Event::Occupied { app: "lienzo".into(), path: path.into() });
    ev.push(Event::JobFinished { app: "lienzo".into(), result: Err(format!("{path} already exists")) });
    let (mut h, sent) = harness_with(ev, ViewMode::Grid, egui::Theme::Dark, true, 1.0);
    h.run_steps(6);
    h.get_by_label(&format!("{path} already exists. Installing Lienzo will overwrite it."));
    assert!(h.query_by_label(&format!("{path} already exists")).is_none());
    sent.borrow_mut().clear();
    h.get_by_label("Replace").click();
    h.run_steps(2);
    assert_eq!(*sent.borrow(), [Command::Replace { app: "lienzo".into(), path: path.into() }]);
    assert!(h.query_by_label("Replace").is_none());
}

#[test]
fn a_not_installed_card_is_grey_until_the_pointer_is_over_it() {
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.set_size(egui::vec2(1180.0, 1900.0));
    h.run_steps(8);
    let icon = h.get_by_label("Celdas icon").rect();
    // The most colourful pixel in the icon: how far apart its red, green and blue are.
    let colour = |h: &mut Harness<'_, Option<LauncherUi>>| {
        let image = h.render().unwrap();
        let mut most = 0_i32;
        for y in icon.top() as u32..icon.bottom() as u32 {
            for x in icon.left() as u32..icon.right() as u32 {
                let p = image.get_pixel(x, y).0;
                let (hi, lo) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
                most = most.max(i32::from(hi) - i32::from(lo));
            }
        }
        most
    };
    let c = colour(&mut h);
    // The panel behind the icon's rounded corners is a touch blue, so allow its own tint.
    assert!(c <= 16, "a not installed app's icon is grey, colour={c}");
    h.hover_at(egui::pos2(icon.center().x + 120.0, icon.center().y + 60.0));
    // The colour picture decodes on a background thread: give it real time, as a slow machine
    // (CI's Windows runner) needs more than a few frames.
    let mut shown = 0;
    for _ in 0..100 {
        h.run_steps(2);
        shown = colour(&mut h);
        if shown > 40 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(shown > 40, "the icon shows its colour under the pointer");
    h.hover_at(egui::pos2(5.0, 400.0));
    h.run_steps(8);
    assert!(colour(&mut h) <= 16, "the colour goes again when the pointer leaves");
}

#[test]
fn the_sort_offers_name_both_ways_recently_updated_and_frequently_used() {
    use nodalia_launcher_ui_egui::settings::SortKey;
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.get_by_value("Name (A–Z)").click();
    h.run_steps(3);
    for label in ["Name (A–Z)", "Name (Z–A)", "Recently updated", "Frequently used"] {
        h.get_by_label(label);
    }
    h.get_by_label("Name (Z–A)").click();
    h.run_steps(4);
    let s = &h.state().as_ref().unwrap().settings;
    assert_eq!((s.sort, s.sort_descending), (SortKey::Name, true));
    h.get_by_value("Name (Z–A)").click();
    h.run_steps(3);
    h.get_by_label("Frequently used").click();
    h.run_steps(4);
    assert_eq!(h.state().as_ref().unwrap().settings.sort, SortKey::Frequent);
}

#[test]
fn opening_an_app_counts_and_frequently_used_puts_the_most_opened_first() {
    use nodalia_launcher_ui_egui::settings::SortKey;
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    h.state_mut().as_mut().unwrap().settings.sort = SortKey::Frequent;
    h.run_steps(4);
    // Installed apps in name order: Letra (Open), Lienzo (updates), Luz (Open), Pulso (updates).
    let opens: Vec<_> = h.get_all_by_label("Open").collect();
    assert_eq!(opens.len(), 2);
    opens[0].click(); // Letra
    h.run_steps(3);
    assert!(sent.borrow().contains(&Command::Launch { app: "letra".into(), args: vec![] }));
    assert_eq!(h.state().as_ref().unwrap().settings.launches.get("letra"), Some(&1));
    h.run_steps(4);
    let word = h.get_by_label("Letra details").rect();
    let light = h.get_by_label("Luz details").rect();
    assert!(word.min.y < light.min.y || (word.min.y == light.min.y && word.min.x < light.min.x), "{word:?} {light:?}");
}

#[test]
fn an_apps_page_is_a_card_in_the_middle_with_next_and_previous_arrows_and_closes_from_outside() {
    use nodalia_launcher_core::details::AppDetails;
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(6);
    let headline = |id: &str| AppDetails::by_id(id).unwrap().headline;
    let showing = |h: &Harness<'_, Option<LauncherUi>>, id: &str| h.query_all_by_label_contains(headline(id)).count() >= 2;
    h.get_by_label("Lienzo details").click();
    h.run_steps(12);
    assert!(showing(&h, "lienzo"));
    // A card with a margin of window all round, not a panel against one edge.
    let close = h.get_by_label("Close (Esc)").rect();
    assert!(close.right() < 1180.0 - 24.0 && close.left() > 24.0, "{close:?}");
    // Next goes to the following app in the grid's order (Luz after Lienzo), and back again.
    h.get_by_label("Next app").click();
    h.run_steps(12);
    assert!(showing(&h, "luz"));
    h.get_by_label("Previous app").click();
    h.run_steps(12);
    assert!(showing(&h, "lienzo"));
    // A click on the card's own margin keeps it open (the card is 1116 wide, centred in 1180).
    let margin = egui::pos2(36.0, 450.0);
    h.hover_at(margin);
    h.run_steps(2);
    h.drag_at(margin);
    h.run_steps(2);
    h.drop_at(margin);
    h.run_steps(12);
    assert!(showing(&h, "lienzo"), "a click on the card closed it");
    // A click on the dimmed window outside the card closes it.
    h.hover_at(egui::pos2(10.0, 450.0));
    h.run_steps(2);
    h.drag_at(egui::pos2(10.0, 450.0));
    h.run_steps(2);
    h.drop_at(egui::pos2(10.0, 450.0));
    h.run_steps(30);
    assert!(!showing(&h, "lienzo"), "a click outside did not close it");
}

/// A version pin is distinct from a favorite: the lock names the exact tag and requires confirmation.
#[test]
fn version_padlock_confirms_before_unlocking_in_grid_and_table() {
    use nodalia_launcher_core::protocol::VersionTrack;
    let tip = "Version pinned to v0.4.1. Click to unlock.";
    for view in [ViewMode::Grid, ViewMode::Table] {
        let (mut h, sent) = harness(view, egui::Theme::Dark, true);
        h.run_steps(4);
        h.state_mut().as_mut().unwrap().settings.versions.insert("lienzo".into(), VersionTrack::Pinned("v0.4.1".into()));
        h.run_steps(4);
        sent.borrow_mut().clear();
        h.get_by_label(tip).hover();
        h.run_steps(30);
        snapshot(&mut h, if view == ViewMode::Grid { "version-lock-grid" } else { "version-lock-table" });
        h.query_all_by_role_and_label(egui::accesskit::Role::Button, tip).next().unwrap().click();
        h.run_steps(4);
        h.get_by_label("Unlock version?");
        h.get_by_label("Lienzo is pinned to v0.4.1. Follow the latest release instead?");
        assert_eq!(h.state().as_ref().unwrap().settings.versions.get("lienzo"), Some(&VersionTrack::Pinned("v0.4.1".into())));
        assert!(!sent.borrow().iter().any(|c| matches!(c, Command::CheckForUpdates { force: true })));
        snapshot(&mut h, if view == ViewMode::Grid { "version-unlock-dialog" } else { "version-unlock-table-dialog" });
        h.get_by_label("Cancel").click();
        h.run_steps(4);
        assert_eq!(h.state().as_ref().unwrap().settings.versions.get("lienzo"), Some(&VersionTrack::Pinned("v0.4.1".into())));
        h.get_by_label(tip).click();
        h.run_steps(4);
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert_eq!(h.state().as_ref().unwrap().settings.versions.get("lienzo"), Some(&VersionTrack::Pinned("v0.4.1".into())));
        h.get_by_label(tip).click();
        h.run_steps(4);
        h.get_by_label("Unlock").click();
        h.run_steps(4);
        assert!(!h.state().as_ref().unwrap().settings.versions.contains_key("lienzo"));
        assert!(h.query_by_label(tip).is_none());
        assert!(sent.borrow().iter().any(|c| matches!(c, Command::Configure(options) if !options.versions.contains_key("lienzo"))));
        assert!(sent.borrow().iter().any(|c| matches!(c, Command::CheckForUpdates { force: true })));
        assert!(!sent.borrow().iter().any(|c| matches!(c, Command::Install { .. })));
    }
}

#[test]
fn a_version_padlock_does_not_unlock_a_choice_changed_after_confirmation_opened() {
    use nodalia_launcher_core::protocol::VersionTrack;
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(4);
    h.state_mut().as_mut().unwrap().settings.versions.insert("lienzo".into(), VersionTrack::Pinned("v0.4.1".into()));
    h.run_steps(4);
    h.get_by_label("Version pinned to v0.4.1. Click to unlock.").click();
    h.run_steps(4);
    h.state_mut().as_mut().unwrap().settings.versions.insert("lienzo".into(), VersionTrack::Pinned("v0.3.0".into()));
    h.run_steps(4);
    h.get_by_label("Unlock").click();
    h.run_steps(4);
    assert_eq!(h.state().as_ref().unwrap().settings.versions.get("lienzo"), Some(&VersionTrack::Pinned("v0.3.0".into())));
}

#[test]
fn favorite_and_nightly_choices_do_not_show_a_version_padlock() {
    use nodalia_launcher_core::protocol::VersionTrack;
    let (mut h, _) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(4);
    h.state_mut().as_mut().unwrap().settings.pin_app("lienzo");
    h.run_steps(4);
    assert!(h.query_by_label("Version pinned to v0.4.1. Click to unlock.").is_none());
    h.state_mut().as_mut().unwrap().settings.versions.insert("lienzo".into(), VersionTrack::Nightly);
    h.run_steps(4);
    assert!(h.query_by_label("Version pinned to v0.4.1. Click to unlock.").is_none());
}

#[test]
fn launcher_version_padlock_confirms_and_returns_to_settings() {
    use nodalia_launcher_core::protocol::VersionTrack;
    let (mut h, sent) = harness(ViewMode::Grid, egui::Theme::Dark, true);
    h.run_steps(4);
    h.state_mut().as_mut().unwrap().settings.launcher_version = VersionTrack::Pinned("v0.19.0".into());
    h.get_by_label("Main menu").click();
    h.run_steps(2);
    h.get_by_label("Settings").click();
    h.run_steps(6);
    h.get_by_label("Version pinned to v0.19.0. Click to unlock.").click();
    h.run_steps(4);
    h.get_by_label("Cancel").click();
    h.run_steps(4);
    assert_eq!(h.state().as_ref().unwrap().settings.launcher_version, VersionTrack::Pinned("v0.19.0".into()));
    h.get_by_label("Version pinned to v0.19.0. Click to unlock.").click();
    h.run_steps(4);
    h.get_by_label("Unlock").click();
    h.run_steps(4);
    assert_eq!(h.state().as_ref().unwrap().settings.launcher_version, VersionTrack::Stable);
    h.get_by_label("Launcher version");
    assert!(sent.borrow().iter().any(|c| matches!(c, Command::Configure(options) if options.launcher_version == VersionTrack::Stable)));
}
