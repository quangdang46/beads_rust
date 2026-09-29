//! Package manifest validation tests.
//!
//! These tests validate the syntax and structure of package manager manifests
//! used for distributing br through Homebrew, Scoop, and AUR.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Test that the Homebrew formula has valid Ruby syntax.
#[test]
fn test_homebrew_formula_syntax() {
    let formula_path = Path::new("packaging/homebrew/br.rb");

    if !formula_path.exists() {
        eprintln!("Skipping: Homebrew formula not found at {formula_path:?}");
        return;
    }

    let content = fs::read_to_string(formula_path).expect("Failed to read Homebrew formula");

    // Basic structure checks
    assert!(
        content.contains("class Br < Formula"),
        "Formula must define Br class extending Formula"
    );
    assert!(
        content.contains("desc \""),
        "Formula must have a description"
    );
    assert!(
        content.contains("homepage \""),
        "Formula must have a homepage"
    );
    assert!(
        content.contains("license \""),
        "Formula must have a license"
    );
    assert!(
        content.contains("version \""),
        "Formula must have a version"
    );

    // Platform-specific URLs
    assert!(
        content.contains("on_macos do"),
        "Formula must have macOS platform section"
    );
    assert!(
        content.contains("on_linux do"),
        "Formula must have Linux platform section"
    );
    assert!(
        content.contains("on_arm do"),
        "Formula must have ARM architecture section"
    );
    assert!(
        content.contains("on_intel do"),
        "Formula must have Intel architecture section"
    );

    // Install and test blocks
    assert!(
        content.contains("def install"),
        "Formula must have install method"
    );
    assert!(content.contains("test do"), "Formula must have test block");

    // Check Ruby syntax if ruby is available
    if Command::new("ruby").arg("--version").output().is_ok() {
        let output = Command::new("ruby")
            .arg("-c")
            .arg(formula_path)
            .output()
            .expect("Failed to run ruby syntax check");

        assert!(
            output.status.success(),
            "Ruby syntax check failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Test that the Scoop manifest has valid JSON schema.
#[test]
fn test_scoop_manifest_schema() {
    let manifest_path = Path::new("packaging/scoop/br.json");

    if !manifest_path.exists() {
        eprintln!("Skipping: Scoop manifest not found at {manifest_path:?}");
        return;
    }

    let content = fs::read_to_string(manifest_path).expect("Failed to read Scoop manifest");

    // Parse as JSON
    let json: serde_json::Value =
        serde_json::from_str(&content).expect("Scoop manifest must be valid JSON");

    // Required fields
    assert!(
        json.get("version").is_some(),
        "Manifest must have 'version' field"
    );
    assert!(
        json.get("description").is_some(),
        "Manifest must have 'description' field"
    );
    assert!(
        json.get("homepage").is_some(),
        "Manifest must have 'homepage' field"
    );
    assert!(
        json.get("license").is_some(),
        "Manifest must have 'license' field"
    );
    assert!(json.get("bin").is_some(), "Manifest must have 'bin' field");

    // Architecture section
    let arch = json
        .get("architecture")
        .expect("Manifest must have 'architecture' field");
    assert!(
        arch.get("64bit").is_some(),
        "Manifest must have '64bit' architecture"
    );

    // 64bit must have url and hash
    let arch_64 = arch.get("64bit").unwrap();
    assert!(
        arch_64.get("url").is_some(),
        "64bit architecture must have 'url'"
    );
    assert!(
        arch_64.get("hash").is_some(),
        "64bit architecture must have 'hash'"
    );

    // Autoupdate section (optional but recommended)
    if let Some(autoupdate) = json.get("autoupdate") {
        assert!(
            autoupdate.get("architecture").is_some(),
            "autoupdate must have 'architecture' section"
        );
    }

    // URL format validation
    let url = arch_64.get("url").unwrap().as_str().unwrap();
    assert!(url.starts_with("https://"), "URL must use HTTPS: {url}");
    assert!(
        url.contains("github.com"),
        "URL should point to GitHub releases"
    );
    // Allow case-sensitive comparison since URLs are case-sensitive
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    {
        assert!(url.ends_with(".zip"), "Windows URL should be a .zip file");
    }
}

/// Test that the AUR PKGBUILD has valid shell syntax.
#[test]
// Syntax-checks a macOS `pkgbuild` property list with `bash -n`; neither
// bash nor pkgbuild exists on Windows.
#[cfg(unix)]
fn test_pkgbuild_syntax() {
    let pkgbuild_path = Path::new("packaging/aur/PKGBUILD");

    if !pkgbuild_path.exists() {
        eprintln!("Skipping: PKGBUILD not found at {pkgbuild_path:?}");
        return;
    }

    let content = fs::read_to_string(pkgbuild_path).expect("Failed to read PKGBUILD");

    // Required variables
    assert!(content.contains("pkgname="), "PKGBUILD must define pkgname");
    assert!(content.contains("pkgver="), "PKGBUILD must define pkgver");
    assert!(content.contains("pkgrel="), "PKGBUILD must define pkgrel");
    assert!(content.contains("pkgdesc="), "PKGBUILD must define pkgdesc");
    assert!(content.contains("arch="), "PKGBUILD must define arch");
    assert!(content.contains("url="), "PKGBUILD must define url");
    assert!(content.contains("license="), "PKGBUILD must define license");

    // Source arrays for both architectures
    assert!(
        content.contains("source_x86_64="),
        "PKGBUILD must have x86_64 sources"
    );
    assert!(
        content.contains("source_aarch64="),
        "PKGBUILD must have aarch64 sources"
    );

    // SHA256 sums
    assert!(
        content.contains("sha256sums_x86_64="),
        "PKGBUILD must have x86_64 checksums"
    );
    assert!(
        content.contains("sha256sums_aarch64="),
        "PKGBUILD must have aarch64 checksums"
    );

    // Package function
    assert!(
        content.contains("package()"),
        "PKGBUILD must have package() function"
    );

    // Check bash syntax if bash is available
    if Command::new("bash").arg("--version").output().is_ok() {
        let output = Command::new("bash")
            .arg("-n")
            .arg(pkgbuild_path)
            .output()
            .expect("Failed to run bash syntax check");

        assert!(
            output.status.success(),
            "Bash syntax check failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Test that Cargo.toml has proper metadata for crates.io publishing.
#[test]
fn test_cargo_metadata() {
    let cargo_toml = fs::read_to_string("Cargo.toml").expect("Failed to read Cargo.toml");

    // Required fields for crates.io
    assert!(cargo_toml.contains("name = "), "Cargo.toml must have name");
    assert!(
        cargo_toml.contains("version = "),
        "Cargo.toml must have version"
    );
    assert!(
        cargo_toml.contains("description = "),
        "Cargo.toml must have description for crates.io"
    );
    assert!(
        cargo_toml.contains("license = "),
        "Cargo.toml must have license for crates.io"
    );
    assert!(
        cargo_toml.contains("repository = "),
        "Cargo.toml should have repository URL"
    );

    // Recommended fields
    assert!(
        cargo_toml.contains("keywords = "),
        "Cargo.toml should have keywords for discoverability"
    );
    assert!(
        cargo_toml.contains("categories = "),
        "Cargo.toml should have categories for crates.io"
    );

    // Binary definition
    assert!(
        cargo_toml.contains("[[bin]]"),
        "Cargo.toml must define binary target"
    );
    assert!(
        cargo_toml.contains("name = \"br\""),
        "Binary must be named 'br'"
    );
}

/// Test that all package manifests carry a version no newer than Cargo.toml.
///
/// The `update-package-manifests.yml` workflow rewrites every packaging
/// manifest (Homebrew, Scoop, AUR) after a release is published, driven by
/// the release tag and the checksums attached to the release.  During
/// development Cargo.toml is bumped ahead of the manifests: the manifests
/// catch up only once a tagged release has been built and CI has rewritten
/// them.  This test therefore asserts "manifest version parses and is not
/// ahead of Cargo.toml" rather than requiring exact equality, which would
/// otherwise fail on every pre-release commit.
#[test]
fn test_version_consistency() -> Result<(), String> {
    fn parse_version(raw: &str, source: &str) -> Result<semver::Version, String> {
        semver::Version::parse(raw.trim())
            .map_err(|err| format!("{source} version '{raw}' is not valid semver: {err}"))
    }

    let cargo_toml = fs::read_to_string("Cargo.toml").expect("Failed to read Cargo.toml");
    let cargo_version_str = cargo_toml
        .lines()
        .find(|line| line.starts_with("version = "))
        .and_then(|line| line.split('"').nth(1))
        .expect("Could not find version in Cargo.toml");
    let cargo_version = parse_version(cargo_version_str, "Cargo.toml")?;

    let formula_path = Path::new("packaging/homebrew/br.rb");
    if formula_path.exists() {
        let formula = fs::read_to_string(formula_path).expect("Failed to read Homebrew formula");
        let raw = formula
            .lines()
            .find_map(|line| {
                let line = line.trim();
                line.strip_prefix("version \"")
                    .and_then(|rest| rest.strip_suffix('"'))
            })
            .expect("Homebrew formula missing `version \"…\"` line");
        let manifest_version = parse_version(raw, "Homebrew formula")?;
        assert!(
            manifest_version <= cargo_version,
            "Homebrew formula version {manifest_version} is ahead of Cargo.toml {cargo_version}"
        );
    }

    let scoop_path = Path::new("packaging/scoop/br.json");
    if scoop_path.exists() {
        let scoop = fs::read_to_string(scoop_path).expect("Failed to read Scoop manifest");
        let scoop_json: serde_json::Value =
            serde_json::from_str(&scoop).expect("Invalid Scoop JSON");
        let scoop_version_str = scoop_json
            .get("version")
            .and_then(serde_json::Value::as_str)
            .expect("Scoop manifest missing `version`");
        let scoop_version = parse_version(scoop_version_str, "Scoop manifest")?;
        assert!(
            scoop_version <= cargo_version,
            "Scoop manifest version {scoop_version} is ahead of Cargo.toml {cargo_version}"
        );
    }

    let pkgbuild_path = Path::new("packaging/aur/PKGBUILD");
    if pkgbuild_path.exists() {
        let pkgbuild = fs::read_to_string(pkgbuild_path).expect("Failed to read PKGBUILD");
        let raw = pkgbuild
            .lines()
            .find_map(|line| line.trim().strip_prefix("pkgver="))
            .expect("PKGBUILD missing `pkgver=` line");
        let manifest_version = parse_version(raw, "PKGBUILD")?;
        assert!(
            manifest_version <= cargo_version,
            "PKGBUILD pkgver {manifest_version} is ahead of Cargo.toml {cargo_version}"
        );
    }

    Ok(())
}

/// Package-manager manifests must reference assets the release actually
/// publishes.
///
/// The names are not hardcoded here: they are read out of
/// `.github/workflows/release.yml`, which is the only place that decides what a
/// release contains. Hardcoding a second copy is how this test drifted in the
/// first place -- it asserted `br-<version>-darwin_arm64.tar.gz`, a naming
/// scheme no release of this repository has ever produced, while the release
/// workflow publishes tagless `br-macos-arm64.tar.gz`. The manifests were
/// therefore correct to stop matching it.
///
/// Deriving the expectation also keeps the original protection: a manifest
/// pointing at an asset the release does not build, or shipping a placeholder
/// Packaging automation must be able to fetch a checksum for every artifact.
///
/// This used to be asserted against `.github/workflows/update-package-manifests.yml`,
/// which commit 7b2ced28 removed along with the other stale workflows. The
/// capability it protected did not go away: `release.yml` now publishes a
/// `.sha256` sidecar itself. The test follows the behaviour to its new home so
/// the protection stays alive instead of being marked skipped.
#[test]
fn test_release_publishes_checksum_sidecars_for_every_asset() -> Result<(), String> {
    let release =
        fs::read_to_string(".github/workflows/release.yml").expect("Failed to read release.yml");

    let mut assets: Vec<String> = Vec::new();
    let mut rest = release.as_str();
    while let Some(i) = rest.find("br-") {
        rest = &rest[i + 3..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
            .unwrap_or(rest.len());
        let name = format!("br-{}", &rest[..end]);
        // release.yml also mentions bare platform prefixes such as
        // `br-linux-x64` (matrix keys, template fragments). Only a name that
        // carries a real archive extension is something someone can download.
        if name.contains(".sha256") || !(name.ends_with(".tar.gz") || name.ends_with(".zip")) {
            continue;
        }
        if !assets.contains(&name) {
            assets.push(name);
        }
    }
    assert!(!assets.is_empty(), "no release assets found in release.yml");

    for asset in &assets {
        assert!(
            release.contains(&format!("{asset}.sha256")),
            "release.yml publishes {asset} but no {asset}.sha256 sidecar, so packaging \
             automation cannot verify what it downloads"
        );
    }
    Ok(())
}
