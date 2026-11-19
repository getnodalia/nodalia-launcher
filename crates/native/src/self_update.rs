//! Nodalia Launcher updating itself, through `nodalia-update`: the same download, check and
//! replace flow every Nodalia app uses.
//!
//! The launcher already knows the newer release (its release source found it), so this only
//! downloads the package for this copy's install kind, checks its SHA-256 and replaces the copy.

use nodalia_launcher_core::PackageKind;
use nodalia_launcher_core::catalog::LAUNCHER;
use nodalia_launcher_core::platform::{Platform, pick_asset};
use nodalia_launcher_core::protocol::JobPhase;
use nodalia_launcher_core::release::Release;
use nodalia_launcher_core::version::{self, Version};

use crate::http::{Http, Stop};
use crate::paths::Paths;

/// How this copy is installed. Only an AppImage, a macOS app bundle and a Windows MSI install can
/// replace themselves; the rest (Flatpak, packages, portable and read-only copies) are updated by
/// whatever installed them.
pub type Install = nodalia_update::InstallKind;

fn app() -> nodalia_update::App {
    nodalia_update::App {
        name: LAUNCHER.name,
        repo: LAUNCHER.repo,
        bundle: "Nodalia Launcher.app",
        version: env!("CARGO_PKG_VERSION"),
        edition: None,
    }
}

/// The version this launcher counts as.
pub fn current_version() -> Option<Version> {
    version::parse(env!("CARGO_PKG_VERSION"))
}

/// How this copy is installed, if it's one the launcher can replace in place.
pub fn current() -> Option<Install> {
    Some(nodalia_update::install_kind(&app())).filter(Install::can_apply_in_place)
}

/// Whether this copy can update itself.
pub fn is_self_updating() -> bool {
    current().is_some()
}

/// Why the last update did not install, once, in English (the UI translates it).
pub fn take_failure(_paths: &Paths) -> Option<String> {
    use nodalia_update::Failure;
    let why = match nodalia_update::take_last_failure(&app())? {
        Failure::StillRunning => "The update wasn't installed because Nodalia Launcher didn't close in time",
        Failure::Mount => "The update wasn't installed: its disk image couldn't be opened",
        Failure::Contents => "The update wasn't installed: its disk image holds no Nodalia Launcher",
        Failure::Copy => "The update wasn't installed: the new version couldn't be copied",
        Failure::Replace => "The update wasn't installed: macOS didn't allow replacing the app (see Privacy & Security, App Management)",
        Failure::Install => "The update wasn't installed: the installer failed",
        Failure::Cancelled => "The update wasn't installed: the installer was cancelled",
        _ => "The update wasn't installed",
    };
    Some(why.to_string())
}

/// The package format updating `install` downloads: the one it was installed from.
pub fn kind(install: &Install) -> PackageKind {
    match install {
        Install::MacBundle(_) => PackageKind::Dmg,
        Install::WindowsMsi { .. } => PackageKind::Msi,
        Install::AppImage(_) => PackageKind::AppImage,
        Install::Portable(_) => PackageKind::PortableZip,
        _ => PackageKind::TarGz,
    }
}

/// Whether `release` has the file that updating `install` on `platform` downloads.
pub fn installs_from(install: &Install, release: &Release, platform: Platform) -> bool {
    install.can_apply_in_place() && pick_asset(release, &[LAUNCHER.id], platform, &[kind(install)]).is_some()
}

/// Downloads `release`, read from `repo`, for `platform`, puts it in place and starts the new
/// version. On success the caller must quit promptly. `last` is called once the download is ready
/// and returns when the launcher may replace itself and restart.
#[allow(clippy::too_many_arguments)]
pub fn update(
    install: &Install,
    release: &Release,
    repo: &str,
    platform: Platform,
    _http: &Http,
    _paths: &Paths,
    stop: &Stop,
    phase: &mut dyn FnMut(JobPhase),
    last: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    if repo != LAUNCHER.repo {
        return Err("the update isn't from Nodalia's launcher repository".into());
    }
    let wanted = kind(install);
    let Some((asset, _)) = pick_asset(release, &[LAUNCHER.id], platform, &[wanted]) else {
        return Err(format!("release {} has no {} for {}", release.version_label(), wanted.label(), platform.label()));
    };
    let package = nodalia_update::Release {
        tag: release.tag.clone(),
        version: release.version_label(),
        notes: release.notes.clone(),
        page: release.html_url.clone(),
        asset: Some(nodalia_update::ReleaseAsset {
            name: asset.name.clone(),
            url: asset.url.clone(),
            sha256: asset.sha256.clone(),
            size: Some(asset.size),
        }),
    };
    let app = app();
    phase(JobPhase::Downloading { done: 0, total: asset.size });
    let path = nodalia_update::download(&app, &package, |done, total| {
        phase(JobPhase::Downloading { done, total: total.unwrap_or(asset.size) });
    })
    .map_err(|e| e.to_string())?;
    if stop.is_stopped() {
        return Err(nodalia_launcher_core::protocol::CANCELLED.into());
    }
    phase(JobPhase::Installing);
    nodalia_update::apply(&app, &path, last).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nodalia_launcher_core::release::Asset;
    use nodalia_launcher_core::{Arch, Os};
    use std::path::PathBuf;

    fn release(names: &[&str]) -> Release {
        Release {
            assets: names.iter().map(|n| Asset { name: (*n).into(), size: 1, url: format!("https://example.invalid/{n}"), sha256: None }).collect(),
            ..Release::default()
        }
    }

    #[test]
    fn a_development_build_does_not_update_itself() {
        assert_eq!(current(), None);
        assert!(!is_self_updating());
    }

    #[test]
    fn an_update_downloads_the_format_the_copy_was_installed_from() {
        assert_eq!(kind(&Install::WindowsMsi { dir: PathBuf::new(), per_machine: false }), PackageKind::Msi);
        assert_eq!(kind(&Install::MacBundle(PathBuf::new())), PackageKind::Dmg);
        assert_eq!(kind(&Install::AppImage(PathBuf::new())), PackageKind::AppImage);
    }

    /// The update is offered to install itself only when the release has the file for this
    /// platform in the format the copy came from.
    #[test]
    fn an_update_is_installable_only_when_the_release_has_its_file() {
        let names = [
            "nodalia-launcher-0.2.0-linux-aarch64.AppImage",
            "nodalia-launcher-0.2.0-linux-x86_64.AppImage",
            "nodalia-launcher-0.2.0-macos-universal.dmg",
            "nodalia-launcher-0.2.0-windows-x64.msi",
            "nodalia-launcher-0.2.0-windows-x64-portable.zip",
            "SHA256SUMS",
        ];
        let release = release(&names);
        let platform = |os, arch| Platform { os, arch };
        let appimage = Install::AppImage(PathBuf::new());
        let mac = Install::MacBundle(PathBuf::new());
        let msi = Install::WindowsMsi { dir: PathBuf::new(), per_machine: false };
        assert!(installs_from(&appimage, &release, platform(Os::Linux, Arch::X64)));
        assert!(installs_from(&appimage, &release, platform(Os::Linux, Arch::Arm64)));
        assert!(!installs_from(&appimage, &release, platform(Os::Linux, Arch::Riscv64)));
        assert!(installs_from(&mac, &release, platform(Os::MacOs, Arch::Arm64)));
        assert!(installs_from(&msi, &release, platform(Os::Windows, Arch::X64)));
        assert!(!installs_from(&msi, &release, platform(Os::Windows, Arch::X86)));
        // A copy something else manages never replaces itself.
        assert!(!installs_from(&Install::Managed, &release, platform(Os::Linux, Arch::X64)));
        assert!(!installs_from(&mac, &Release::default(), platform(Os::MacOs, Arch::Arm64)));
    }

    #[test]
    fn an_update_from_another_repository_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths { data: dir.path().join("data"), cache: dir.path().join("cache") };
        let http = Http::new().unwrap();
        let release = release(&["nodalia-launcher-0.2.0-linux-x86_64.AppImage"]);
        let platform = Platform { os: Os::Linux, arch: Arch::X64 };
        let error =
            update(&Install::AppImage(PathBuf::new()), &release, "someone/else", platform, &http, &paths, &Stop::default(), &mut |_| {}, &|| Ok(()))
                .unwrap_err();
        assert!(error.contains("isn't from Nodalia"), "{error}");
        let error = update(
            &Install::AppImage(PathBuf::new()),
            &Release::default(),
            LAUNCHER.repo,
            platform,
            &http,
            &paths,
            &Stop::default(),
            &mut |_| {},
            &|| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("has no AppImage"), "{error}");
    }
}
