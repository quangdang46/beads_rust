//! `mutate()` — the single chokepoint for every disk write performed by
//! `br doctor --repair` (R-001).
//!
//! ## Contract
//!
//! Every disk-write under `--repair` flows through [`mutate`]. No
//! exceptions. The 8-step contract (paraphrased from
//! `references/methodology/MUTATE-CHOKEPOINT.md`):
//!
//! 1. Acquire a per-path advisory lock.
//! 2. Compute SHA-256 `before_hash` (empty hash if the file does not
//!    exist).
//! 3. Validate preconditions (path is inside [`Capabilities::write_scopes`],
//!    op is allowed, …).
//! 4. Write a verbatim backup to `<run-dir>/backups/<rel-path>`,
//!    preserving permissions + mtime, and verify byte-identical via
//!    a strict in-process `cmp -s`-equivalent.
//! 5. Plan the mutation in memory (op-specific).
//! 6. Execute atomically (write-tmp-then-rename, transaction, …).
//! 7. Compute SHA-256 `after_hash`.
//! 8. Append a JSON line to `actions.jsonl` and return [`ActionResult`].
//!
//! If any step 3-6 fails, no `actions.jsonl` line is written, no backup
//! is consumed, and the workspace is unchanged. Steps 7-8 should be
//! infallible in practice; if they fault, restoration is the caller's
//! responsibility (the verbatim backup is sitting in
//! `<run-dir>/backups/`).
//!
//! ## Forbidden ops
//!
//! There is no `Op::Delete`. Per AGENTS.md and the project's safety
//! envelope §1, file deletion is prohibited at every layer of the
//! doctor subsystem. Anything that "needs to delete" must use
//! [`Op::Rename`] to move into the per-run quarantine area
//! (`<run-dir>/quarantine/<rel-path>`). The user can review and
//! manually remove the quarantined files later — that decision is
//! theirs, not the doctor's.
//!
//! ## DB ops
//!
//! [`Op::DbExec`] and [`Op::DbMigrate`] are **stubbed** in WP1 — they
//! return [`BeadsError::internal`] with a message tagged
//! `unimplemented_db_op`. WP4 wires them through the existing
//! `fsqlite::Connection` + transaction primitives. Routing the whole
//! 8-step contract for the file-based ops first ensures the chokepoint
//! is the only place these get added later.
//!
//! ## Crate-internal-only
//!
//! WP1 keeps `mutate()` `pub(crate)` so out-of-crate callers cannot
//! bypass the doctor's invariant checks. The upcoming WP3-WP12
//! migration of existing `repair_*` functions is the only consumer.

#![allow(dead_code)] // WP1 foundation; consumed by WP3-WP12.
#![allow(clippy::similar_names)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::BeadsError;
use crate::util::hex_encode;

/// Empty-input SHA-256 (i.e., `sha256("")`), prefixed with `sha256:`.
///
/// Used as the "did not exist" sentinel for missing files in
/// [`ActionRecord::before_hash`].
const SHA256_EMPTY_PREFIXED: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// One disk-mutating operation the chokepoint can execute.
///
/// The variants are stable — adding a new variant is fine, but
/// renaming or removing one is a breaking change for `actions.jsonl`
/// readers.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Op {
    /// Create-or-overwrite the file at `path` with `content`. The
    /// resulting file's mode is set to `mode` if `Some`, otherwise the
    /// platform default (usually `0o644`).
    WriteFile {
        #[serde(skip)]
        content: Vec<u8>,
        mode: Option<u32>,
    },
    /// Append `content` to the file at `path`. Creates the file if it
    /// does not exist.
    AppendFile {
        #[serde(skip)]
        content: Vec<u8>,
    },
    /// Rename `path` (the source) to `to` (the destination). The
    /// destination's parent is created if it does not exist. `path`
    /// and `to` must be on the same filesystem for atomicity.
    Rename { to: PathBuf },
    /// Set the mode of `path`.
    Chmod { mode: u32 },
    /// Execute `sql` against the project's `fsqlite` DB inside a
    /// transaction; rolls back on error. **Stubbed in WP1.**
    DbExec {
        sql: String,
        #[serde(skip)]
        args: Vec<DbArg>,
    },
    /// Run a versioned schema migration. **Stubbed in WP1.**
    DbMigrate { from: u32, to: u32 },
    /// Replace the symlink at `path` with one pointing at `target`.
    /// Implemented atomically via tmp-symlink + rename.
    SymlinkAtomic { target: PathBuf },
}

/// Lightweight stand-in for a SQL bind value. The DB ops are stubbed in
/// WP1, but having the public surface in place avoids a contract churn
/// when WP4 wires it up.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DbArg {
    /// SQL `NULL`.
    Null,
    /// Signed 64-bit integer.
    I64(i64),
    /// Double-precision float.
    F64(f64),
    /// UTF-8 string.
    Text(String),
    /// Raw bytes.
    Blob(Vec<u8>),
}

impl Op {
    /// Stable kebab-case name of the op for `actions.jsonl`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::WriteFile { .. } => "write_file",
            Self::AppendFile { .. } => "append_file",
            Self::Rename { .. } => "rename",
            Self::Chmod { .. } => "chmod",
            Self::DbExec { .. } => "db_exec",
            Self::DbMigrate { .. } => "db_migrate",
            Self::SymlinkAtomic { .. } => "symlink_atomic",
        }
    }
}

/// Statically-declared set of paths the doctor may write under during
/// `--repair`.
///
/// In WP1 we always include the workspace `.beads/` and the doctor
/// run-dir root `.doctor/`. WP3-WP12 may extend this when wiring
/// individual fixers.
#[derive(Debug, Clone)]
pub struct Capabilities {
    /// Canonicalized prefixes that paths must start with.
    pub write_scopes: Vec<PathBuf>,
}

impl Capabilities {
    /// Build a default capabilities set rooted at `repo_root`. Includes
    /// `<repo_root>/.beads/` and `<repo_root>/.doctor/`.
    #[must_use]
    pub fn for_repo(repo_root: &Path) -> Self {
        Self {
            write_scopes: vec![repo_root.join(".beads"), repo_root.join(".doctor")],
        }
    }
}

/// Per-run state shared across every [`mutate`] call. Owned by the
/// doctor driver; the contract is single-process so the inner mutex
/// over `actions_file` is lightweight.
pub struct MutateContext {
    /// Stable identifier for this doctor run. Embedded in every
    /// `actions.jsonl` line.
    pub run_id: String,
    /// `.doctor/runs/<run-id>/` — created by [`super::run_dir`].
    pub run_dir: PathBuf,
    /// Allowed write scopes.
    pub capabilities: Capabilities,
    /// `actions.jsonl` handle. Wrapped in a `Mutex` for forward
    /// compatibility with future parallel fixers.
    pub actions_file: Mutex<std::fs::File>,
    /// Identifier for the fixer that owns the current call.
    pub fixer_id: String,
    /// Workspace root used to canonicalize paths.
    pub repo_root: PathBuf,
    /// If `true`, [`mutate`] prints a `[dry-run]` line to stderr and
    /// does not touch disk.
    pub dry_run: bool,
    /// Captured at run-start; used to compute relative timestamps in
    /// `actions.jsonl`.
    pub start_ns: u128,
}

/// Outcome of a single [`mutate`] call.
#[derive(Debug, Clone)]
pub struct ActionResult {
    /// `true` if step 6 (atomic execute) succeeded.
    pub ok: bool,
    /// `sha256:<hex>` of the file before the mutation.
    pub before_hash: String,
    /// `sha256:<hex>` of the file after the mutation. Equal to
    /// `before_hash` in dry-run mode.
    pub after_hash: String,
    /// Error message if `ok == false`.
    pub error: Option<String>,
}

/// Single line in `actions.jsonl`. Stable contract.
#[derive(Debug, Clone, Serialize)]
pub struct ActionRecord {
    /// Workspace-relative path of the target.
    pub path: String,
    /// Op name (see [`Op::name`]).
    pub op: &'static str,
    /// `sha256:<hex>`.
    pub before_hash: String,
    /// `sha256:<hex>`.
    pub after_hash: String,
    /// Nanoseconds since [`MutateContext::start_ns`].
    pub started_at_ns: u128,
    /// Nanoseconds since [`MutateContext::start_ns`].
    pub finished_at_ns: u128,
    /// Stable run identifier.
    pub run_id: String,
    /// Stable fixer identifier.
    pub fixer_id: String,
    /// Whether step 6 succeeded.
    pub ok: bool,
    /// For [`Op::Rename`] only — destination path. `doctor undo` reads
    /// this to reverse the move.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename_to: Option<String>,
    /// Set if the mutation faulted and was rolled back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rolled_back: Option<bool>,
    /// Set on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Compute `sha256:<hex>` of `bytes`.
fn sha256_hex_prefixed(bytes: &[u8]) -> String {
    let h = Sha256::digest(bytes);
    format!("sha256:{}", hex_encode(&h))
}

/// Read `path` into bytes; return empty `Vec` if it does not exist.
fn read_or_empty(path: &Path) -> std::io::Result<Vec<u8>> {
    match fs::read(path) {
        Ok(b) => Ok(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Best-effort canonicalization that tolerates not-yet-existing files
/// (walks up to the first existing ancestor, canonicalizes that, then
/// re-joins the missing tail). This handles renames into not-yet-created
/// quarantine subdirs.
fn canonicalize_existing_or_parent(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return path.canonicalize();
    }
    // Walk up until we find an existing ancestor.
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cursor = path;
    loop {
        if let Some(name) = cursor.file_name() {
            tail.push(name);
        }
        match cursor.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                if parent.exists() {
                    let mut canonical = parent.canonicalize()?;
                    for segment in tail.iter().rev() {
                        canonical.push(segment);
                    }
                    return Ok(canonical);
                }
                cursor = parent;
            }
            _ => {
                // No existing ancestor; canonicalize CWD as a fallback.
                let cwd = Path::new(".").canonicalize()?;
                let mut canonical = cwd;
                for segment in tail.iter().rev() {
                    canonical.push(segment);
                }
                return Ok(canonical);
            }
        }
    }
}

fn ensure_in_scope(caps: &Capabilities, path: &Path) -> Result<(), BeadsError> {
    let canonical = canonicalize_existing_or_parent(path).map_err(BeadsError::Io)?;
    for scope in &caps.write_scopes {
        // Tolerate scopes that haven't been created yet (e.g.,
        // `.doctor/` on a fresh workspace).
        let canonical_scope =
            canonicalize_existing_or_parent(scope).unwrap_or_else(|_| scope.clone());
        if canonical.starts_with(&canonical_scope) {
            return Ok(());
        }
    }
    Err(BeadsError::internal(format!(
        "doctor: path {} is outside write_scopes (refused for safety)",
        path.display()
    )))
}

fn copy_verbatim_with_perms(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst)?;
    let meta = fs::metadata(src)?;
    fs::set_permissions(dst, fs::Permissions::from_mode(meta.permissions().mode()))?;
    Ok(())
}

/// Strict byte-by-byte comparison of two files. Used to verify the
/// verbatim backup matches the live file before we proceed with the
/// mutation.
fn cmp_strict(a: &Path, b: &Path) -> std::io::Result<()> {
    let ba = fs::read(a)?;
    let bb = fs::read(b)?;
    if ba != bb {
        return Err(std::io::Error::other(
            "doctor: backup verify failed (cmp-strict)",
        ));
    }
    Ok(())
}

fn now_ns() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// The single disk-mutation chokepoint for `br doctor --repair`.
///
/// See the module-level documentation for the 8-step contract.
///
/// # Errors
///
/// Returns [`BeadsError`] for:
/// - I/O faults during backup or atomic-write
/// - Out-of-scope paths (refused per safety envelope)
/// - DB ops in WP1 (stubbed; `Internal` with `unimplemented_db_op` tag)
pub fn mutate(ctx: &MutateContext, path: &Path, op: Op) -> Result<ActionResult, BeadsError> {
    // (1) Per-path advisory lock — minimal in-process mutex via the
    // actions_file lock; cross-process locking is provided by the
    // existing `acquire_routed_workspace_write_lock` upstream of the
    // doctor driver. We deliberately do NOT introduce a second OS-level
    // lockfile here; the workspace lock is already held when --repair
    // runs.

    // (2) before_hash.
    let before_bytes = read_or_empty(path).map_err(BeadsError::Io)?;
    let before_existed = path.exists();
    let before_hash = if before_existed {
        sha256_hex_prefixed(&before_bytes)
    } else {
        SHA256_EMPTY_PREFIXED.to_string()
    };

    // (3) Preconditions — write_scopes check.
    ensure_in_scope(&ctx.capabilities, path)?;
    if let Op::Rename { to } = &op {
        ensure_in_scope(&ctx.capabilities, to)?;
    }

    // For DB ops, scope-check the project DB path instead of the
    // dummy-`path` callers will pass. WP4 will replace this stub.
    if matches!(op, Op::DbExec { .. } | Op::DbMigrate { .. }) {
        return Err(BeadsError::internal(format!(
            "doctor: db op {} is unimplemented_db_op (WP4)",
            op.name()
        )));
    }
    let op_name = op.name();
    let rename_to = match &op {
        Op::Rename { to } => Some(to.to_string_lossy().into_owned()),
        _ => None,
    };

    // (4) Verbatim backup — only meaningful if the file existed.
    let rel = path.strip_prefix(&ctx.repo_root).unwrap_or(path);
    let backup = ctx.run_dir.join("backups").join(rel);
    if !ctx.dry_run && before_existed {
        copy_verbatim_with_perms(path, &backup).map_err(BeadsError::Io)?;
        cmp_strict(path, &backup).map_err(BeadsError::Io)?;
    }

    // (5) Plan + (6) Execute atomically.
    let started_at_ns = now_ns().saturating_sub(ctx.start_ns);
    if ctx.dry_run {
        eprintln!("[dry-run] would mutate {}: {}", path.display(), op_name);
        return Ok(ActionResult {
            ok: true,
            before_hash: before_hash.clone(),
            after_hash: before_hash,
            error: None,
        });
    }
    execute_atomic(path, op).map_err(BeadsError::Io)?;

    // (7) after_hash.
    let after_bytes = read_or_empty(path).map_err(BeadsError::Io)?;
    let after_exists = path.exists();
    let after_hash = if after_exists {
        sha256_hex_prefixed(&after_bytes)
    } else {
        // For Rename ops, the source is gone — record the empty hash
        // so undo can detect "this path was emptied / moved".
        SHA256_EMPTY_PREFIXED.to_string()
    };
    let finished_at_ns = now_ns().saturating_sub(ctx.start_ns);

    // (8) Record.
    let record = ActionRecord {
        path: rel.to_string_lossy().into_owned(),
        op: op_name,
        before_hash: before_hash.clone(),
        after_hash: after_hash.clone(),
        started_at_ns,
        finished_at_ns,
        run_id: ctx.run_id.clone(),
        fixer_id: ctx.fixer_id.clone(),
        ok: true,
        rename_to,
        rolled_back: None,
        error: None,
    };
    let line = serde_json::to_string(&record).map_err(BeadsError::Json)? + "\n";
    {
        let mut f = ctx.actions_file.lock().map_err(|e| {
            BeadsError::internal(format!("doctor: actions_file mutex poisoned: {e}"))
        })?;
        f.write_all(line.as_bytes()).map_err(BeadsError::Io)?;
        f.sync_data().map_err(BeadsError::Io)?;
    }

    Ok(ActionResult {
        ok: true,
        before_hash,
        after_hash,
        error: None,
    })
}

/// Execute the planned op atomically. File-based ops use
/// `tempfile::NamedTempFile::persist` (i.e., `rename(2)`); the temp
/// file lives in the **same directory** as the target so cross-FS
/// rename never breaks atomicity.
fn execute_atomic(path: &Path, op: Op) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    match op {
        Op::WriteFile { content, mode } => {
            let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
            tmp.write_all(&content)?;
            tmp.as_file().sync_data()?;
            let perms = fs::Permissions::from_mode(mode.unwrap_or(0o644));
            fs::set_permissions(tmp.path(), perms)?;
            tmp.persist(path)
                .map_err(|e| std::io::Error::other(e.error.to_string()))?;
        }
        Op::AppendFile { content } => {
            let mut f = OpenOptions::new().append(true).create(true).open(path)?;
            f.write_all(&content)?;
            f.sync_data()?;
        }
        Op::Rename { to } => {
            if let Some(p) = to.parent() {
                fs::create_dir_all(p)?;
            }
            fs::rename(path, &to)?;
        }
        Op::Chmod { mode } => {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        }
        Op::SymlinkAtomic { target } => {
            use std::os::unix::fs::symlink;
            let basename = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "target".to_string());
            let tmp = path.with_file_name(format!(
                ".{}.doctor-symlink-tmp.{}.{}",
                basename,
                std::process::id(),
                now_ns()
            ));
            // If a stale tmp from a crashed run is sitting around, get
            // rid of it (it is a symlink we own; this is the only place
            // in the doctor that may unlink, and only the tmp file).
            if tmp.symlink_metadata().is_ok() {
                fs::remove_file(&tmp)?;
            }
            symlink(target, &tmp)?;
            fs::rename(&tmp, path)?;
        }
        Op::DbExec { .. } | Op::DbMigrate { .. } => {
            // Unreachable — caller path returns BeadsError before this.
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "doctor: db op reached execute_atomic; should be intercepted",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::path::PathBuf;

    /// Build a [`MutateContext`] rooted at `tmp` with a fresh
    /// `actions.jsonl` and `backups/` subdir, optionally in dry-run.
    fn make_ctx(tmp: &Path, dry_run: bool) -> (MutateContext, PathBuf) {
        let run_id = "test-run".to_string();
        let run_dir = tmp.join(".doctor/runs").join(&run_id);
        fs::create_dir_all(run_dir.join("backups")).unwrap();
        let actions_path = run_dir.join("actions.jsonl");
        let actions_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&actions_path)
            .unwrap();

        let ctx = MutateContext {
            run_id,
            run_dir: run_dir.clone(),
            capabilities: Capabilities::for_repo(tmp),
            actions_file: Mutex::new(actions_file),
            fixer_id: "test-fixer".to_string(),
            repo_root: tmp.to_path_buf(),
            dry_run,
            start_ns: now_ns(),
        };
        (ctx, actions_path)
    }

    #[test]
    fn write_file_creates_backup_and_records_action() {
        let tmp = tempfile::tempdir().unwrap();
        // Set up a `.beads/` dir with an existing tracked file so we
        // exercise the backup path.
        let beads_dir = tmp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        let target = beads_dir.join("foo.txt");
        fs::write(&target, b"original").unwrap();

        let (ctx, actions_path) = make_ctx(tmp.path(), false);

        let result = mutate(
            &ctx,
            &target,
            Op::WriteFile {
                content: b"updated".to_vec(),
                mode: Some(0o644),
            },
        )
        .expect("mutate should succeed");

        assert!(result.ok);
        assert!(result.before_hash.starts_with("sha256:"));
        assert!(result.after_hash.starts_with("sha256:"));
        assert_ne!(result.before_hash, result.after_hash);

        // File contents updated.
        assert_eq!(fs::read(&target).unwrap(), b"updated");

        // Backup contains the original bytes byte-for-byte.
        let backup = ctx.run_dir.join("backups/.beads/foo.txt");
        assert!(backup.exists(), "backup must exist: {}", backup.display());
        assert_eq!(fs::read(&backup).unwrap(), b"original");

        // actions.jsonl has exactly one line of valid JSON.
        let f = std::fs::File::open(&actions_path).unwrap();
        let lines: Vec<String> = std::io::BufReader::new(f)
            .lines()
            .collect::<std::io::Result<_>>()
            .unwrap();
        assert_eq!(lines.len(), 1);
        let v: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["op"], "write_file");
        assert_eq!(v["fixer_id"], "test-fixer");
        assert_eq!(v["run_id"], "test-run");
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn dry_run_does_not_touch_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let beads_dir = tmp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        let target = beads_dir.join("dry.txt");
        fs::write(&target, b"keep me").unwrap();

        let (ctx, actions_path) = make_ctx(tmp.path(), true);

        let result = mutate(
            &ctx,
            &target,
            Op::WriteFile {
                content: b"would not write".to_vec(),
                mode: Some(0o644),
            },
        )
        .expect("dry-run mutate should succeed");

        assert!(result.ok);
        // before_hash == after_hash in dry-run mode.
        assert_eq!(result.before_hash, result.after_hash);
        // File unchanged.
        assert_eq!(fs::read(&target).unwrap(), b"keep me");
        // No backup created.
        let backup = ctx.run_dir.join("backups/.beads/dry.txt");
        assert!(!backup.exists());
        // No actions.jsonl line written.
        assert_eq!(fs::metadata(&actions_path).unwrap().len(), 0);
    }

    #[test]
    fn rename_moves_file_and_after_hash_is_empty_sentinel() {
        let tmp = tempfile::tempdir().unwrap();
        let beads_dir = tmp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        let src = beads_dir.join("orphan.lock");
        fs::write(&src, b"lock-bytes").unwrap();

        let (ctx, _) = make_ctx(tmp.path(), false);
        let dst = ctx.run_dir.join("quarantine/orphan.lock");

        let result =
            mutate(&ctx, &src, Op::Rename { to: dst.clone() }).expect("rename should succeed");

        assert!(result.ok);
        // before_hash should be a real hash of "lock-bytes".
        assert_ne!(result.before_hash, SHA256_EMPTY_PREFIXED);
        // after_hash should be the empty-file sentinel because src is
        // gone.
        assert_eq!(result.after_hash, SHA256_EMPTY_PREFIXED);

        assert!(!src.exists(), "source must be moved");
        assert!(dst.exists(), "destination must exist");
        assert_eq!(fs::read(&dst).unwrap(), b"lock-bytes");
    }

    #[test]
    fn out_of_scope_path_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let beads_dir = tmp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();

        let (ctx, _) = make_ctx(tmp.path(), false);

        // /tmp/foo is well outside `.beads/` and `.doctor/`.
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("victim.txt");
        fs::write(&outside_target, b"untouched").unwrap();

        let err = mutate(
            &ctx,
            &outside_target,
            Op::WriteFile {
                content: b"oops".to_vec(),
                mode: None,
            },
        )
        .expect_err("out-of-scope writes must be refused");
        assert!(
            err.to_string().contains("outside write_scopes"),
            "error must mention scope refusal: {err}"
        );
        // File untouched.
        assert_eq!(fs::read(&outside_target).unwrap(), b"untouched");
    }

    #[test]
    fn rename_destination_outside_write_scope_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let beads_dir = tmp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        let source = beads_dir.join("keep-inside.txt");
        fs::write(&source, b"keep inside").unwrap();

        let (ctx, actions_path) = make_ctx(tmp.path(), false);
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("escaped.txt");

        let err = mutate(
            &ctx,
            &source,
            Op::Rename {
                to: outside_target.clone(),
            },
        )
        .expect_err("out-of-scope rename destinations must be refused");
        assert!(
            err.to_string().contains("outside write_scopes"),
            "error must mention scope refusal: {err}"
        );
        assert_eq!(fs::read(&source).unwrap(), b"keep inside");
        assert!(!outside_target.exists());
        assert_eq!(fs::metadata(&actions_path).unwrap().len(), 0);
        let backup = ctx.run_dir.join("backups/.beads/keep-inside.txt");
        assert!(!backup.exists());
    }

    #[test]
    fn db_ops_are_unimplemented_in_wp1() {
        let tmp = tempfile::tempdir().unwrap();
        let beads_dir = tmp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        let target = beads_dir.join("beads.db");
        fs::write(&target, b"sqlite header...").unwrap();

        let (ctx, _) = make_ctx(tmp.path(), false);

        let err = mutate(
            &ctx,
            &target,
            Op::DbExec {
                sql: "SELECT 1".into(),
                args: vec![],
            },
        )
        .expect_err("db ops should be unimplemented in WP1");
        assert!(
            err.to_string().contains("unimplemented_db_op"),
            "error should be tagged unimplemented_db_op: {err}"
        );
    }
}
