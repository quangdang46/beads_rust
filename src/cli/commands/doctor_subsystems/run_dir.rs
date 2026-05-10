//! `.doctor/runs/<run-id>/` artifact directory (R-002).
//!
//! Every `br doctor --repair` run lays down an artifact directory:
//!
//! ```text
//! <repo>/.doctor/runs/<run-id>/
//!   actions.jsonl   # one line per mutate() call
//!   backups/        # verbatim pre-mutation copies
//!   report.json     # final report (written at end of run)
//!   undo.sh         # pure-bash fallback when br itself is broken
//! <repo>/.doctor/latest -> runs/<run-id>/   # atomic symlink
//! ```
//!
//! ## Run identifier
//!
//! `run_id` = `<UTC ISO 8601 seconds>__<short-hex>` where `short-hex` is
//! a SHA-256 truncation of `repo_root || iso8601_seconds || pid`. The
//! shape is human-sortable and unique-per-run.
//!
//! ## Escape hatch
//!
//! If `BR_DOCTOR_RUNS_DIR` is set in the environment, the run-dir is
//! placed under that path instead of `<repo>/.doctor/runs/`. This is
//! the documented escape hatch for read-only working trees and CI
//! sandboxes.
//!
//! ## Atomic symlink update
//!
//! `<repo>/.doctor/latest` is updated with a tmp-symlink + rename so
//! readers either see the previous run or the new run, never a torn
//! state.
//!
//! ## .gitignore
//!
//! On creation we ensure `.doctor/` is in `<repo>/.gitignore`. The
//! existing `.beads/` ignore patterns and conventions are not touched.

#![allow(dead_code)] // WP1 foundation; consumed by WP3-WP12.

use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::error::BeadsError;

/// Environment variable that overrides the `.doctor/runs/` location.
pub const ENV_RUNS_DIR: &str = "BR_DOCTOR_RUNS_DIR";

/// Concrete handles for the artifact directory of a single run.
#[derive(Debug, Clone)]
pub struct RunDir {
    /// Stable run identifier (ISO-8601 + short hash).
    pub run_id: String,
    /// `<runs_root>/<run-id>/`.
    pub root: PathBuf,
    /// `<root>/backups/`.
    pub backups: PathBuf,
    /// `<root>/actions.jsonl`.
    pub actions_file: PathBuf,
    /// `<root>/report.json`.
    pub report_file: PathBuf,
    /// `<root>/undo.sh` (only after [`write_undo_sh`] is called).
    pub undo_script: PathBuf,
    /// `<repo_root>/.doctor/latest` (or the `BR_DOCTOR_RUNS_DIR`
    /// equivalent). Symlink to `root`.
    pub latest_link: PathBuf,
}

/// Create a fresh run directory under `<repo>/.doctor/runs/` (or the
/// `BR_DOCTOR_RUNS_DIR` override).
///
/// On success:
/// - The run directory exists with `backups/`, `actions.jsonl`,
///   `report.json`.
/// - `<runs_root>/../latest` symlink points at the new run dir.
/// - `<repo>/.gitignore` contains `.doctor/` (added if missing).
///
/// # Errors
///
/// Returns [`BeadsError`] for I/O faults or for the case where
/// `repo_root` does not exist.
pub fn create_run_dir(repo_root: &Path) -> Result<RunDir, BeadsError> {
    if !repo_root.exists() {
        return Err(BeadsError::internal(format!(
            "doctor: repo_root {} does not exist",
            repo_root.display()
        )));
    }

    let runs_root = runs_root_for(repo_root);
    fs::create_dir_all(&runs_root).map_err(BeadsError::Io)?;

    let run_id = generate_run_id(repo_root);
    let root = runs_root.join(&run_id);
    let backups = root.join("backups");
    fs::create_dir_all(&backups).map_err(BeadsError::Io)?;

    let actions_file = root.join("actions.jsonl");
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&actions_file)
        .map_err(BeadsError::Io)?;

    let report_file = root.join("report.json");
    if !report_file.exists() {
        // Touch an empty placeholder so the file path is stable.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&report_file)
            .map_err(BeadsError::Io)?;
    }

    let undo_script = root.join("undo.sh");

    // .doctor/latest symlink (points relative inside the runs root so
    // it survives moves of the repo root).
    let latest_link = runs_root.parent().unwrap_or(&runs_root).join("latest");
    update_latest_symlink(&latest_link, &root)?;

    // Best-effort .gitignore update; not fatal if the repo has no
    // .gitignore (e.g., bare-bones test fixtures).
    let _ = ensure_doctor_in_gitignore(repo_root);

    Ok(RunDir {
        run_id,
        root,
        backups,
        actions_file,
        report_file,
        undo_script,
        latest_link,
    })
}

/// Resolve where `runs/` should live for a given repo, honoring the
/// `BR_DOCTOR_RUNS_DIR` env var.
fn runs_root_for(repo_root: &Path) -> PathBuf {
    runs_root_with_override(repo_root, std::env::var_os(ENV_RUNS_DIR).map(PathBuf::from))
}

/// Inner form of [`runs_root_for`] with an explicit override so tests
/// can exercise the redirect without mutating process-wide environment
/// state (the crate enforces `#![forbid(unsafe_code)]` so
/// `std::env::set_var` is unavailable).
fn runs_root_with_override(repo_root: &Path, env_override: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = env_override {
        return dir.join("runs");
    }
    repo_root.join(".doctor").join("runs")
}

fn generate_run_id(repo_root: &Path) -> String {
    let now = Utc::now();
    let iso = now.format("%Y%m%dT%H%M%SZ").to_string();
    let mut hasher = Sha256::new();
    hasher.update(repo_root.to_string_lossy().as_bytes());
    hasher.update(iso.as_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let mut short = String::with_capacity(6);
    for byte in hasher.finalize().iter().take(3) {
        write!(&mut short, "{byte:02x}").expect("writing to a String cannot fail");
    }
    format!("{iso}__{short}")
}

/// Atomically point `latest_link` at `target`. If the link already
/// exists, replace it via tmp-symlink + rename.
fn update_latest_symlink(latest_link: &Path, target: &Path) -> Result<(), BeadsError> {
    use std::os::unix::fs::symlink;
    if let Some(parent) = latest_link.parent() {
        fs::create_dir_all(parent).map_err(BeadsError::Io)?;
    }

    // Use a relative target so the link stays valid if `<repo>` is
    // moved. The symlink lives under `<runs_root>/..`, the target lives
    // at `<runs_root>/<run-id>/`, so `runs/<run-id>/` is the right
    // relative path.
    let rel_target = target
        .strip_prefix(latest_link.parent().unwrap_or(target))
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| target.to_path_buf());

    let tmp = latest_link.with_file_name(format!(
        ".latest.doctor-tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));

    // Clear stale tmp from a crashed run, if any.
    if tmp.symlink_metadata().is_ok() {
        fs::remove_file(&tmp).map_err(BeadsError::Io)?;
    }
    symlink(&rel_target, &tmp).map_err(BeadsError::Io)?;
    // `fs::rename` over an existing symlink atomically replaces it on
    // Unix.
    fs::rename(&tmp, latest_link).map_err(BeadsError::Io)?;
    Ok(())
}

/// Ensure `<repo>/.gitignore` contains a `.doctor/` ignore rule. Adds
/// it idempotently; never removes or rewrites unrelated entries.
fn ensure_doctor_in_gitignore(repo_root: &Path) -> Result<(), BeadsError> {
    let gitignore = repo_root.join(".gitignore");
    let needle = ".doctor/";
    let existing = match fs::read_to_string(&gitignore) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(BeadsError::Io(e)),
    };
    let already = existing.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == needle || trimmed == ".doctor" || trimmed == "/.doctor/"
    });
    if already {
        return Ok(());
    }
    let mut new_contents = existing;
    if !new_contents.is_empty() && !new_contents.ends_with('\n') {
        new_contents.push('\n');
    }
    new_contents.push_str("# br doctor per-run artifacts\n");
    new_contents.push_str(needle);
    new_contents.push('\n');

    // Atomic write: tmp + rename.
    let parent = gitignore.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(BeadsError::Io)?;
    tmp.write_all(new_contents.as_bytes())
        .map_err(BeadsError::Io)?;
    tmp.as_file().sync_data().map_err(BeadsError::Io)?;
    tmp.persist(&gitignore)
        .map_err(|e| BeadsError::Io(e.error))?;
    Ok(())
}

/// Write `<run-dir>/undo.sh` — a pure-bash fallback that reads
/// `actions.jsonl` in reverse and restores files from `backups/`.
///
/// The script is intentionally stand-alone (depends only on bash, jq,
/// cp, mv) so it is recoverable even when the `br` binary itself is
/// broken.
///
/// # Errors
///
/// Returns [`BeadsError::Io`] for write/permission failures.
pub fn write_undo_sh(run: &RunDir) -> Result<(), BeadsError> {
    let script = format!(
        r#"#!/usr/bin/env bash
# br doctor undo — pure-bash fallback for run {run_id}
#
# Replays {{actions_jsonl}} in reverse, restoring the verbatim backups
# under {{backups_dir}}. Requires: bash, jq, cp, mv.
#
# This script is generated by br; do NOT hand-edit unless the live br
# binary is broken.
set -euo pipefail

run_dir="$(cd "$(dirname "$0")" && pwd)"
actions="${{run_dir}}/actions.jsonl"
backups="${{run_dir}}/backups"
repo_root="$(cd "${{run_dir}}/../../.." && pwd)"

if [[ ! -s "${{actions}}" ]]; then
  echo "no actions.jsonl entries — nothing to undo" >&2
  exit 0
fi

# Reverse the actions and replay each one.
tac "${{actions}}" | while read -r line; do
  op=$(jq -r '.op' <<<"${{line}}")
  rel=$(jq -r '.path' <<<"${{line}}")
  rename_to=$(jq -r '.rename_to // empty' <<<"${{line}}")
  case "${{op}}" in
    write_file|append_file|chmod|symlink_atomic)
      # Restore from backup if one exists.
      backup="${{backups}}/${{rel}}"
      target="${{repo_root}}/${{rel}}"
      if [[ -e "${{backup}}" ]]; then
        mkdir -p "$(dirname "${{target}}")"
        cp -p "${{backup}}" "${{target}}"
      fi
      ;;
    rename)
      # Rename op moved <rel> -> <rename_to>; reverse it.
      if [[ -n "${{rename_to}}" && -e "${{rename_to}}" ]]; then
        mkdir -p "$(dirname "${{repo_root}}/${{rel}}")"
        mv "${{rename_to}}" "${{repo_root}}/${{rel}}"
      fi
      ;;
    db_exec|db_migrate)
      echo "[warn] cannot undo ${{op}} from bash — re-run br doctor undo" >&2
      ;;
    *)
      echo "[warn] unknown op ${{op}}; skipping" >&2
      ;;
  esac
done

echo "undo complete for run {run_id}" >&2
"#,
        run_id = run.run_id,
    );

    fs::write(&run.undo_script, script).map_err(BeadsError::Io)?;
    fs::set_permissions(&run.undo_script, fs::Permissions::from_mode(0o755))
        .map_err(BeadsError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn unique_temp_root(label: &str) -> tempfile::TempDir {
        let prefix = format!("br-doctor-rundir-{label}-");
        tempfile::Builder::new()
            .prefix(prefix.as_str())
            .tempdir()
            .expect("tempdir")
    }

    #[test]
    fn create_run_dir_produces_stable_run_id_format() {
        let tmp = unique_temp_root("stable");
        let run = create_run_dir(tmp.path()).expect("create_run_dir");

        // Format: <YYYYMMDDTHHMMSSZ>__<6 hex chars>
        let parts: Vec<&str> = run.run_id.split("__").collect();
        assert_eq!(parts.len(), 2, "run_id must split into ts__hash");
        assert_eq!(parts[0].len(), 16, "iso ts must be 16 chars");
        assert_eq!(parts[1].len(), 6, "short hash must be 6 hex");
        assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));

        // Directory layout exists.
        assert!(run.root.is_dir(), "root dir missing");
        assert!(run.backups.is_dir(), "backups dir missing");
        assert!(run.actions_file.is_file(), "actions.jsonl missing");
        assert!(run.report_file.is_file(), "report.json placeholder missing");

        // Symlink atomically updated. Note: the only env-controlled
        // path lives behind `runs_root_for`, which falls back to
        // `<repo>/.doctor/runs/`. If `BR_DOCTOR_RUNS_DIR` happens to be
        // set in the calling shell, the latest_link path will be under
        // that override, so we only assert that the symlink resolves
        // to a target containing run_id.
        let meta = fs::symlink_metadata(&run.latest_link).expect("latest");
        assert!(meta.file_type().is_symlink(), "latest must be symlink");
        let target = fs::read_link(&run.latest_link).unwrap();
        assert!(
            target.to_string_lossy().contains(&run.run_id),
            "symlink target {} must contain run_id {}",
            target.display(),
            run.run_id
        );

        // .gitignore now contains .doctor/.
        let gi = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".doctor/"));
    }

    #[test]
    fn second_run_replaces_latest_atomically() {
        let tmp = unique_temp_root("atomic");

        let run1 = create_run_dir(tmp.path()).expect("first run");
        // Sleep just over one second so the second run's iso-second
        // timestamp differs.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let run2 = create_run_dir(tmp.path()).expect("second run");
        assert_ne!(run1.run_id, run2.run_id);

        let target = fs::read_link(&run2.latest_link).unwrap();
        assert!(target.to_string_lossy().contains(&run2.run_id));
        // The first run's directory still exists — we don't delete it.
        assert!(run1.root.is_dir());
    }

    #[test]
    fn write_undo_sh_emits_executable_script() {
        let tmp = unique_temp_root("undo");

        let run = create_run_dir(tmp.path()).expect("create run");
        write_undo_sh(&run).expect("write undo");
        assert!(run.undo_script.is_file());
        let meta = fs::metadata(&run.undo_script).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
        let body = fs::read_to_string(&run.undo_script).unwrap();
        assert!(body.starts_with("#!/usr/bin/env bash"));
        assert!(body.contains(&run.run_id));
    }

    /// Verifies the env-override path without mutating process env
    /// (the crate forbids `unsafe`, so `std::env::set_var` is
    /// unavailable; we drive the inner pure helper directly).
    #[test]
    fn runs_root_with_override_redirects_runs_root() {
        let outer = unique_temp_root("envouter");
        let override_dir = unique_temp_root("envoverride");
        let computed =
            runs_root_with_override(outer.path(), Some(override_dir.path().to_path_buf()));
        assert!(computed.starts_with(override_dir.path()));
        assert!(computed.ends_with("runs"));

        // And without an override, falls back to <repo>/.doctor/runs.
        let fallback = runs_root_with_override(outer.path(), None);
        assert_eq!(fallback, outer.path().join(".doctor").join("runs"));
    }
}
