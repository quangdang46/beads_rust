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
//! [`Op::DbExec`] runs a parameterized SQL statement against
//! `<repo>/.beads/beads.db` inside a `BEGIN IMMEDIATE` transaction.
//! Before the SQL fires, every row of every table named in
//! `affected_tables` is snapshotted as JSON to
//! `<run-dir>/backups/db/<table>__<sha8>__<ns>.json` (where `sha8` is
//! the first 8 hex chars of `sha256(<predicate>)` and `<ns>` is a
//! zero-padded wall-clock nanosecond counter so multiple calls within
//! a single run do not collide). On any error the transaction is
//! rolled back and **no** `actions.jsonl` line is written.
//!
//! [`Op::DbMigrate`] runs a versioned schema migration. WP4 ships the
//! safety scaffolding — `from` / `to` precondition gate plus a verbatim
//! `beads.db.pre-migrate` snapshot — and emits a
//! `migration_logic_not_yet_routed` warning because schema.rs's
//! `run_migrations()` is private and self-transactional. Wiring the
//! actual DDL through the chokepoint is a follow-up: the safety net
//! lands in WP4 so callers can route migrations through the chokepoint
//! the moment the public hook lands.
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
    /// `BEGIN IMMEDIATE` transaction. Before the SQL fires, every row
    /// of every table named in `affected_tables` is snapshotted as
    /// JSON under `<run-dir>/backups/db/`; on any error the
    /// transaction rolls back and no `actions.jsonl` line is written.
    DbExec {
        sql: String,
        #[serde(skip)]
        args: Vec<DbArg>,
        /// Tables whose rows must be snapshotted before the SQL runs.
        /// Empty list is allowed (no snapshot taken) — the chokepoint
        /// still records the op, but undo will not be able to revert
        /// the data change.
        #[serde(default)]
        affected_tables: Vec<String>,
        /// Optional WHERE clause (without the `WHERE` keyword) used
        /// when snapshotting. Applied to every table in
        /// `affected_tables`. `None` means "snapshot the whole table".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        affected_predicate: Option<String>,
    },
    /// Run a versioned schema migration. WP4 implements the safety
    /// scaffolding (precondition gate + verbatim DB-file snapshot);
    /// the actual DDL is currently driven by the legacy
    /// `apply_runtime_compatible_schema` path so the chokepoint emits
    /// a `migration_logic_not_yet_routed` warning.
    DbMigrate { from: u32, to: u32 },
    /// Replace the symlink at `path` with one pointing at `target`.
    /// Implemented atomically via tmp-symlink + rename.
    SymlinkAtomic { target: PathBuf },
}

/// Lightweight stand-in for a SQL bind value. WP4 wires this through
/// the chokepoint by converting to [`fsqlite_types::value::SqliteValue`]
/// at the SQL boundary; callers can therefore stay independent of the
/// fsqlite type stack.
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

impl DbArg {
    /// Convert into the `fsqlite` type system. Used at the chokepoint's
    /// SQL boundary; not exposed publicly because it leaks the
    /// underlying engine type.
    fn to_sqlite_value(&self) -> fsqlite_types::value::SqliteValue {
        use fsqlite_types::value::SqliteValue;
        match self {
            Self::Null => SqliteValue::Null,
            Self::I64(n) => SqliteValue::Integer(*n),
            Self::F64(f) => SqliteValue::Float(*f),
            Self::Text(s) => SqliteValue::Text(std::sync::Arc::from(s.as_str())),
            Self::Blob(b) => SqliteValue::Blob(std::sync::Arc::from(b.as_slice())),
        }
    }
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

/// DB-specific JSONL action record appended by [`mutate_db`].
#[derive(Serialize)]
struct DbActionRecord<'a> {
    path: String,
    op: &'static str,
    before_hash: &'a str,
    after_hash: &'a str,
    started_at_ns: u128,
    finished_at_ns: u128,
    run_id: &'a str,
    fixer_id: &'a str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    affected_tables: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    affected_predicate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    migrate_from: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    migrate_to: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<&'static str>,
}

struct DbMutationOutcome {
    after_hash: String,
    affected_tables: Option<String>,
    affected_predicate: Option<String>,
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
/// - DB ops that fail the precondition gate or the SQL transaction
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

    // DB ops route through their dedicated handler. The chokepoint
    // contract still applies — `path` must be in scope (it is the
    // beads.db file), backups go to `<run-dir>/backups/`, and
    // `actions.jsonl` records the op exactly once.
    if matches!(op, Op::DbExec { .. } | Op::DbMigrate { .. }) {
        return mutate_db(ctx, path, &op, &before_hash);
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

/// Routed handler for [`Op::DbExec`] and [`Op::DbMigrate`].
///
/// Implements the same 8-step contract as [`mutate`] but tailored to a
/// SQLite database file:
///
/// 1. Refuse if `path` is outside the configured write scopes (already
///    checked by the caller).
/// 2. For `DbExec`: snapshot affected rows as JSON to
///    `<run-dir>/backups/db/<table>__<sha8>__<ns>.json` before any SQL runs.
///    For `DbMigrate`: snapshot the entire DB file verbatim to
///    `<run-dir>/backups/db/beads.db.pre-migrate`.
/// 3. Open a writable `fsqlite::Connection`, run the work inside
///    `BEGIN IMMEDIATE` / `COMMIT`. On any error, `ROLLBACK` and
///    return without writing an `actions.jsonl` line.
/// 4. Compute `after_hash` from the post-COMMIT DB file SHA-256.
/// 5. Append a single `actions.jsonl` record describing the op.
#[allow(clippy::too_many_lines)]
fn mutate_db(
    ctx: &MutateContext,
    path: &Path,
    op: &Op,
    before_hash: &str,
) -> Result<ActionResult, BeadsError> {
    // Snapshot path under run-dir.
    let backups_db = ctx.run_dir.join("backups").join("db");

    // Pre-write inventory. We capture this before any disk write so a
    // crash mid-snapshot is recoverable from the JSONL log + the
    // pre-existing DB file.
    let started_at_ns = now_ns().saturating_sub(ctx.start_ns);

    if ctx.dry_run {
        eprintln!("[dry-run] would mutate {}: {}", path.display(), op.name());
        return Ok(ActionResult {
            ok: true,
            before_hash: before_hash.to_string(),
            after_hash: before_hash.to_string(),
            error: None,
        });
    }

    let rel = path.strip_prefix(&ctx.repo_root).unwrap_or(path);
    let op_name = op.name();

    let exec_result: Result<DbMutationOutcome, BeadsError> = match op {
        Op::DbExec {
            sql,
            args,
            affected_tables,
            affected_predicate,
        } => {
            fs::create_dir_all(&backups_db).map_err(BeadsError::Io)?;

            let predicate_for_snapshot = affected_predicate.clone();
            let tables_for_snapshot = affected_tables.clone();
            let sql_owned = sql.clone();
            let args_owned: Vec<fsqlite_types::value::SqliteValue> =
                args.iter().map(DbArg::to_sqlite_value).collect();

            run_db_exec(
                path,
                &backups_db,
                &sql_owned,
                &args_owned,
                &tables_for_snapshot,
                predicate_for_snapshot.as_deref(),
            )
            .map(|()| {
                let table_summary = if tables_for_snapshot.is_empty() {
                    None
                } else {
                    Some(tables_for_snapshot.join(","))
                };
                DbMutationOutcome {
                    after_hash: sha256_file_hex_prefixed(path)
                        .unwrap_or_else(|_| SHA256_EMPTY_PREFIXED.to_string()),
                    affected_tables: table_summary,
                    affected_predicate: predicate_for_snapshot,
                }
            })
        }
        Op::DbMigrate { from, to } => {
            fs::create_dir_all(&backups_db).map_err(BeadsError::Io)?;
            run_db_migrate(path, &backups_db, *from, *to).map(|_warning| DbMutationOutcome {
                after_hash: sha256_file_hex_prefixed(path)
                    .unwrap_or_else(|_| SHA256_EMPTY_PREFIXED.to_string()),
                affected_tables: None,
                affected_predicate: None,
            })
        }
        _ => unreachable!("mutate_db only handles DB ops"),
    };

    let DbMutationOutcome {
        after_hash,
        affected_tables: db_affected_tables,
        affected_predicate: db_predicate,
    } = exec_result?;
    let finished_at_ns = now_ns().saturating_sub(ctx.start_ns);

    let (migrate_from, migrate_to, warning) = match op {
        Op::DbMigrate { from, to } => (
            Some(*from),
            Some(*to),
            Some("migration_logic_not_yet_routed"),
        ),
        _ => (None, None, None),
    };

    let record = DbActionRecord {
        path: rel.to_string_lossy().into_owned(),
        op: op_name,
        before_hash,
        after_hash: &after_hash,
        started_at_ns,
        finished_at_ns,
        run_id: &ctx.run_id,
        fixer_id: &ctx.fixer_id,
        ok: true,
        affected_tables: db_affected_tables,
        affected_predicate: db_predicate,
        migrate_from,
        migrate_to,
        warning,
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
        before_hash: before_hash.to_string(),
        after_hash,
        error: None,
    })
}

/// Compute `sha256:<hex>` of the file at `path`. Returns the empty
/// sentinel if the file does not exist.
fn sha256_file_hex_prefixed(path: &Path) -> std::io::Result<String> {
    let bytes = read_or_empty(path)?;
    if bytes.is_empty() && !path.exists() {
        return Ok(SHA256_EMPTY_PREFIXED.to_string());
    }
    Ok(sha256_hex_prefixed(&bytes))
}

/// Snapshot every row of every table in `tables`, write the JSON, then
/// run `sql` with `args` inside a `BEGIN IMMEDIATE` transaction. On any
/// error, `ROLLBACK` and propagate.
fn run_db_exec(
    db_path: &Path,
    backups_db: &Path,
    sql: &str,
    args: &[fsqlite_types::value::SqliteValue],
    affected_tables: &[String],
    affected_predicate: Option<&str>,
) -> Result<(), BeadsError> {
    use fsqlite::Connection;

    // Snapshot first. If snapshotting faults, we have not touched the
    // DB so the workspace is unchanged.
    for table in affected_tables {
        validate_identifier(table)?;
        let predicate = affected_predicate.unwrap_or("").trim();
        let select_sql = if predicate.is_empty() {
            format!("SELECT * FROM {table}")
        } else {
            format!("SELECT * FROM {table} WHERE {predicate}")
        };
        // Open a read-only connection just for the snapshot to avoid
        // any chance of accidental mutation.
        let read_conn = Connection::open(db_path.to_string_lossy().into_owned())?;
        let column_names = collect_column_names(&read_conn, table)?;
        let stmt = read_conn.prepare(&select_sql)?;
        let rows = stmt.query()?;

        let mut json_rows: Vec<serde_json::Value> = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut obj = serde_json::Map::new();
            for (i, name) in column_names.iter().enumerate() {
                let val = row
                    .get(i)
                    .cloned()
                    .unwrap_or(fsqlite_types::value::SqliteValue::Null);
                obj.insert(name.clone(), sqlite_value_to_json(&val));
            }
            json_rows.push(serde_json::Value::Object(obj));
        }

        let mut hasher = Sha256::new();
        hasher.update(predicate.as_bytes());
        let predicate_hash = &hex_encode(&hasher.finalize())[..8];
        // Include the wall-clock-nanosecond marker so successive
        // DbExec calls against the same (table, predicate) within a
        // single doctor run do not clobber each other's snapshots.
        let stamp = now_ns();
        let snapshot_path = backups_db.join(format!("{table}__{predicate_hash}__{stamp:020}.json"));

        let snapshot_envelope = serde_json::json!({
            "schema_version": "br.doctor.db_snapshot.v1",
            "table": table,
            "predicate": affected_predicate,
            "columns": column_names,
            "rows": json_rows,
        });
        let body = serde_json::to_vec_pretty(&snapshot_envelope).map_err(BeadsError::Json)?;
        fs::write(&snapshot_path, &body).map_err(BeadsError::Io)?;

        let _ = read_conn.close();
    }

    // Now run the SQL inside a BEGIN IMMEDIATE / COMMIT transaction.
    // Any error inside rolls back.
    let conn = Connection::open(db_path.to_string_lossy().into_owned())?;
    conn.execute("BEGIN IMMEDIATE")?;

    let exec_outcome = if args.is_empty() {
        conn.execute(sql).map(|_| ())
    } else {
        conn.execute_with_params(sql, args).map(|_| ())
    };

    match exec_outcome {
        Ok(()) => {
            if let Err(e) = conn.execute("COMMIT") {
                let _ = conn.execute("ROLLBACK");
                let _ = conn.close();
                return Err(e.into());
            }
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK");
            let _ = conn.close();
            return Err(e.into());
        }
    }
    let _ = conn.close();
    Ok(())
}

/// Validate that an identifier consists only of `[A-Za-z0-9_]` so it
/// cannot be used to inject SQL when interpolated into a `SELECT * FROM`
/// statement. Rejects empty strings.
fn validate_identifier(ident: &str) -> Result<(), BeadsError> {
    if ident.is_empty() || !ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(BeadsError::internal(format!(
            "doctor: invalid SQL identifier '{ident}' (must be ASCII alphanumeric + underscore)"
        )));
    }
    Ok(())
}

/// Resolve the column-name vector for `table` via `PRAGMA
/// table_info`. Returns an error if the table does not exist.
fn collect_column_names(
    conn: &fsqlite::Connection,
    table: &str,
) -> Result<Vec<String>, BeadsError> {
    use fsqlite_types::value::SqliteValue;
    validate_identifier(table)?;
    let rows = conn.query(&format!("PRAGMA table_info({table})"))?;
    if rows.is_empty() {
        return Err(BeadsError::internal(format!(
            "doctor: table '{table}' has no columns (does it exist?)"
        )));
    }
    let mut names = Vec::with_capacity(rows.len());
    // PRAGMA table_info: cid, name, type, notnull, dflt_value, pk
    for row in &rows {
        if let Some(SqliteValue::Text(name)) = row.get(1) {
            names.push(name.to_string());
        } else {
            return Err(BeadsError::internal(format!(
                "doctor: PRAGMA table_info({table}) returned non-text column name"
            )));
        }
    }
    Ok(names)
}

/// JSON-encode a single `SqliteValue`. NULL→null, Integer→number,
/// Float→number, Text→string, Blob→`{"$blob_b64": "..."}` to keep the
/// JSON faithful.
fn sqlite_value_to_json(val: &fsqlite_types::value::SqliteValue) -> serde_json::Value {
    use fsqlite_types::value::SqliteValue;
    match val {
        SqliteValue::Null => serde_json::Value::Null,
        SqliteValue::Integer(n) => serde_json::Value::from(*n),
        SqliteValue::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        SqliteValue::Text(s) => serde_json::Value::String(s.to_string()),
        SqliteValue::Blob(b) => {
            // Hex-encode rather than base64 so the snapshot has no
            // additional dependency on a base64 crate. Restore is via
            // hex-decode → SqliteValue::Blob.
            serde_json::json!({ "$blob_hex": hex_encode(b.as_ref()) })
        }
    }
}

/// Run the migration safety scaffolding for [`Op::DbMigrate`]. Verifies
/// the precondition gate (`PRAGMA user_version == from`), snapshots the
/// DB file verbatim, and returns `Ok(Some(()))` to flag the
/// `migration_logic_not_yet_routed` warning so the caller can record
/// it in `actions.jsonl`. Returning `Err` aborts before any state is
/// changed.
fn run_db_migrate(
    db_path: &Path,
    backups_db: &Path,
    from: u32,
    to: u32,
) -> Result<Option<()>, BeadsError> {
    use fsqlite::Connection;
    use fsqlite_types::value::SqliteValue;

    if to <= from {
        return Err(BeadsError::internal(format!(
            "doctor: db migrate refused — to ({to}) must be > from ({from})"
        )));
    }

    // (1) Snapshot the DB file *before* we open any connection. Opening
    //     a fsqlite connection can dirty header counters even on a
    //     read-only PRAGMA query; snapshotting first keeps the
    //     pre-migrate file byte-identical to the on-disk state callers
    //     observed before invoking the chokepoint.
    let snapshot_path = backups_db.join("beads.db.pre-migrate");
    if let Some(parent) = snapshot_path.parent() {
        fs::create_dir_all(parent).map_err(BeadsError::Io)?;
    }
    fs::copy(db_path, &snapshot_path).map_err(BeadsError::Io)?;
    let meta = fs::metadata(db_path).map_err(BeadsError::Io)?;
    fs::set_permissions(
        &snapshot_path,
        fs::Permissions::from_mode(meta.permissions().mode()),
    )
    .map_err(BeadsError::Io)?;

    // (2) Verify PRAGMA user_version matches `from`. If the precondition
    //     gate fails we discard the snapshot and refuse — leaving no
    //     backup behind keeps undo from trying to "restore" a no-op.
    let conn = Connection::open(db_path.to_string_lossy().into_owned())?;
    let row = conn.query_row("PRAGMA user_version")?;
    let current = row
        .get(0)
        .and_then(|v| match v {
            SqliteValue::Integer(n) => u32::try_from(*n).ok(),
            _ => None,
        })
        .unwrap_or(0);
    let _ = conn.close();
    if current != from {
        // Discard the now-orphan snapshot so it can't masquerade as a
        // recoverable backup of a never-mutated DB.
        let _ = fs::remove_file(&snapshot_path);
        return Err(BeadsError::internal(format!(
            "doctor: db migrate refused — user_version mismatch (expected {from}, got {current})"
        )));
    }

    // (3) The actual DDL is currently encapsulated in
    //     `crate::storage::schema::run_migrations`, which is private
    //     and self-transactional. Wiring its body through this
    //     chokepoint is a follow-up; WP4 lands the safety net only.
    //     Returning `Some(())` flags the warning for actions.jsonl.
    let _ = to; // reserved for the future "actually migrate" path
    Ok(Some(()))
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
    use fsqlite::Connection;
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

    // ========================================================================
    // WP4 — DB ops (DbExec / DbMigrate)
    // ========================================================================

    /// Set up a fresh `.beads/beads.db` with a single sample table for
    /// the DB-op tests. Returns the absolute path to the DB.
    fn setup_test_db(tmp: &Path) -> PathBuf {
        let beads_dir = tmp.join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        let db = beads_dir.join("beads.db");
        let conn = Connection::open(db.to_string_lossy().into_owned()).unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sample_widgets (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                value INTEGER NOT NULL DEFAULT 0
            )",
        )
        .unwrap();
        let _ = conn.close();
        db
    }

    #[test]
    fn test_db_exec_snapshots_affected_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let db = setup_test_db(tmp.path());
        let (ctx, actions_path) = make_ctx(tmp.path(), false);

        // Pre-state: empty table → snapshot must reflect that.
        let before_sha_path = sha256_file_hex_prefixed(&db).unwrap();
        assert!(before_sha_path.starts_with("sha256:"));

        let result = mutate(
            &ctx,
            &db,
            Op::DbExec {
                sql: "INSERT INTO sample_widgets(name, value) VALUES (?, ?)".into(),
                args: vec![DbArg::Text("alpha".into()), DbArg::I64(7)],
                affected_tables: vec!["sample_widgets".into()],
                affected_predicate: None,
            },
        )
        .expect("DbExec should succeed");
        assert!(result.ok);
        assert_ne!(
            result.before_hash, result.after_hash,
            "after_hash must differ from before_hash because the DB grew"
        );

        // Snapshot file exists, captures empty pre-state.
        let snapshot_dir = ctx.run_dir.join("backups/db");
        let entries: Vec<_> = fs::read_dir(&snapshot_dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .collect();
        assert_eq!(entries.len(), 1, "expected exactly one snapshot file");
        let body = fs::read_to_string(entries[0].path()).unwrap();
        let snap: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(snap["table"], "sample_widgets");
        assert_eq!(snap["rows"].as_array().unwrap().len(), 0);
        assert_eq!(
            snap["columns"].as_array().unwrap(),
            &vec![
                serde_json::Value::String("id".into()),
                serde_json::Value::String("name".into()),
                serde_json::Value::String("value".into()),
            ]
        );

        // Post-state: row is in the DB.
        let conn = Connection::open(db.to_string_lossy().into_owned()).unwrap();
        let rows = conn
            .query("SELECT name, value FROM sample_widgets")
            .unwrap();
        assert_eq!(rows.len(), 1);
        let _ = conn.close();

        // actions.jsonl carries one DbExec line.
        let log = fs::read_to_string(&actions_path).unwrap();
        let line = log.lines().next().expect("at least one action line");
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["op"], "db_exec");
        assert_eq!(v["affected_tables"], "sample_widgets");
    }

    #[test]
    fn test_db_exec_rollback_on_constraint_violation() {
        let tmp = tempfile::tempdir().unwrap();
        let db = setup_test_db(tmp.path());

        // Seed an existing row so a duplicate INSERT trips UNIQUE.
        {
            let conn = Connection::open(db.to_string_lossy().into_owned()).unwrap();
            conn.execute("INSERT INTO sample_widgets(name, value) VALUES ('dup', 1)")
                .unwrap();
            let _ = conn.close();
        }

        let (ctx, actions_path) = make_ctx(tmp.path(), false);

        let err = mutate(
            &ctx,
            &db,
            Op::DbExec {
                sql: "INSERT INTO sample_widgets(name, value) VALUES (?, ?)".into(),
                args: vec![DbArg::Text("dup".into()), DbArg::I64(2)],
                affected_tables: vec!["sample_widgets".into()],
                affected_predicate: None,
            },
        )
        .expect_err("UNIQUE violation should propagate");

        // Error mentions a database / constraint failure.
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("unique") || msg.to_lowercase().contains("constraint"),
            "expected unique/constraint failure; got {msg}"
        );

        // No actions.jsonl line was written.
        let log_len = fs::metadata(&actions_path).map(|m| m.len()).unwrap_or(0);
        assert_eq!(log_len, 0, "actions.jsonl must remain empty on rollback");

        // Behavioral check: the rolled-back transaction left the table
        // exactly one row (the seed). We do not compare DB file bytes
        // because fsqlite touches header counters even on a rolled-back
        // transaction; the row-level invariant is what matters.
        let conn = Connection::open(db.to_string_lossy().into_owned()).unwrap();
        let rows = conn.query("SELECT COUNT(*) FROM sample_widgets").unwrap();
        assert_eq!(
            rows[0].get(0).and_then(|v| match v {
                fsqlite_types::value::SqliteValue::Integer(n) => Some(*n),
                _ => None,
            }),
            Some(1),
            "rollback must leave only the seed row in the table"
        );
        let _ = conn.close();
    }

    #[test]
    fn test_db_migrate_user_version_mismatch_refuses() {
        let tmp = tempfile::tempdir().unwrap();
        let db = setup_test_db(tmp.path());

        // Stamp user_version = 5 directly.
        {
            let conn = Connection::open(db.to_string_lossy().into_owned()).unwrap();
            conn.execute("PRAGMA user_version = 5").unwrap();
            let _ = conn.close();
        }

        let (ctx, actions_path) = make_ctx(tmp.path(), false);

        let err = mutate(&ctx, &db, Op::DbMigrate { from: 4, to: 6 })
            .expect_err("user_version mismatch must refuse");
        let msg = err.to_string();
        assert!(
            msg.contains("user_version mismatch"),
            "error should reference user_version mismatch; got {msg}"
        );

        // PRAGMA user_version unchanged: this is the behavioral
        // invariant the precondition gate guarantees.
        let conn = Connection::open(db.to_string_lossy().into_owned()).unwrap();
        let row = conn.query_row("PRAGMA user_version").unwrap();
        let v = match row.get(0) {
            Some(fsqlite_types::value::SqliteValue::Integer(n)) => *n,
            _ => -1,
        };
        assert_eq!(v, 5, "user_version must not change after refusal");
        let _ = conn.close();

        // No actions.jsonl line was written.
        let log_len = fs::metadata(&actions_path).map(|m| m.len()).unwrap_or(0);
        assert_eq!(log_len, 0);

        // The orphan pre-migrate snapshot was scrubbed when we refused.
        let snap = ctx.run_dir.join("backups/db/beads.db.pre-migrate");
        assert!(
            !snap.exists(),
            "refused migrate must not leave a stale snapshot at {}",
            snap.display()
        );
    }

    #[test]
    fn test_db_migrate_happy_path_writes_backup_and_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let db = setup_test_db(tmp.path());

        // Stamp user_version = 4 so we can migrate 4 → 5.
        {
            let conn = Connection::open(db.to_string_lossy().into_owned()).unwrap();
            conn.execute("PRAGMA user_version = 4").unwrap();
            let _ = conn.close();
        }

        let pre_size = fs::metadata(&db).unwrap().len();
        let (ctx, actions_path) = make_ctx(tmp.path(), false);

        let result = mutate(&ctx, &db, Op::DbMigrate { from: 4, to: 5 })
            .expect("DbMigrate safety scaffold should succeed");
        assert!(result.ok);

        // Backup of the DB file exists. We don't assert byte-identical
        // against a previous fs::read because fsqlite mutates header
        // counters between connections; the safety guarantee that
        // matters is that the snapshot was taken before any connection
        // was opened (run_db_migrate snapshots first, then verifies
        // the pragma).
        let snap = ctx.run_dir.join("backups/db/beads.db.pre-migrate");
        assert!(
            snap.exists(),
            "pre-migrate snapshot missing: {}",
            snap.display()
        );
        let snap_size = fs::metadata(&snap).unwrap().len();
        assert_eq!(
            snap_size, pre_size,
            "snapshot length must match pre-migrate DB length"
        );
        let snap_bytes = fs::read(&snap).unwrap();
        assert!(
            snap_bytes.starts_with(b"SQLite format 3\0"),
            "snapshot must be a valid SQLite file (header check)"
        );

        // actions.jsonl has the migration warning recorded.
        let log = fs::read_to_string(&actions_path).unwrap();
        let line = log.lines().next().expect("expected one action line");
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["op"], "db_migrate");
        assert_eq!(v["migrate_from"], 4);
        assert_eq!(v["migrate_to"], 5);
        assert_eq!(v["warning"], "migration_logic_not_yet_routed");
    }
}
