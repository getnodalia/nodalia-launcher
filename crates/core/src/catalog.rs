//! The Nodalia apps: bundled data.
//!
//! The list is data (`catalog.toml`, compiled by `build.rs`): [`SUITE`] is the built-in copy.
//!
//! Text here is English, the source the UI translates from: each `tagline` has a catalog row
//! keyed by the app id (`tr_ctx(app.id, app.tagline)` in `ui-egui`).

use crate::platform::Os;

/// The family an app belongs to; the launcher groups and filters by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Suite {
    Office,
    Creator,
    Engineering,
    /// Everyday apps that belong to none of the three work suites.
    Everyday,
}

impl Suite {
    pub const ALL: [Suite; 4] = [Self::Office, Self::Creator, Self::Engineering, Self::Everyday];

    /// English name; the UI translates it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Office => "Office",
            Self::Creator => "Creator",
            Self::Engineering => "Engineering",
            Self::Everyday => "Everyday",
        }
    }
}

/// One app in the suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuiteApp {
    /// Machine name: repo, binary, package and asset stem (`lienzo`).
    pub id: &'static str,
    /// Display name (`Lámina`).
    pub name: &'static str,
    pub suite: Suite,
    /// What it's for, in a few words (English; the UI translates it by app id).
    pub tagline: &'static str,
    /// The app's accent colour (sRGB), for surfaces: its NDL accent.
    pub color: [u8; 3],
    /// The accent for text on a light background.
    pub ink_light: [u8; 3],
    /// The accent for text on a dark background.
    pub ink_dark: [u8; 3],
    /// GitHub `owner/repo`.
    pub repo: &'static str,
    /// The macOS bundle id, Flatpak id and Linux desktop-file id (`dev.danielmiguelt.Lamina`).
    pub bundle: &'static str,
    /// The `.app` bundle stem on macOS (`Lamina`, `Vinilo Nodalia`).
    pub mac_bundle_name: &'static str,
    /// The systems it is built for; empty means all of them.
    pub systems: &'static [Os],
}

impl SuiteApp {
    pub fn bundle_id(&self) -> String {
        self.bundle.to_string()
    }

    pub fn releases_url(&self) -> String {
        format!("https://github.com/{}/releases", self.repo)
    }

    pub fn repo_url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }

    /// The app's page on getnodalia.com.
    pub fn website(&self) -> String {
        format!("{WEBSITE_URL}/{}", self.id)
    }

    /// Whether the launcher installs this app from a `kind` package.
    pub fn installs(&self, _kind: crate::PackageKind) -> bool {
        true
    }

    /// Whether this app is built for the given operating system.
    pub fn runs_on(&self, os: Os) -> bool {
        self.systems.is_empty() || self.systems.contains(&os)
    }

    /// Bundle ids this app may be installed under.
    pub fn bundle_ids(&self) -> impl Iterator<Item = String> + '_ {
        std::iter::once(self.bundle_id())
    }

    /// Asset stems this app's releases use.
    pub fn stems(&self) -> impl Iterator<Item = &'static str> + '_ {
        std::iter::once(self.id)
    }

    /// Display names this app's installers register.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        std::iter::once(self.name)
    }

    /// The `.app` bundle stem on macOS.
    pub fn mac_bundle_name(&self) -> &'static str {
        self.mac_bundle_name
    }

    /// Bundle stems a macOS `.app` may carry, release name first.
    pub fn mac_bundle_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        let primary = self.mac_bundle_name;
        std::iter::once(primary).chain(std::iter::once(self.name).filter(move |&n| n != primary))
    }

    pub fn by_id(id: &str) -> Option<&'static SuiteApp> {
        SUITE.iter().find(|a| a.id == id)
    }
}

// The bundled suite, in display order: Office, Creator, Engineering, Everyday.
include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

/// The name every Nodalia installer registers as its publisher (the Windows Installer's
/// `Manufacturer`, a package's `Maintainer`).
pub const PUBLISHER: &str = "Nodalia";

/// The folder under `Program Files` (and the per-user `Programs` folder) that the apps' MSI
/// packages install into, `<folder>\<App>`. A path on disk that the apps' installers fix, not a
/// product name: detection and the default install folder must match it.
pub const MSI_SUITE_FOLDER: &str = "Nodalia";

/// Suite folders earlier releases of some apps installed into, still searched when looking for an
/// installed app (never offered as a default).
pub const MSI_LEGACY_SUITE_FOLDERS: &[&str] = &["ArtCraft"];

/// Nodalia's website.
pub const WEBSITE_URL: &str = "https://getnodalia.com";
/// The GitHub account every Nodalia app's repository lives under. Downloads must come from a
/// repository under it.
pub const GITHUB_OWNER: &str = "getnodalia";
/// Nodalia on GitHub: every app's source.
pub const GITHUB_URL: &str = "https://github.com/getnodalia";
/// Where bugs and ideas about the launcher go.
pub const ISSUES_URL: &str = "https://github.com/getnodalia/nodalia-launcher/issues";
/// Where to support the project.
pub const DONATE_URL: &str = "https://buymeacoffee.com/danielmigueltejedor";

/// Nodalia Launcher itself, described like an app so it can update itself with the same
/// machinery (asset names `nodalia-launcher-<ver>-…`, bundle id `dev.danielmiguelt.NodaliaLauncher`,
/// `Nodalia Launcher.app`). Not part of [`SUITE`].
pub const LAUNCHER: SuiteApp = SuiteApp {
    id: "nodalia-launcher",
    name: "Nodalia Launcher",
    suite: Suite::Everyday,
    tagline: "Installs and updates the Nodalia apps",
    color: [10, 130, 144],
    ink_light: [8, 103, 112],
    ink_dark: [44, 197, 211],
    repo: "getnodalia/nodalia-launcher",
    bundle: "dev.danielmiguelt.NodaliaLauncher",
    mac_bundle_name: "Nodalia Launcher",
    systems: &[],
};

/// The launcher's own repo, checked so it can say when a newer launcher is out.
pub const LAUNCHER_REPO: &str = "getnodalia/nodalia-launcher";
/// The launcher's own asset stem.
pub const LAUNCHER_ID: &str = "nodalia-launcher";

include!(concat!(env!("OUT_DIR"), "/app_icons.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_has_its_icon() {
        for app in &SUITE {
            let png = app_icon_png(app.id).unwrap_or_else(|| panic!("no icon for {}", app.id));
            assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "{} icon isn't a PNG", app.id);
        }
        assert!(app_icon_png("notanapp").is_none());
    }

    #[test]
    fn suite_has_twenty_four_apps_with_unique_ids() {
        assert_eq!(SUITE.len(), 24);
        let mut ids: Vec<_> = SUITE.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SUITE.len());
        for a in &SUITE {
            assert_eq!(a.repo, format!("getnodalia/{}", a.id));
            assert!(a.bundle.starts_with("dev.danielmiguelt."), "{}", a.id);
            assert!(a.mac_bundle_name.is_ascii(), "{}", a.id);
        }
    }

    #[test]
    fn the_suites_group_the_apps() {
        let in_suite = |suite| SUITE.iter().filter(|a| a.suite == suite).map(|a| a.id).collect::<Vec<_>>();
        assert_eq!(in_suite(Suite::Engineering), ["forja", "torno", "vortia", "circuito", "abaco", "codigo"]);
        assert_eq!(in_suite(Suite::Everyday), ["vinilo", "brujula"]);
        assert_eq!(SUITE.iter().filter(|a| a.suite == Suite::Office).count(), 7);
        assert_eq!(SUITE.iter().filter(|a| a.suite == Suite::Creator).count(), 9);
    }

    #[test]
    fn bundle_ids_and_websites() {
        let app = SuiteApp::by_id("lamina").unwrap();
        assert_eq!(app.name, "Lámina");
        assert_eq!(app.bundle_id(), "dev.danielmiguelt.Lamina");
        assert_eq!(app.website(), "https://getnodalia.com/lamina");
        assert_eq!(app.releases_url(), "https://github.com/getnodalia/lamina/releases");
        assert_eq!(LAUNCHER.bundle_id(), "dev.danielmiguelt.NodaliaLauncher");
    }

    #[test]
    fn vinilo_ships_as_the_nodalia_edition_on_mac() {
        let vinilo = SuiteApp::by_id("vinilo").unwrap();
        assert_eq!(vinilo.mac_bundle_name(), "Vinilo Nodalia");
        assert_eq!(vinilo.mac_bundle_names().collect::<Vec<_>>(), ["Vinilo Nodalia", "Vinilo"]);
        assert_eq!(vinilo.bundle_id(), "dev.danielmiguelt.Vinilo");
    }

    #[test]
    fn platform_support_follows_the_catalog() {
        let lienzo = SuiteApp::by_id("lienzo").unwrap();
        assert!([Os::Windows, Os::MacOs, Os::Linux, Os::FreeBsd].iter().all(|os| lienzo.runs_on(*os)));
        let brujula = SuiteApp::by_id("brujula").unwrap();
        assert!(brujula.runs_on(Os::Linux) && !brujula.runs_on(Os::MacOs) && !brujula.runs_on(Os::Windows));
        assert!(!SuiteApp::by_id("vinilo").unwrap().runs_on(Os::Windows));
    }
}
