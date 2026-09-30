//! Regression coverage for high-risk release workflow shell fragments.
//!
//! Eight of the nine tests here drive `run_bash_step`, which shells out to
//! `bash -euo pipefail`. On windows-latest a bare `bash` resolves to
//! `C:\Windows\System32\bash.exe`, the WSL launcher, which with no WSL
//! distribution installed answers "Windows Subsystem for Linux has no installed
//! distributions." in UTF-16LE and exits non-zero. The fragments themselves only
//! ever run in release.yml's bash steps on ubuntu, so those tests are Unix-gated
//! rather than made to pass on a platform that will never execute them.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const RELEASE_WORKFLOW: &str = ".github/workflows/release.yml";
/// Platform tokens as the release workflow actually names its archives.
/// These are the tagless names `bd` and `install.sh` publish and request --
/// the Go-style `linux_amd64` / `darwin_arm64` tokens matched no asset the
/// release has ever produced.
const REQUIRED_PLATFORMS: &[&str] = &[
    "linux-x64",
    "linux-arm64",
    "macos-x64",
    "macos-arm64",
    "windows-x64",
];

#[derive(Debug, Deserialize)]
struct Workflow {
    jobs: BTreeMap<String, Job>,
}

#[derive(Debug, Deserialize)]
struct Job {
    steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
struct Step {
    name: Option<String>,
    run: Option<String>,
}

struct ShellOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

#[test]
fn release_workflow_exposes_expected_fragment_steps() -> Result<(), String> {
    for step_name in [
        "Validate reliability override",
        "Validate required artifacts present",
        "Generate combined checksums",
        "Verify all checksums",
        "Sign archive with Ed25519 (Linux/macOS)",
        "Generate changelog",
    ] {
        release_step_script(step_name)?;
    }

    Ok(())
}

#[test]
fn release_workflow_uses_tagless_asset_file_names() -> Result<(), String> {
    let workflow = read_to_string(Path::new(RELEASE_WORKFLOW))?;

    // The protection this test exists for: an asset name that embeds a version
    // breaks the moment the tag is moved, and a name built straight from
    // `github.ref_name` is the usual way that happens. `bd` and `install.sh`
    // both publish and request *tagless* archives -- `br-macos-arm64.tar.gz` --
    // so the workflow has to do the same.
    require_not_contains(&workflow, "br-${{ github.ref_name }}-${{ matrix.name }}")?;
    require_not_contains(&workflow, "artifacts/br-${{ github.ref_name }}-*")?;

    // Every published archive is `br-<platform>.<ext>` with no version segment,
    // and every reference to one goes through the `artifacts/` staging dir.
    let mut published = Vec::new();
    for line in workflow.lines() {
        let trimmed = line.trim();
        let marker = "tar czf artifacts/";
        let Some(idx) = trimmed.find(marker) else {
            continue;
        };
        let rest = &trimmed[idx + marker.len()..];
        if let Some(name) = rest.split_whitespace().next() {
            published.push(name.to_string());
        }
    }
    if published.is_empty() {
        return Err("release.yml publishes no archives into artifacts/".to_string());
    }
    for name in &published {
        if !name.starts_with("br-") {
            return Err(format!("unexpected archive name {name}"));
        }
        if name.contains("GITHUB_REF_NAME") || name.contains("VERSION") {
            return Err(format!(
                "archive {name} embeds a version; release assets must be tagless"
            ));
        }
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn reliability_override_fragment_requires_reason_and_records_summary() -> Result<(), String> {
    let script = release_step_script("Validate reliability override")?;
    let fixture = WorkflowFixture::new()?;
    let summary_path = fixture.root().join("summary.md");
    let summary_path_text = path_string(&summary_path);

    let missing_reason = run_bash_step(
        &script,
        fixture.root(),
        &[
            ("GITHUB_STEP_SUMMARY", summary_path_text.as_str()),
            ("RELIABILITY_OVERRIDE_REASON", ""),
        ],
    )?;
    require_failure(&missing_reason, "empty override reason should fail")?;
    require_contains(
        &missing_reason.stdout,
        "reliability_override_reason is required",
    )?;

    let accepted = run_bash_step(
        &script,
        fixture.root(),
        &[
            ("GITHUB_STEP_SUMMARY", summary_path_text.as_str()),
            (
                "RELIABILITY_OVERRIDE_REASON",
                "documented operator emergency",
            ),
        ],
    )?;
    require_success(&accepted)?;
    let summary = read_to_string(&summary_path)?;
    require_contains(&summary, "Reliability gates were explicitly skipped")?;
    require_contains(&summary, "documented operator emergency")
}

#[test]
#[cfg(unix)]
fn required_artifact_fragment_reports_missing_platforms() -> Result<(), String> {
    let script = release_step_script("Validate required artifacts present")?;
    let fixture = WorkflowFixture::new()?;
    fixture.create_artifacts_dir()?;
    for platform in REQUIRED_PLATFORMS {
        fixture.write_release_artifact(platform, b"binary")?;
    }

    let complete = run_bash_step(&script, fixture.root(), &[])?;
    require_success(&complete)?;
    require_contains(&complete.stdout, "All required platform artifacts present")?;

    let missing = WorkflowFixture::new()?;
    missing.create_artifacts_dir()?;
    for platform in REQUIRED_PLATFORMS
        .iter()
        .copied()
        .filter(|platform| *platform != "windows-x64")
    {
        missing.write_release_artifact(platform, b"binary")?;
    }

    let result = run_bash_step(&script, missing.root(), &[])?;
    require_failure(&result, "missing platform should fail")?;
    require_contains(&result.stdout, "br-windows-x64.zip")
}

#[test]
#[cfg(unix)]
fn combined_checksums_fragment_is_null_safe_and_replaces_existing_file() -> Result<(), String> {
    let script = release_step_script("Generate combined checksums")?;
    let fixture = WorkflowFixture::new()?;
    fixture.create_artifacts_dir()?;
    fixture.write_artifact("br-9.9.9-linux_amd64.tar.gz.sha256", b"linux\n")?;
    fixture.write_artifact("br-9.9.9-darwin amd64.tar.gz.sha256", b"darwin\n")?;
    fixture.write_artifact("--leading-name.sha256", b"leading\n")?;
    fixture.write_artifact("checksums.sha256", b"stale\n")?;

    let result = run_bash_step(&script, fixture.root(), &[])?;
    require_success(&result)?;
    let combined = fixture.read_artifact("checksums.sha256")?;
    require_contains(&combined, "linux")?;
    require_contains(&combined, "darwin")?;
    require_contains(&combined, "leading")?;
    require_not_contains(&combined, "stale")
}

#[test]
#[cfg(unix)]
fn verify_checksums_fragment_accepts_spaces_and_leading_dashes() -> Result<(), String> {
    let script = release_step_script("Verify all checksums")?;
    let fixture = WorkflowFixture::new()?;
    fixture.create_artifacts_dir()?;
    fixture.write_artifact_with_checksum("artifact with spaces.tar.gz", b"space-safe")?;
    fixture.write_artifact_with_checksum("--leading-artifact.tar.gz", b"dash-safe")?;
    fixture.write_artifact("checksums.sha256", b"combined file should be skipped\n")?;

    let result = run_bash_step(&script, fixture.root(), &[])?;
    require_success(&result)?;
    require_contains(&result.stdout, "artifact with spaces.tar.gz: OK")?;
    require_contains(&result.stdout, "--leading-artifact.tar.gz: OK")
}

#[test]
#[cfg(unix)]
fn verify_checksums_fragment_fails_on_corrupt_checksum() -> Result<(), String> {
    let script = release_step_script("Verify all checksums")?;
    let fixture = WorkflowFixture::new()?;
    fixture.create_artifacts_dir()?;
    fixture.write_artifact("br-9.9.9-linux_amd64.tar.gz", b"actual bytes")?;
    fixture.write_artifact(
        "br-9.9.9-linux_amd64.tar.gz.sha256",
        b"0000000000000000000000000000000000000000000000000000000000000000  br-9.9.9-linux_amd64.tar.gz\n",
    )?;

    let result = run_bash_step(&script, fixture.root(), &[])?;
    require_failure(&result, "corrupt checksum should fail release verification")
}

#[test]
fn signing_fragment_uses_private_ephemeral_key_file() -> Result<(), String> {
    let script = release_step_script("Sign archive with Ed25519 (Linux/macOS)")?;

    require_contains(&script, "mktemp")?;
    require_contains(&script, "RUNNER_TEMP")?;
    require_contains(&script, "chmod 600 \"$signing_key\"")?;
    require_contains(&script, "trap 'rm -f \"$signing_key\"' EXIT")?;
    require_contains(&script, "printf '%s\\n' \"$MINISIGN_SECRET_KEY\"")?;
    require_contains(&script, "-s \"$signing_key\"")?;
    require_not_contains(&script, "/tmp/minisign.key")?;
    require_not_contains(&script, "echo \"$MINISIGN_SECRET_KEY\"")
}

#[test]
#[cfg(unix)]
fn changelog_fragment_keeps_previous_tag_and_reliability_paths() -> Result<(), String> {
    let script = release_step_script("Generate changelog")?;

    require_contains(&script, "git describe --tags --abbrev=0 HEAD^")?;
    require_contains(&script, "No previous tag found")?;
    require_contains(&script, "HEAD~20..HEAD")?;
    require_contains(&script, "Reliability gates were explicitly skipped")?;
    require_contains(
        &script,
        "Release reliability gates completed before artifacts were built",
    )
}

fn release_step_script(step_name: &str) -> Result<String, String> {
    let raw = read_to_string(Path::new(RELEASE_WORKFLOW))?;
    let workflow: Workflow = serde_yml::from_str(&raw)
        .map_err(|error| format!("failed to parse {RELEASE_WORKFLOW}: {error}"))?;

    let Some(step) = workflow
        .jobs
        .values()
        .flat_map(|job| &job.steps)
        .find(|step| step.name.as_deref() == Some(step_name))
    else {
        return Err(format!("release workflow step not found: {step_name}"));
    };

    let Some(run) = step.run.as_deref() else {
        return Err(format!("step {step_name:?} has no run script"));
    };

    Ok(run.to_owned())
}

#[cfg(unix)]
fn run_bash_step(
    script: &str,
    working_dir: &Path,
    envs: &[(&str, &str)],
) -> Result<ShellOutput, String> {
    let mut command = Command::new("bash");
    command
        .arg("-euo")
        .arg("pipefail")
        .arg("-c")
        .arg(script)
        .current_dir(working_dir);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to run bash fragment: {error}"))?;

    Ok(ShellOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn require_success(output: &ShellOutput) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "fragment failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            output.stdout,
            output.stderr
        ))
    }
}

fn require_failure(output: &ShellOutput, context: &str) -> Result<(), String> {
    if output.status.success() {
        Err(format!(
            "{context}; fragment unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        ))
    } else {
        Ok(())
    }
}

fn require_contains(haystack: &str, needle: &str) -> Result<(), String> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(format!("expected to find {needle:?} in:\n{haystack}"))
    }
}

fn require_not_contains(haystack: &str, needle: &str) -> Result<(), String> {
    if haystack.contains(needle) {
        Err(format!("did not expect to find {needle:?} in:\n{haystack}"))
    } else {
        Ok(())
    }
}

fn read_to_string(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("failed to read {}: {error}", path.display()))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct WorkflowFixture {
    temp_dir: tempfile::TempDir,
}

impl WorkflowFixture {
    fn new() -> Result<Self, String> {
        Ok(Self {
            temp_dir: tempfile::TempDir::new()
                .map_err(|error| format!("failed to create temp fixture: {error}"))?,
        })
    }

    fn root(&self) -> &Path {
        self.temp_dir.path()
    }

    fn artifacts_dir(&self) -> PathBuf {
        self.root().join("artifacts")
    }

    fn create_artifacts_dir(&self) -> Result<(), String> {
        fs::create_dir_all(self.artifacts_dir())
            .map_err(|error| format!("failed to create artifacts fixture: {error}"))
    }

    fn write_artifact(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.artifacts_dir().join(name);
        fs::write(&path, bytes)
            .map_err(|error| format!("failed to write {}: {error}", path.display()))
    }

    fn write_release_artifact(&self, platform: &str, bytes: &[u8]) -> Result<(), String> {
        // Windows ships as a zip; every other platform ships a tar.gz. The
        // required-artifact gate checks the real published names.
        let mut name = String::from("br-");
        name.push_str(platform);
        if platform.starts_with("windows-") {
            name.push_str(".zip");
        } else {
            name.push_str(".tar.gz");
        }
        self.write_artifact(&name, bytes)
    }

    fn read_artifact(&self, name: &str) -> Result<String, String> {
        read_to_string(&self.artifacts_dir().join(name))
    }

    fn write_artifact_with_checksum(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.write_artifact(name, bytes)?;
        let digest = sha256_hex(bytes);
        self.write_artifact(
            &format!("{name}.sha256"),
            format!("{digest}  {name}\n").as_bytes(),
        )
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}
