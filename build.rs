//! Build script for `beads_rust`.
//!
//! Uses vergen-gix for stable build/rustc metadata and quiet git probes for
//! optional repository metadata.

use std::{env, process::Command};
use vergen_gix::{BuildBuilder, CargoBuilder, Emitter, RustcBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Warn if the `web` feature is enabled but the static assets haven't been
    // built. The build-web.sh script must be run before cargo build in CI.
    if std::env::var("CARGO_FEATURE_WEB").is_ok()
        && !std::path::Path::new("src/web/static/index.html").exists()
    {
        println!("cargo:warning=Web UI assets not found (src/web/static/ is empty or missing).");
        println!("cargo:warning=Run `bash scripts/build-web.sh` first, or omit `--features web`.");
        println!("cargo:warning=The `br web` command will not work without these assets.");
    }

    let build = BuildBuilder::default().build_timestamp(true).build()?;
    let cargo = CargoBuilder::default().target_triple(true).build()?;
    let rustc = RustcBuilder::default().semver(true).build()?;

    let mut emitter = Emitter::default();
    emitter
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&rustc)?;

    emitter.emit()?;
    emit_git_metadata();

    Ok(())
}

fn emit_git_metadata() {
    if git_output(&["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
        && let Some(sha) = git_output(&["rev-parse", "HEAD"])
    {
        emit_env("VERGEN_GIT_SHA", &sha);

        if let Some(branch) = git_output(&["rev-parse", "--abbrev-ref", "HEAD"]) {
            emit_env("VERGEN_GIT_BRANCH", &branch);
        }

        if let Some(timestamp) = git_output(&["log", "-1", "--format=%cI"]) {
            emit_env("VERGEN_GIT_COMMIT_TIMESTAMP", &timestamp);
        }

        if let Some(status) = git_output(&["status", "--porcelain"]) {
            emit_env(
                "VERGEN_GIT_DIRTY",
                if status.is_empty() { "false" } else { "true" },
            );
        }
        return;
    }

    if let Some(sha) = first_env(&[
        "VERGEN_GIT_SHA",
        "RCH_SOURCE_COMMIT",
        "RCH_GIT_SHA",
        "RCH_GIT_COMMIT",
        "GIT_COMMIT",
        "GITHUB_SHA",
        "CI_COMMIT_SHA",
        "BUILDKITE_COMMIT",
        "DRONE_COMMIT_SHA",
        "VERCEL_GIT_COMMIT_SHA",
    ]) {
        emit_env("VERGEN_GIT_SHA", &sha);
    }

    if let Some(branch) = first_env(&["VERGEN_GIT_BRANCH", "GITHUB_REF_NAME", "CI_COMMIT_REF_NAME"])
    {
        emit_env("VERGEN_GIT_BRANCH", &branch);
    }
}

fn emit_env(key: &str, value: &str) {
    println!("cargo:rustc-env={key}={value}");
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();

    Some(trimmed.to_string())
}

fn first_env(names: &[&str]) -> Option<String> {
    for name in names {
        println!("cargo:rerun-if-env-changed={name}");
        if let Ok(value) = env::var(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}
