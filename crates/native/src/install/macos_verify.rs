//! Read-only checks on the private copy, before the installer swaps it into place.
//!
//! Nodalia's macOS apps are signed ad hoc: they carry no Apple Developer ID and are not
//! notarized, so there is no signing team to pin and Gatekeeper would not vouch for them. The
//! trust in a download is therefore built from what can be proven:
//!
//! 1. The disk image's SHA-256 matches the release's `SHA256SUMS.txt` (checked before the image is
//!    opened, over a connection to GitHub's own release pages).
//! 2. The app inside has an intact code signature, nested code included, so no file was changed
//!    after the release was signed.
//! 3. The app's signed identifier is the app's own bundle id from the catalog, never one the
//!    downloaded app supplies for itself.
//! 4. The app reports the release's version (checked by the caller).
//!
//! No Gatekeeper assessment is made: it would reject an ad-hoc signature by design.

use std::path::Path;
use std::time::Duration;

use nodalia_launcher_core::SuiteApp;

const CODESIGN: &str = "/usr/bin/codesign";
const TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn verify_with(
    app: &SuiteApp,
    bundle: &Path,
    mut run: impl FnMut(&str, &[&str], Duration) -> Result<crate::process::RunOutput, String>,
) -> Result<(), String> {
    let path = bundle.to_str().ok_or("couldn't check the app's unusual path, so it wasn't installed")?;
    let requirement = identity_requirement(app)?;
    for (check, args) in [
        // Do not apply the identifier requirement to the app's nested code.
        ("signature integrity", vec!["--verify", "--all-architectures", "--deep", "--strict", path]),
        ("app identity", vec!["--verify", "--all-architectures", "--strict", "-R", &requirement, path]),
    ] {
        let output = run(CODESIGN, &args, TIMEOUT)
            .map_err(|reason| format!("macOS verification did not succeed ({check}: {reason}), so the app wasn't installed"))?;
        if !output.success {
            let reason = output.stderr.trim();
            let reason = if reason.is_empty() { output.status } else { reason.to_string() };
            return Err(format!("macOS verification did not succeed ({check}: {reason}), so the app wasn't installed"));
        }
    }
    Ok(())
}

/// The code requirement pinning the signed identifier to the catalog's bundle id.
fn identity_requirement(app: &SuiteApp) -> Result<String, String> {
    // Catalog data only, never an identifier supplied by the downloaded app.
    let id = app.bundle;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')) {
        return Err("no valid macOS bundle identity is configured, so the app wasn't installed".into());
    }
    Ok(format!("=identifier \"{id}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lienzo() -> &'static SuiteApp {
        SuiteApp::by_id("lienzo").unwrap()
    }

    fn success(output: impl Into<String>) -> crate::process::RunOutput {
        crate::process::RunOutput::success(output)
    }

    #[test]
    fn mac_install_checks_are_bounded_and_only_pin_the_top_level_identity() {
        let app = lienzo();
        let mut calls = Vec::new();
        verify_with(app, Path::new("/private/owned/Lienzo.app"), |program, args, timeout| {
            assert_eq!(program, CODESIGN);
            assert_eq!(timeout, Duration::from_secs(60));
            calls.push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            Ok(success(String::new()))
        })
        .unwrap();
        assert_eq!(calls.len(), 2);
        let integrity = &calls[0];
        assert!(integrity.iter().any(|a| a == "--all-architectures"));
        assert!(integrity.iter().any(|a| a == "--deep"));
        assert!(integrity.iter().any(|a| a == "--strict"));
        assert!(!integrity.iter().any(|a| a == "-R"));
        let identity = &calls[1];
        assert!(!identity.iter().any(|a| a == "--deep"));
        assert!(identity.iter().any(|a| a == "--all-architectures"));
        assert!(identity.iter().any(|a| a == "=identifier \"dev.danielmiguelt.Lienzo\""));
    }

    #[test]
    fn mac_install_rejects_tampering_and_a_wrong_identity() {
        let app = lienzo();
        for (failed_call, reason) in [(0, "signature integrity failed"), (1, "identifier requirement failed")] {
            let mut calls = 0;
            let error = verify_with(app, Path::new("/private/owned/Lienzo.app"), |_, _, _| {
                let call = calls;
                calls += 1;
                if call == failed_call { Err(reason.into()) } else { Ok(success(String::new())) }
            })
            .unwrap_err();
            assert!(error.contains(reason), "{error}");
            assert!(error.contains("wasn't installed"), "{error}");
            assert_eq!(calls, failed_call + 1);
        }
        let error = verify_with(app, Path::new("/private/owned/Lienzo.app"), |_, _, _| {
            Ok(crate::process::RunOutput::exit(1, "", "a sealed resource is missing or invalid"))
        })
        .unwrap_err();
        assert!(error.contains("a sealed resource is missing or invalid"), "{error}");
    }

    #[test]
    fn mac_install_identity_comes_only_from_the_catalog() {
        assert_eq!(identity_requirement(lienzo()).unwrap(), "=identifier \"dev.danielmiguelt.Lienzo\"");
        let bad = SuiteApp { bundle: "io.bad\" or true", ..*lienzo() };
        assert!(identity_requirement(&bad).is_err());
        let empty = SuiteApp { bundle: "", ..*lienzo() };
        assert!(identity_requirement(&empty).is_err());
    }
}
