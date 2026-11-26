#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::Deserialize;
use std::{env, error::Error, fmt::Write as _, fs, path::PathBuf};

#[derive(Deserialize)]
struct Manifest {
    apps: Vec<App>,
}
#[derive(Deserialize)]
struct App {
    id: String,
    name: String,
    suite: String,
    tagline: String,
    color: [u8; 3],
    ink_light: [u8; 3],
    ink_dark: [u8; 3],
    repo: String,
    bundle: String,
    mac_bundle_name: String,
    #[serde(default)]
    systems: Vec<String>,
    details: Option<Details>,
}
#[derive(Deserialize)]
struct Details {
    headline: String,
    about: String,
    #[serde(default)]
    features: Vec<Feature>,
    #[serde(default)]
    shots: Vec<Shot>,
}
#[derive(Deserialize)]
struct Feature {
    title: String,
    text: String,
}
#[derive(Deserialize)]
struct Shot {
    key: String,
    caption: String,
}
fn system(name: &str) -> Result<&'static str, String> {
    Ok(match name {
        "windows" => "Os::Windows",
        "macos" => "Os::MacOs",
        "linux" => "Os::Linux",
        "freebsd" => "Os::FreeBsd",
        other => return Err(format!("unknown system {other}")),
    })
}
/// `app_icon_png`: every catalog app's 128 px icon, read from `assets/app-icons/<id>.png`. The
/// icon is bundled when the file exists, so a new app needs only its row in `catalog.toml` and its
/// icon dropped into that folder.
fn app_icons(manifest: &Manifest) -> Result<String, Box<dyn Error>> {
    let dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR missing")?).join("../../assets/app-icons");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut arms = String::new();
    for app in &manifest.apps {
        let file = dir.join(format!("{}.png", app.id));
        println!("cargo:rerun-if-changed={}", file.display());
        if file.is_file() {
            let path = file.canonicalize()?;
            writeln!(arms, "{:?} => include_bytes!({:?}),", app.id, path.to_string_lossy())?;
        }
    }
    Ok(format!(
        "/// An app's 128 px icon (PNG), from `assets/app-icons`: the UI draws it, and the Linux install\n/// puts it in the desktop menu. `None` for an app with no icon file yet.\npub fn app_icon_png(id: &str) -> Option<&'static [u8]> {{\n    Some(match id {{\n{arms}        _ => return None,\n    }})\n}}\n"
    ))
}
fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=catalog.toml");
    let manifest: Manifest = toml::from_str(&fs::read_to_string("catalog.toml")?)?;
    let mut apps = format!("pub const SUITE: [SuiteApp; {}] = [\n", manifest.apps.len());
    let count = manifest.apps.iter().filter(|a| a.details.is_some()).count();
    let mut details = format!("pub const DETAILS: [AppDetails; {count}] = [\n");
    for app in &manifest.apps {
        let suite = match app.suite.as_str() {
            "office" => "Office",
            "creator" => "Creator",
            "engineering" => "Engineering",
            "enterprise" => "Enterprise",
            "everyday" => "Everyday",
            _ => return Err(format!("unknown suite for {}", app.id).into()),
        };
        let systems = app.systems.iter().map(|s| system(s)).collect::<Result<Vec<_>, _>>()?.join(", ");
        writeln!(
            apps,
            "SuiteApp {{ id: {:?}, name: {:?}, suite: Suite::{suite}, tagline: {:?}, color: {:?}, ink_light: {:?}, ink_dark: {:?}, repo: {:?}, bundle: {:?}, mac_bundle_name: {:?}, systems: &[{systems}] }},",
            app.id, app.name, app.tagline, app.color, app.ink_light, app.ink_dark, app.repo, app.bundle, app.mac_bundle_name
        )?;
        if let Some(d) = &app.details {
            write!(details, "AppDetails {{ id: {:?}, headline: {:?}, about: {:?}, features: &[", app.id, d.headline, d.about)?;
            for f in &d.features {
                write!(details, "feature({:?}, {:?}),", f.title, f.text)?;
            }
            write!(details, "], shots: &[")?;
            for s in &d.shots {
                write!(details, "shot({:?}, {:?}),", s.key, s.caption)?;
            }
            writeln!(details, "] }},")?;
        }
    }
    apps.push_str("];\n");
    details.push_str("];\n");
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR missing")?);
    fs::write(out.join("app_icons.rs"), app_icons(&manifest)?)?;
    fs::write(out.join("catalog.rs"), apps)?;
    fs::write(out.join("details.rs"), details)?;
    Ok(())
}
