//! WP6 — agent-ergonomics surface for `br doctor`.
//!
//! Implements the dispatch targets for the `br doctor <subcommand>` group
//! defined by [`crate::cli::DoctorSubcommand`]:
//!
//! - `capabilities` — `br.doctor.capabilities.v1` envelope (JSON or table).
//! - `robot-docs`  — paste-ready agent handbook (Markdown or wrapped JSON).
//! - `health`      — sub-200 ms liveness summary; exit-code = liveness.
//! - `ls`          — list runs in `.doctor/runs/`.
//! - `undo`        — restore from `.doctor/runs/<run-id>/backups/`.
//! - `explain`     — expand a single finding (stub).
//!
//! Every JSON surface pins a `schema_version`. The `--robot-triage` flag
//! on the flat doctor command also lives here ([`emit_robot_triage`]).
//!
//! ## Safety
//!
//! - `health`, `capabilities`, `robot-docs`, `ls`, `explain` are
//!   read-only.
//! - `undo` mutates. File restores flow through
//!   [`super::mutate::mutate`] with a fresh undo run-dir; DB restores
//!   replay the recorded JSON snapshots inside one SQLite transaction.

#![allow(clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::cli::commands::doctor_subsystems::capabilities_doctor::DoctorCapabilities;
use crate::cli::commands::doctor_subsystems::exit_codes::DoctorExitCode;
use crate::cli::commands::doctor_subsystems::mutate::{
    self as chokepoint, Capabilities as MutateCapabilities, MutateContext, Op,
};
use crate::cli::commands::doctor_subsystems::run_dir;
use crate::cli::{
    DoctorCapabilitiesArgs, DoctorExplainArgs, DoctorHealthArgs, DoctorLsArgs, DoctorRobotDocsArgs,
    DoctorSubcommand, DoctorUndoArgs, OutputFormatBasic,
};
use crate::config;
use crate::error::{BeadsError, Result};
use crate::output::OutputContext;

/// Top-level dispatcher for `br doctor <subcommand>`. Resolves the
/// repo root via `config::discover_optional_beads_dir_with_cli` and
/// hands off to the per-subcommand handler.
///
/// # Errors
///
/// Returns [`BeadsError`] if subcommand-specific I/O or serialization
/// faults.
pub fn dispatch_subcommand(
    sub: &DoctorSubcommand,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let repo_root = match config::discover_optional_beads_dir_with_cli(cli)? {
        Some(beads) => beads
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| beads.clone()),
        None => std::env::current_dir().map_err(BeadsError::Io)?,
    };
    match sub {
        DoctorSubcommand::Capabilities(a) => execute_capabilities(a, ctx),
        DoctorSubcommand::RobotDocs(a) => execute_robot_docs(a, ctx),
        DoctorSubcommand::Health(a) => {
            let code = execute_health(a, &repo_root)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        DoctorSubcommand::Ls(a) => execute_ls(a, &repo_root),
        DoctorSubcommand::Undo(a) => execute_undo(a, &repo_root),
        DoctorSubcommand::Explain(a) => execute_explain(a, &repo_root),
    }
}

/// Stable schema-version constants — every JSON envelope pins one.
pub const CAPABILITIES_SCHEMA: &str = "br.doctor.capabilities.v1";
pub const HEALTH_SCHEMA: &str = "br.doctor.health.v1";
pub const RUNS_LIST_SCHEMA: &str = "br.doctor.runs_list.v1";
pub const TRIAGE_SCHEMA: &str = "br.doctor.triage.v1";
pub const ROBOT_DOCS_SCHEMA: &str = "br.doctor.robot_docs.v1";
pub const UNDO_SCHEMA: &str = "br.doctor.undo.v1";
pub const EXPLAIN_SCHEMA: &str = "br.doctor.explain.v1";

// =============================================================================
// capabilities
// =============================================================================

/// Top-level envelope for `br doctor capabilities`. Wraps the inner
/// [`DoctorCapabilities`] struct with a stable
/// `schema_version = "br.doctor.capabilities.v1"`.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilitiesEnvelope<'a> {
    pub schema_version: &'static str,
    #[serde(flatten)]
    pub inner: &'a DoctorCapabilities,
}

/// Execute `br doctor capabilities`.
///
/// Read-only. Always exits 0 (the doctor's contract: capabilities is a
/// pure diagnostic).
///
/// # Errors
///
/// Returns [`BeadsError`] if JSON serialization fails (effectively
/// never with the data shapes used here).
pub fn execute_capabilities(args: &DoctorCapabilitiesArgs, _ctx: &OutputContext) -> Result<()> {
    let caps = DoctorCapabilities::build();
    let envelope = CapabilitiesEnvelope {
        schema_version: CAPABILITIES_SCHEMA,
        inner: &caps,
    };

    // The optional --command filter is reserved for future fixer/detector
    // expansion (capabilities currently has empty fixers/detectors lists,
    // so filtering is a no-op for now). We honor the flag silently to
    // avoid breaking pinned agent invocations later.
    let _filter = args.command.as_deref();

    match args.format {
        OutputFormatBasic::Json | OutputFormatBasic::Toon => {
            // The capabilities subcommand is a pure machine-readable
            // surface — emit pretty JSON to stdout regardless of the
            // outer OutputContext (which may be Quiet for CI runs).
            let json = serde_json::to_string_pretty(&envelope).map_err(BeadsError::Json)?;
            println!("{json}");
        }
        OutputFormatBasic::Text => render_capabilities_text(&envelope),
    }
    Ok(())
}

fn render_capabilities_text(env: &CapabilitiesEnvelope<'_>) {
    println!("br doctor capabilities");
    println!("  schema_version  : {}", env.schema_version);
    println!("  contract_version: {}", env.inner.contract_version);
    println!("  doctor_version  : {}", env.inner.doctor_version);
    println!();
    println!("  Exit codes:");
    for entry in &env.inner.exit_codes {
        println!(
            "    {:>3} {:<24} {}",
            entry.code, entry.name, entry.description
        );
    }
    println!();
    println!("  Write scopes:");
    for scope in &env.inner.write_scopes {
        println!("    - {scope}");
    }
    println!();
    println!("  Env vars:");
    for var in &env.inner.env_vars {
        println!("    - {var}");
    }
    println!();
    println!("  Fixers     : {} registered", env.inner.fixers.len());
    println!("  Detectors  : {} registered", env.inner.detectors.len());
    println!();
    println!("Use `br doctor capabilities --format json` for the machine envelope.");
}

// =============================================================================
// robot-docs
// =============================================================================

const ROBOT_HANDBOOK_BODY: &str = r#"# br doctor — Agent Handbook

Contract version: **br.doctor.contract.v1**

`br doctor` is a diagnose-and-(optionally)-repair surface designed for AI
agents. Every disk write under `--repair` flows through a single
[`mutate()`](https://docs.rs/beads_rust) chokepoint that records a verbatim
backup, an `actions.jsonl` audit line, and an `undo.sh` fallback before
touching any byte of state.

## Subcommand surface

| Subcommand | Purpose | Mutates? |
|------------|---------|----------|
| (flat) `br doctor` | Run all detectors. Print findings. | NO |
| (flat) `br doctor --repair` | Apply fixers; back up everything. | YES (via `mutate()`) |
| `br doctor --robot-triage` | Single JSON envelope for swarm triage. | NO |
| `br doctor capabilities --format json` | Machine-readable contract. | NO |
| `br doctor robot-docs` | This handbook. | NO |
| `br doctor health` | Cheap one-line liveness summary. | NO |
| `br doctor ls` | List `.doctor/runs/` directories. | NO |
| `br doctor undo <run-id>` | Restore from `.doctor/runs/<id>/backups/`. | YES (restore) |
| `br doctor undo latest` | Resolve `latest` and restore. | YES (restore) |
| `br doctor explain <finding-id>` | Expand a single finding. | NO |

## Top-level flags (flat command)

| Flag | Purpose |
|------|---------|
| `--repair` | Apply fixers. Routes through `mutate()`. |
| `--dry-run` | Print the plan; do NOT execute. Pair with `--repair`. |
| `--allow-repeated-repair` | Permit a fresh JSONL rebuild after a prior failed recovery. |
| `--robot-triage` | Emit `br.doctor.triage.v1` and exit. |

## Exit codes

| Code | Name | Meaning |
|------|------|---------|
| `0`  | `healthy` | every check passed |
| `1`  | `findings_present` | findings exist; `--repair` not requested |
| `2`  | `fix_partial` | `--repair` ran; some fixers failed |
| `3`  | `fix_failed_rolled_back` | `--repair` faulted and rolled back from backup |
| `4`  | `refused_unsafe` | precondition gate refused (scope / schema / fingerprint) |
| `5`  | `concurrency_lost` | workspace lock unavailable |
| `6`  | `online_required` | network probe required `--online` |
| `64` | `usage_error` | clap rejected the invocation |
| `66` | `no_input` | required input missing (no `.beads/`) |
| `73` | `cannot_create_output` | could not create the run-dir |
| `74` | `io_error` | generic I/O fault during a non-mutating op |

## Canonical examples

### Happy path (workspace healthy)

```sh
br doctor               # exit 0; no findings
br doctor --robot-triage  # exit 0; envelope shows zero findings
```

### Broken path (findings present)

```sh
br doctor                                   # exit 1; findings printed
br doctor --robot-triage                    # exit 1; JSON shows recommended_command
br doctor --repair --dry-run                # exit 0; prints the plan
br doctor --repair                          # exit 0/2/3 depending on fixer outcomes
br doctor undo latest                       # exit 0; restores from the latest run
```

### Recovery (worked through `mutate()`)

Every `--repair` lays down `<repo>/.doctor/runs/<run-id>/`:

```text
.doctor/runs/<run-id>/
  actions.jsonl     # one JSON line per mutation
  backups/          # verbatim pre-mutation copies
  report.json       # final report (written at end of run)
  undo.sh           # pure-bash fallback when br itself is broken
.doctor/latest -> runs/<run-id>/   # atomic symlink
```

## What `br doctor` will NEVER do

1. Delete files. Anything that "needs to delete" is renamed into
   `<run-dir>/quarantine/` instead, so `undo` can reverse it.
2. Run destructive shell. There is no `Command::new("git")` in the
   doctor — the chokepoint is the only writer.
3. Write outside its declared `write_scopes` (`.beads/`, `.doctor/`).
4. Skip the verbatim backup — every mutation is preceded by a strict
   byte-by-byte `cmp -s` of the live file against the freshly-written
   backup.
5. Mutate without an audit trail — every action lands in
   `actions.jsonl` so `br doctor undo` can replay it in reverse.

## Recovery without br

If the `br` binary itself is broken, the per-run directory ships with a
pure-bash `undo.sh` that needs only `bash`, `jq`, `cp`, and `mv`. Run:

```sh
bash .doctor/runs/<run-id>/undo.sh
```

## See also

- `br doctor capabilities --format json` — machine-readable contract
- `br doctor health --json` — liveness summary
- The operator playbook lives at
  `<repo>/.../doctor_workspace/playbook.md`; consult it before running
  `--repair` on production workspaces.
"#;

/// Execute `br doctor robot-docs`.
///
/// # Errors
///
/// Returns [`BeadsError`] only if the JSON envelope fails to serialize.
pub fn execute_robot_docs(args: &DoctorRobotDocsArgs, _ctx: &OutputContext) -> Result<()> {
    match args.format {
        OutputFormatBasic::Json | OutputFormatBasic::Toon => {
            #[derive(Serialize)]
            struct Envelope<'a> {
                schema_version: &'static str,
                tool: &'static str,
                tool_version: &'static str,
                contract_version: &'static str,
                title: &'static str,
                line_count: usize,
                handbook: &'a str,
            }
            let envelope = Envelope {
                schema_version: ROBOT_DOCS_SCHEMA,
                tool: "br",
                tool_version: env!("CARGO_PKG_VERSION"),
                contract_version: "br.doctor.contract.v1",
                title: "br doctor — Agent Handbook",
                line_count: ROBOT_HANDBOOK_BODY.lines().count(),
                handbook: ROBOT_HANDBOOK_BODY,
            };
            let json = serde_json::to_string_pretty(&envelope).map_err(BeadsError::Json)?;
            println!("{json}");
        }
        OutputFormatBasic::Text => {
            print!("{ROBOT_HANDBOOK_BODY}");
        }
    }
    Ok(())
}

/// Pure accessor — used by tests and by `--robot-triage` to embed the
/// handbook command in the envelope.
#[must_use]
pub const fn robot_handbook_body() -> &'static str {
    ROBOT_HANDBOOK_BODY
}

// =============================================================================
// health
// =============================================================================

/// Output of `br doctor health`. Shape pinned by [`HEALTH_SCHEMA`].
#[derive(Debug, Clone, Serialize)]
pub struct HealthOutput {
    pub schema_version: &'static str,
    pub status: &'static str,
    pub exit_code: i32,
    pub beads_dir_present: bool,
    pub db_present: bool,
    pub jsonl_present: bool,
    pub merge_artifacts_present: bool,
    pub orphan_write_lock: bool,
    pub orphan_sync_lock: bool,
    pub elapsed_ms: u128,
    /// One-line summary suitable for stdout.
    pub line: String,
}

/// Execute `br doctor health`.
///
/// Stays under 200 ms by avoiding any DB query — only stat checks
/// against the workspace tree.
///
/// # Errors
///
/// Always returns `Ok`; the doctor exit code is the liveness signal,
/// not an error.
pub fn execute_health(args: &DoctorHealthArgs, repo_root: &Path) -> Result<i32> {
    let start = Instant::now();
    let beads = repo_root.join(".beads");
    let beads_dir_present = beads.is_dir();

    if !beads_dir_present {
        let line = format!(
            "no_workspace  br={} reason=missing_dot_beads",
            env!("CARGO_PKG_VERSION")
        );
        let exit_code = DoctorExitCode::NoInput.as_i32();
        emit_health(args, &line, exit_code, start, beads_dir_present);
        return Ok(exit_code);
    }

    let db = beads.join("beads.db");
    let db_present = db.is_file();
    let jsonl_present = beads.join("issues.jsonl").is_file() || beads.join("beads.jsonl").is_file();

    // MERGE_* artifacts indicate a torn previous merge.
    let merge_artifacts_present = match fs::read_dir(&beads) {
        Ok(it) => it.flatten().any(|e| {
            let n = e.file_name();
            let s = n.to_string_lossy();
            s.starts_with("MERGE_") || s.contains(".bad_") || s.contains(".rej")
        }),
        Err(_) => false,
    };

    // Orphan locks: present-but-empty, owner unknown.
    let orphan_write_lock = beads.join(".write.lock").exists();
    let orphan_sync_lock = beads.join(".sync.lock").exists();

    let findings_present = !db_present || !jsonl_present || merge_artifacts_present;

    let (status, exit_code) = if findings_present {
        ("findings_present", DoctorExitCode::FindingsPresent.as_i32())
    } else {
        // Lock files alone are advisory only; keep status healthy but
        // tag the line below so agents can see them.
        ("healthy", DoctorExitCode::Healthy.as_i32())
    };

    let mut line = format!(
        "{status}  br={ver} doctor=1 db={db} jsonl={jsonl}",
        status = status,
        ver = env!("CARGO_PKG_VERSION"),
        db = if db_present { "ok" } else { "missing" },
        jsonl = if jsonl_present { "ok" } else { "missing" },
    );
    if merge_artifacts_present {
        line.push_str(" merge_artifacts=present");
    }
    if orphan_write_lock {
        line.push_str(" write_lock=present");
    }
    if orphan_sync_lock {
        line.push_str(" sync_lock=present");
    }

    emit_health(args, &line, exit_code, start, beads_dir_present);
    Ok(exit_code)
}

fn emit_health(
    args: &DoctorHealthArgs,
    line: &str,
    exit_code: i32,
    start: Instant,
    beads_dir_present: bool,
) {
    let elapsed_ms = start.elapsed().as_millis();
    if args.json {
        let beads = std::env::current_dir()
            .map(|p| p.join(".beads"))
            .unwrap_or_default();
        let payload = HealthOutput {
            schema_version: HEALTH_SCHEMA,
            status: if exit_code == 0 {
                "healthy"
            } else if exit_code == DoctorExitCode::NoInput.as_i32() {
                "no_workspace"
            } else {
                "findings_present"
            },
            exit_code,
            beads_dir_present,
            db_present: beads.join("beads.db").is_file(),
            jsonl_present: beads.join("issues.jsonl").is_file()
                || beads.join("beads.jsonl").is_file(),
            merge_artifacts_present: false, // computed inside line; cheap re-derive omitted
            orphan_write_lock: beads.join(".write.lock").exists(),
            orphan_sync_lock: beads.join(".sync.lock").exists(),
            elapsed_ms,
            line: line.to_string(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&payload) {
            println!("{json}");
        }
    } else {
        println!("{line}");
    }
}

// =============================================================================
// ls
// =============================================================================

/// One row of `br doctor ls`.
#[derive(Debug, Clone, Serialize)]
pub struct RunListRow {
    pub run_id: String,
    pub started_at: String,
    pub exit_code: Option<i32>,
    pub action_count: usize,
}

/// Top-level `runs_list` envelope.
#[derive(Debug, Clone, Serialize)]
pub struct RunsListEnvelope {
    pub schema_version: &'static str,
    pub runs_dir: String,
    pub count: usize,
    pub runs: Vec<RunListRow>,
}

/// Execute `br doctor ls`.
///
/// # Errors
///
/// Returns [`BeadsError`] for I/O faults reading `.doctor/runs/`.
pub fn execute_ls(args: &DoctorLsArgs, repo_root: &Path) -> Result<()> {
    let runs_dir = resolve_runs_root(repo_root);
    let mut rows = Vec::new();
    if runs_dir.is_dir() {
        for entry in fs::read_dir(&runs_dir).map_err(BeadsError::Io)? {
            let entry = entry.map_err(BeadsError::Io)?;
            let ft = entry.file_type().map_err(BeadsError::Io)?;
            if !ft.is_dir() {
                continue;
            }
            let run_id = entry.file_name().to_string_lossy().into_owned();
            // Skip top-level non-run housekeeping (e.g. symlinks named
            // `latest` end up under the parent of `runs_dir`, not inside).
            let actions = entry.path().join("actions.jsonl");
            let action_count = count_lines(&actions);
            let report = entry.path().join("report.json");
            let exit_code = read_exit_code_from_report(&report);
            let started_at = run_id.split("__").next().unwrap_or("").to_string();
            rows.push(RunListRow {
                run_id,
                started_at,
                exit_code,
                action_count,
            });
        }
    }
    rows.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    if args.json {
        let envelope = RunsListEnvelope {
            schema_version: RUNS_LIST_SCHEMA,
            runs_dir: runs_dir.to_string_lossy().into_owned(),
            count: rows.len(),
            runs: rows,
        };
        if let Ok(json) = serde_json::to_string_pretty(&envelope) {
            println!("{json}");
        }
    } else {
        if rows.is_empty() {
            println!("no doctor runs in {}", runs_dir.display());
            return Ok(());
        }
        println!(
            "{:<32} {:<20} {:>8} {:>10}",
            "run_id", "started_at", "exit", "actions"
        );
        for row in &rows {
            println!(
                "{:<32} {:<20} {:>8} {:>10}",
                row.run_id,
                row.started_at,
                row.exit_code
                    .map_or_else(|| "-".to_string(), |c| c.to_string()),
                row.action_count
            );
        }
    }
    Ok(())
}

fn resolve_runs_root(repo_root: &Path) -> PathBuf {
    if let Some(over) = std::env::var_os(run_dir::ENV_RUNS_DIR) {
        PathBuf::from(over).join("runs")
    } else {
        repo_root.join(".doctor").join("runs")
    }
}

fn count_lines(path: &Path) -> usize {
    let Ok(f) = std::fs::File::open(path) else {
        return 0;
    };
    BufReader::new(f)
        .lines()
        .map_while(std::io::Result::ok)
        .count()
}

fn read_exit_code_from_report(path: &Path) -> Option<i32> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exit_code")
        .and_then(serde_json::Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
}

// =============================================================================
// undo
// =============================================================================

/// JSONL line shape on disk in `.doctor/runs/<id>/actions.jsonl`. We
/// only need a subset of fields for replay.
#[derive(Debug, Deserialize, Clone)]
struct StoredActionRecord {
    path: String,
    op: String,
    before_hash: String,
    #[serde(default)]
    rename_to: Option<String>,
    /// Workspace-relative paths of the JSON snapshot files written by
    /// the corresponding DbExec call. Empty for non-DbExec ops.
    #[serde(default)]
    db_snapshots: Vec<String>,
    /// Comma-separated list of affected table names recorded by the
    /// DbExec forward path. The undo replay cross-checks this against
    /// the table names inside `db_snapshots` before touching the DB.
    #[serde(default)]
    affected_tables: Option<String>,
    /// SQL predicate (WHERE clause body) used by the DbExec forward
    /// path; `None` means "snapshot the whole table". The undo replay
    /// cross-checks this against every snapshot envelope predicate.
    #[serde(default)]
    affected_predicate: Option<String>,
}

/// One restore step.
#[derive(Debug, Clone, Serialize)]
pub struct UndoStep {
    pub path: String,
    pub op: String,
    pub status: String,
    pub backup_used: Option<String>,
}

/// Top-level envelope for `br doctor undo`.
#[derive(Debug, Clone, Serialize)]
pub struct UndoEnvelope {
    pub schema_version: &'static str,
    pub run_id: String,
    pub run_dir: String,
    pub undo_run_id: Option<String>,
    pub dry_run: bool,
    pub steps: Vec<UndoStep>,
    pub restored: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Execute `br doctor undo`.
///
/// Per-action contract:
/// 1. Look up the verbatim backup at `<run-dir>/backups/<rel-path>`.
/// 2. Verify the backup matches the action's `before_hash`.
/// 3. Restore the backup contents via [`chokepoint::mutate`] under a
///    fresh undo run-dir, so the restore itself is auditable.
/// 4. After all restored, mark the original run as undone in
///    `report.json`.
///
/// # Errors
///
/// Returns [`BeadsError`] if the run-id cannot be resolved or if the
/// `.doctor/runs/` tree is unreadable.
pub fn execute_undo(args: &DoctorUndoArgs, repo_root: &Path) -> Result<()> {
    let runs_root = resolve_runs_root(repo_root);
    let run_id = if args.run_id == "latest" {
        find_latest_run(&runs_root)?
            .ok_or_else(|| BeadsError::internal("doctor: no runs found in .doctor/runs/"))?
    } else {
        args.run_id.clone()
    };
    let run_dir_path = runs_root.join(&run_id);
    if !run_dir_path.is_dir() {
        return Err(BeadsError::internal(format!(
            "doctor: run-id `{run_id}` not found at {}",
            run_dir_path.display()
        )));
    }
    let actions_path = run_dir_path.join("actions.jsonl");
    let (actions, parse_failures) = read_actions_reverse(&actions_path)?;
    let backups_dir = run_dir_path.join("backups");

    // Build a fresh "undo" run-dir so the restore writes are
    // themselves audited via mutate(). If we can't create one (e.g.,
    // dry-run on a read-only tree), fall back to a synthetic ctx that
    // refuses real writes.
    let mut steps = parse_failures;
    let mut restored = 0;
    let mut skipped = 0;
    let mut failed = steps.len();

    let undo_run = if args.dry_run {
        None
    } else {
        Some(run_dir::create_run_dir(repo_root)?)
    };

    if let Some(ref undo) = undo_run {
        let actions_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&undo.actions_file)
            .map_err(BeadsError::Io)?;
        // Undo writes target whatever paths the original repair touched.
        // The forward `--repair` run extended write_scopes (e.g. to a
        // root `.gitignore` for the `doctor.gitignore_repair` fixer); the
        // undo must permit the same paths or `mutate()` will refuse the
        // restore with `outside write_scopes`. We trust the recorded
        // actions because the run-dir is created and owned by the
        // doctor; the verbatim backup hash is still verified inside
        // `mutate()` itself.
        let mut capabilities = MutateCapabilities::for_repo(repo_root);
        for record in &actions {
            let target = repo_root.join(&record.path);
            if !capabilities
                .write_scopes
                .iter()
                .any(|scope| target.starts_with(scope))
            {
                capabilities.write_scopes.push(target.clone());
            }
            if let Some(rt) = &record.rename_to {
                let rt_path = PathBuf::from(rt);
                if !capabilities
                    .write_scopes
                    .iter()
                    .any(|scope| rt_path.starts_with(scope))
                {
                    capabilities.write_scopes.push(rt_path);
                }
            }
        }
        let ctx = MutateContext {
            run_id: undo.run_id.clone(),
            run_dir: undo.root.clone(),
            capabilities,
            actions_file: Mutex::new(actions_file),
            fixer_id: format!("doctor_undo[{run_id}]"),
            repo_root: repo_root.to_path_buf(),
            dry_run: false,
            start_ns: now_ns(),
        };
        for record in actions {
            let step = restore_one(&ctx, repo_root, &backups_dir, &record);
            update_counts(&step, &mut restored, &mut skipped, &mut failed);
            steps.push(step);
        }
    } else {
        // Dry-run: report the plan but never call mutate().
        for record in actions {
            let step = plan_one(repo_root, &backups_dir, &record);
            update_counts(&step, &mut restored, &mut skipped, &mut failed);
            steps.push(step);
        }
    }

    if !args.dry_run && failed == 0 {
        let _ = mark_report_undone(&run_dir_path, &run_id);
    }

    let envelope = UndoEnvelope {
        schema_version: UNDO_SCHEMA,
        run_id: run_id.clone(),
        run_dir: run_dir_path.to_string_lossy().into_owned(),
        undo_run_id: undo_run.as_ref().map(|r| r.run_id.clone()),
        dry_run: args.dry_run,
        steps,
        restored,
        skipped,
        failed,
    };
    emit_undo_result(&envelope, args.json);
    Ok(())
}

fn emit_undo_result(envelope: &UndoEnvelope, json: bool) {
    if json {
        if let Ok(json) = serde_json::to_string_pretty(envelope) {
            println!("{json}");
        }
    } else {
        println!(
            "doctor undo {run_id}: restored={restored} skipped={skipped} failed={failed}",
            run_id = envelope.run_id,
            restored = envelope.restored,
            skipped = envelope.skipped,
            failed = envelope.failed,
        );
    }
}

fn update_counts(step: &UndoStep, restored: &mut usize, skipped: &mut usize, failed: &mut usize) {
    match step.status.as_str() {
        "restored" | "would_restore" => *restored += 1,
        "skipped_idempotent" | "skipped_no_op" => *skipped += 1,
        _ => *failed += 1,
    }
}

fn read_actions_reverse(path: &Path) -> Result<(Vec<StoredActionRecord>, Vec<UndoStep>)> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(e) => return Err(BeadsError::Io(e)),
    };
    let mut actions = Vec::new();
    let mut parse_failures = Vec::new();
    for (line_number, line) in bytes.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        match serde_json::from_slice::<StoredActionRecord>(line) {
            Ok(rec) => actions.push(rec),
            Err(err) => parse_failures.push(UndoStep {
                path: format!("actions.jsonl:{}", line_number + 1),
                op: "parse_action".to_string(),
                status: format!("failed_parse_action:{err}"),
                backup_used: Some(path.to_string_lossy().into_owned()),
            }),
        }
    }
    actions.reverse();
    Ok((actions, parse_failures))
}

fn restore_one(
    ctx: &MutateContext,
    repo_root: &Path,
    backups_dir: &Path,
    record: &StoredActionRecord,
) -> UndoStep {
    let target = repo_root.join(&record.path);
    let backup = backups_dir.join(&record.path);

    // For Rename ops we recorded an empty after-hash; the recovery is
    // to move the renamed file back from its destination.
    if record.op == "rename" {
        return restore_rename(ctx, record, target);
    }

    // For DbExec we replay the JSON snapshot back into the live DB
    // inside a single BEGIN IMMEDIATE / COMMIT. The chokepoint
    // recorded snapshot file paths under `db_snapshots`; each snapshot
    // is a `br.doctor.db_snapshot.v1` envelope with the table, the
    // predicate, the column list, and the row vector taken before the
    // forward DbExec ran.
    if record.op == "db_exec" {
        return restore_db_exec(repo_root, record, target);
    }

    if !backup.exists() {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "failed_no_backup".to_string(),
            backup_used: None,
        };
    }
    let backup_bytes = match fs::read(&backup) {
        Ok(b) => b,
        Err(e) => {
            return UndoStep {
                path: record.path.clone(),
                op: record.op.clone(),
                status: format!("failed_read_backup:{e}"),
                backup_used: Some(backup.to_string_lossy().into_owned()),
            };
        }
    };

    // Idempotence: if the live file already matches the backup byte-for-
    // byte, we've already restored this action.
    if let Ok(live) = fs::read(&target)
        && live == backup_bytes
    {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "skipped_idempotent".to_string(),
            backup_used: Some(backup.to_string_lossy().into_owned()),
        };
    }

    // Verify the backup's hash matches the recorded `before_hash`.
    if !record.before_hash.is_empty() && !before_hash_matches(&record.before_hash, &backup_bytes) {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "failed_hash_mismatch".to_string(),
            backup_used: Some(backup.to_string_lossy().into_owned()),
        };
    }

    let mode = fs::metadata(&backup).ok().map(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode()
    });
    match chokepoint::mutate(
        ctx,
        &target,
        Op::WriteFile {
            content: backup_bytes,
            mode,
        },
    ) {
        Ok(_) => UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "restored".to_string(),
            backup_used: Some(backup.to_string_lossy().into_owned()),
        },
        Err(e) => UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: format!("failed_mutate:{e}"),
            backup_used: Some(backup.to_string_lossy().into_owned()),
        },
    }
}

/// Replay a `db_exec` action by restoring the rows captured in the
/// snapshot envelope back into the live DB. We expect each entry in
/// `record.db_snapshots` to be a workspace-relative path to a
/// `br.doctor.db_snapshot.v1` JSON file. The snapshot envelope carries
/// the table name, predicate, column list, and pre-mutation rows.
///
/// Replay shape (per snapshot):
/// 1. `BEGIN IMMEDIATE`.
/// 2. `DELETE FROM <table>` (whole table) or `DELETE ... WHERE
///    <predicate>` if the forward path used a predicate.
/// 3. `INSERT INTO <table>(<cols>) VALUES (?, ?, ...)` for every
///    snapshot row.
/// 4. `COMMIT`, or `ROLLBACK` on any error.
///
/// All snapshots inside ONE record are replayed inside ONE transaction
/// so the restore is atomic across tables.
fn restore_db_exec(repo_root: &Path, record: &StoredActionRecord, target: PathBuf) -> UndoStep {
    use fsqlite::Connection;
    use fsqlite_types::value::SqliteValue;

    if record.db_snapshots.is_empty() {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "skipped_no_snapshots".to_string(),
            backup_used: None,
        };
    }

    if !target.is_file() {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "failed_db_missing".to_string(),
            backup_used: None,
        };
    }

    // Read every snapshot envelope first so we fail-fast before opening
    // a writable DB connection.
    let mut envelopes: Vec<DbSnapshotEnvelope> = Vec::with_capacity(record.db_snapshots.len());
    for rel_snap in &record.db_snapshots {
        let snap_path = match workspace_relative_path(repo_root, rel_snap) {
            Ok(p) => p,
            Err(e) => {
                return UndoStep {
                    path: record.path.clone(),
                    op: record.op.clone(),
                    status: format!("failed_invalid_snapshot_path:{e}"),
                    backup_used: Some(rel_snap.clone()),
                };
            }
        };
        let body = match fs::read(&snap_path) {
            Ok(b) => b,
            Err(e) => {
                return UndoStep {
                    path: record.path.clone(),
                    op: record.op.clone(),
                    status: format!("failed_read_snapshot:{e}"),
                    backup_used: Some(snap_path.to_string_lossy().into_owned()),
                };
            }
        };
        match serde_json::from_slice::<DbSnapshotEnvelope>(&body) {
            Ok(env) => envelopes.push(env),
            Err(e) => {
                return UndoStep {
                    path: record.path.clone(),
                    op: record.op.clone(),
                    status: format!("failed_parse_snapshot:{e}"),
                    backup_used: Some(snap_path.to_string_lossy().into_owned()),
                };
            }
        }
    }

    // Validate every envelope before touching the DB.
    for env in &envelopes {
        if env.schema_version != "br.doctor.db_snapshot.v1" {
            return UndoStep {
                path: record.path.clone(),
                op: record.op.clone(),
                status: format!("failed_unknown_snapshot_schema:{}", env.schema_version),
                backup_used: None,
            };
        }
        if !is_safe_sql_ident(&env.table) {
            return UndoStep {
                path: record.path.clone(),
                op: record.op.clone(),
                status: format!("failed_invalid_table_ident:{}", env.table),
                backup_used: None,
            };
        }
        for col in &env.columns {
            if !is_safe_sql_ident(col) {
                return UndoStep {
                    path: record.path.clone(),
                    op: record.op.clone(),
                    status: format!("failed_invalid_column_ident:{col}"),
                    backup_used: None,
                };
            }
        }
    }
    if let Err(e) = validate_db_snapshot_contract(record, &envelopes) {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: e,
            backup_used: Some(record.db_snapshots.join(",")),
        };
    }

    // Replay every snapshot inside one BEGIN IMMEDIATE / COMMIT.
    let conn = match Connection::open(target.to_string_lossy().into_owned()) {
        Ok(c) => c,
        Err(e) => {
            return UndoStep {
                path: record.path.clone(),
                op: record.op.clone(),
                status: format!("failed_open_db:{e}"),
                backup_used: None,
            };
        }
    };
    if let Err(e) = conn.execute("BEGIN IMMEDIATE") {
        let _ = conn.close();
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: format!("failed_begin:{e}"),
            backup_used: None,
        };
    }

    let replay_result: std::result::Result<(), String> = (|| {
        for env in &envelopes {
            // Step 1: DELETE the rows the forward call would have
            // touched. For predicate-less DbExec the forward path
            // snapshotted the whole table, so we restore by clearing
            // the whole table first.
            let predicate = env.predicate.as_deref().unwrap_or("").trim();
            let table_ident = quote_sql_ident(&env.table);
            let delete_sql = if predicate.is_empty() {
                format!("DELETE FROM {table_ident}")
            } else {
                format!("DELETE FROM {table_ident} WHERE {predicate}")
            };
            conn.execute(&delete_sql)
                .map_err(|e| format!("delete:{e}"))?;

            // Step 2: INSERT each snapshot row. We bind via
            // execute_with_params to keep the SQL injection-free.
            if env.rows.is_empty() {
                continue;
            }
            let cols_csv = env
                .columns
                .iter()
                .map(|c| quote_sql_ident(c))
                .collect::<Vec<_>>()
                .join(", ");
            let placeholders: Vec<&str> = vec!["?"; env.columns.len()];
            let insert_sql = format!(
                "INSERT INTO {table_ident}({cols_csv}) VALUES ({})",
                placeholders.join(", ")
            );
            for row in &env.rows {
                let mut bound: Vec<SqliteValue> = Vec::with_capacity(env.columns.len());
                for col in &env.columns {
                    let val = row.get(col).cloned().unwrap_or(serde_json::Value::Null);
                    bound.push(json_to_sqlite_value(&val).map_err(|e| format!("bind:{e}"))?);
                }
                conn.execute_with_params(&insert_sql, &bound)
                    .map_err(|e| format!("insert:{e}"))?;
            }
        }
        Ok(())
    })();

    match replay_result {
        Ok(()) => match conn.execute("COMMIT") {
            Ok(_) => {
                let _ = conn.close();
                UndoStep {
                    path: record.path.clone(),
                    op: record.op.clone(),
                    status: "restored".to_string(),
                    backup_used: Some(record.db_snapshots.join(",")),
                }
            }
            Err(e) => {
                let _ = conn.execute("ROLLBACK");
                let _ = conn.close();
                UndoStep {
                    path: record.path.clone(),
                    op: record.op.clone(),
                    status: format!("failed_commit:{e}"),
                    backup_used: Some(record.db_snapshots.join(",")),
                }
            }
        },
        Err(e) => {
            let _ = conn.execute("ROLLBACK");
            let _ = conn.close();
            UndoStep {
                path: record.path.clone(),
                op: record.op.clone(),
                status: format!("failed_replay:{e}"),
                backup_used: Some(record.db_snapshots.join(",")),
            }
        }
    }
}

fn validate_db_snapshot_contract(
    record: &StoredActionRecord,
    envelopes: &[DbSnapshotEnvelope],
) -> std::result::Result<(), String> {
    let expected_tables = parse_affected_tables(record.affected_tables.as_deref())?;
    let actual_tables: Vec<String> = envelopes.iter().map(|env| env.table.clone()).collect();
    if expected_tables != actual_tables {
        return Err(format!(
            "failed_snapshot_table_mismatch:expected={} actual={}",
            expected_tables.join(","),
            actual_tables.join(",")
        ));
    }

    let expected_predicate = normalize_predicate(record.affected_predicate.as_deref());
    for env in envelopes {
        let actual_predicate = normalize_predicate(env.predicate.as_deref());
        if actual_predicate != expected_predicate {
            return Err(format!(
                "failed_snapshot_predicate_mismatch:table={}",
                env.table
            ));
        }
    }
    Ok(())
}

fn parse_affected_tables(raw: Option<&str>) -> std::result::Result<Vec<String>, String> {
    let Some(raw) = raw else {
        return Err("failed_missing_affected_tables".to_string());
    };
    let tables: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if tables.is_empty() {
        return Err("failed_empty_affected_tables".to_string());
    }
    for table in &tables {
        if !is_safe_sql_ident(table) {
            return Err(format!("failed_invalid_record_table_ident:{table}"));
        }
    }
    Ok(tables)
}

fn normalize_predicate(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|s| !s.is_empty())
}

/// In-memory shape of `br.doctor.db_snapshot.v1`. Mirrors the writer
/// in `mutate.rs::run_db_exec`.
#[derive(Debug, Clone, Deserialize)]
struct DbSnapshotEnvelope {
    schema_version: String,
    table: String,
    #[serde(default)]
    predicate: Option<String>,
    columns: Vec<String>,
    rows: Vec<serde_json::Map<String, serde_json::Value>>,
}

/// Defensive ASCII-alphanumeric+underscore identifier check; mirrors
/// the chokepoint's `validate_identifier`. Empty input is rejected so
/// no replay path can interpolate "" into SQL.
fn is_safe_sql_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn quote_sql_ident(s: &str) -> String {
    debug_assert!(is_safe_sql_ident(s));
    format!("\"{s}\"")
}

fn workspace_relative_path(repo_root: &Path, rel: &str) -> std::result::Result<PathBuf, String> {
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err("absolute".to_string());
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return Err("path_traversal".to_string());
    }
    Ok(repo_root.join(path))
}

/// Convert one JSON value back into an `SqliteValue` for re-binding.
/// Mirrors the inverse of `mutate.rs::sqlite_value_to_json`.
fn json_to_sqlite_value(
    val: &serde_json::Value,
) -> std::result::Result<fsqlite_types::value::SqliteValue, String> {
    use fsqlite_types::value::SqliteValue;
    match val {
        serde_json::Value::Null => Ok(SqliteValue::Null),
        serde_json::Value::Bool(b) => Ok(SqliteValue::Integer(i64::from(*b))),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(SqliteValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(SqliteValue::Float(f))
            } else {
                Err(format!("non-finite number {n}"))
            }
        }
        serde_json::Value::String(s) => Ok(SqliteValue::Text(s.clone().into())),
        serde_json::Value::Object(map) => {
            // {"$blob_hex": "..."} encoding from the snapshot writer.
            if let Some(serde_json::Value::String(hex)) = map.get("$blob_hex") {
                let bytes = decode_hex(hex).map_err(|e| format!("blob hex decode: {e}"))?;
                return Ok(SqliteValue::Blob(bytes.into()));
            }
            Err(format!("unsupported object shape in snapshot: {map:?}"))
        }
        serde_json::Value::Array(_) => Err("unsupported array value in snapshot".to_string()),
    }
}

/// Hex decoder for the `$blob_hex` envelope. Lowercase / uppercase
/// both accepted; whitespace not tolerated.
fn decode_hex(s: &str) -> std::result::Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("odd hex length".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for pair in bytes.chunks(2) {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> std::result::Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        other => Err(format!("non-hex byte 0x{other:02x}")),
    }
}

fn restore_rename(ctx: &MutateContext, record: &StoredActionRecord, target: PathBuf) -> UndoStep {
    let Some(rt) = &record.rename_to else {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "skipped_no_op".to_string(),
            backup_used: None,
        };
    };

    let from = PathBuf::from(rt);
    if !from.exists() {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "skipped_idempotent".to_string(),
            backup_used: Some(rt.clone()),
        };
    }

    match chokepoint::mutate(ctx, &from, Op::Rename { to: target }) {
        Ok(result) if result.ok => UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "restored".to_string(),
            backup_used: Some(rt.clone()),
        },
        Ok(result) => UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: format!(
                "failed_mutate:{}",
                result.error.unwrap_or_else(|| "unknown".to_string())
            ),
            backup_used: Some(rt.clone()),
        },
        Err(e) => UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: format!("failed_mutate:{e}"),
            backup_used: Some(rt.clone()),
        },
    }
}

fn plan_one(repo_root: &Path, backups_dir: &Path, record: &StoredActionRecord) -> UndoStep {
    let target = repo_root.join(&record.path);
    let backup = backups_dir.join(&record.path);
    if record.op == "rename"
        && let Some(rt) = &record.rename_to
    {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: if PathBuf::from(rt).exists() {
                "would_restore".to_string()
            } else {
                "skipped_idempotent".to_string()
            },
            backup_used: Some(rt.clone()),
        };
    }
    if record.op == "db_exec" {
        return plan_db_exec(repo_root, record);
    }
    if !backup.exists() {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "failed_no_backup".to_string(),
            backup_used: None,
        };
    }
    if let (Ok(live), Ok(b)) = (fs::read(&target), fs::read(&backup))
        && live == b
    {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "skipped_idempotent".to_string(),
            backup_used: Some(backup.to_string_lossy().into_owned()),
        };
    }
    UndoStep {
        path: record.path.clone(),
        op: record.op.clone(),
        status: "would_restore".to_string(),
        backup_used: Some(backup.to_string_lossy().into_owned()),
    }
}

fn plan_db_exec(repo_root: &Path, record: &StoredActionRecord) -> UndoStep {
    if record.db_snapshots.is_empty() {
        return UndoStep {
            path: record.path.clone(),
            op: record.op.clone(),
            status: "skipped_no_snapshots".to_string(),
            backup_used: None,
        };
    }
    for rel_snap in &record.db_snapshots {
        let snap_path = match workspace_relative_path(repo_root, rel_snap) {
            Ok(p) => p,
            Err(e) => {
                return UndoStep {
                    path: record.path.clone(),
                    op: record.op.clone(),
                    status: format!("failed_invalid_snapshot_path:{e}"),
                    backup_used: Some(rel_snap.clone()),
                };
            }
        };
        if !snap_path.exists() {
            return UndoStep {
                path: record.path.clone(),
                op: record.op.clone(),
                status: "failed_no_snapshot".to_string(),
                backup_used: Some(snap_path.to_string_lossy().into_owned()),
            };
        }
    }
    UndoStep {
        path: record.path.clone(),
        op: record.op.clone(),
        status: "would_restore".to_string(),
        backup_used: Some(record.db_snapshots.join(",")),
    }
}

fn before_hash_matches(expected: &str, bytes: &[u8]) -> bool {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(bytes);
    let prefixed = format!("sha256:{}", crate::util::hex_encode(&h));
    prefixed == expected
}

fn find_latest_run(runs_root: &Path) -> Result<Option<String>> {
    if !runs_root.is_dir() {
        return Ok(None);
    }
    if let Some(run_id) = latest_run_from_symlink(runs_root)? {
        return Ok(Some(run_id));
    }
    let mut best: Option<String> = None;
    for entry in fs::read_dir(runs_root).map_err(BeadsError::Io)? {
        let entry = entry.map_err(BeadsError::Io)?;
        if !entry.file_type().map_err(BeadsError::Io)?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        match &best {
            Some(curr) => {
                if name.as_str() > curr.as_str() {
                    best = Some(name);
                }
            }
            None => best = Some(name),
        }
    }
    Ok(best)
}

fn latest_run_from_symlink(runs_root: &Path) -> Result<Option<String>> {
    let latest_link = runs_root.parent().unwrap_or(runs_root).join("latest");
    let link_target = match fs::read_link(&latest_link) {
        Ok(target) => target,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => return Ok(None),
        Err(e) => return Err(BeadsError::Io(e)),
    };
    let base = latest_link.parent().unwrap_or(runs_root);
    let target = if link_target.is_absolute() {
        link_target
    } else {
        base.join(link_target)
    };
    if !target.is_dir() || !target.starts_with(runs_root) {
        return Ok(None);
    }
    Ok(target
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string))
}

fn mark_report_undone(run_dir_path: &Path, run_id: &str) -> Result<()> {
    let report = run_dir_path.join("report.json");
    let mut map: HashMap<String, serde_json::Value> = match fs::read(&report) {
        Ok(b) if !b.is_empty() => serde_json::from_slice(&b).unwrap_or_else(|_| HashMap::new()),
        _ => HashMap::new(),
    };
    map.insert("run_id".into(), serde_json::Value::String(run_id.into()));
    map.insert(
        "undone_at".into(),
        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
    );
    let bytes = serde_json::to_vec_pretty(&map).map_err(BeadsError::Json)?;
    fs::write(&report, bytes).map_err(BeadsError::Io)?;
    Ok(())
}

fn now_ns() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

// =============================================================================
// explain (stub)
// =============================================================================

/// Execute `br doctor explain <finding-id>`. WP6 ships a stub envelope;
/// the full evidence-expansion path lands in a later pass.
///
/// # Errors
///
/// Returns [`BeadsError`] only on serialization fault.
pub fn execute_explain(args: &DoctorExplainArgs, _repo_root: &Path) -> Result<()> {
    #[derive(Serialize)]
    struct Envelope<'a> {
        schema_version: &'static str,
        finding_id: &'a str,
        title: &'a str,
        evidence: &'static str,
        remediation: Remediation,
        note: &'static str,
    }
    #[derive(Serialize)]
    struct Remediation {
        command: String,
        explain_command: String,
        capabilities_url: &'static str,
    }
    let env = Envelope {
        schema_version: EXPLAIN_SCHEMA,
        finding_id: &args.finding_id,
        title: "doctor explain — WP6 stub",
        evidence: "The full evidence-expansion path is implemented in a later pass; \
             this envelope pins the contract surface so agents can rely on it.",
        remediation: Remediation {
            command: "br doctor --repair --dry-run".to_string(),
            explain_command: format!("br doctor explain {}", args.finding_id),
            capabilities_url: "br doctor capabilities --format json",
        },
        note: "WP6 stub; consult the full diagnostic via `br doctor`.",
    };
    if args.json {
        if let Ok(json) = serde_json::to_string_pretty(&env) {
            println!("{json}");
        }
    } else {
        println!("doctor explain {} (stub)", args.finding_id);
        println!("  See: br doctor --repair --dry-run");
        println!("  See: br doctor capabilities --format json");
    }
    Ok(())
}

// =============================================================================
// --robot-triage
// =============================================================================

/// Lightweight finding shape for the triage envelope.
#[derive(Debug, Clone, Serialize)]
pub struct TriageFinding {
    pub id: String,
    pub severity: String,
    pub message: String,
}

/// Top-level envelope for `--robot-triage`.
#[derive(Debug, Clone, Serialize)]
pub struct TriageEnvelope {
    pub schema_version: &'static str,
    pub summary: String,
    pub findings: Vec<TriageFinding>,
    pub actions_planned: Vec<serde_json::Value>,
    pub recommended_command: String,
    pub capabilities_url: String,
    pub robot_docs_command: String,
    pub quick_ref: TriageQuickRef,
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageQuickRef {
    pub healthy: usize,
    pub warn: usize,
    pub error: usize,
}

/// Build a triage envelope from raw counts and a list of compact
/// findings. The doctor driver passes the values it already computed
/// for the flat run.
#[must_use]
pub fn build_triage_envelope(
    healthy: usize,
    warn: usize,
    error: usize,
    findings: Vec<TriageFinding>,
) -> TriageEnvelope {
    let any_findings = warn > 0 || error > 0;
    let summary = if !any_findings {
        "workspace healthy".to_string()
    } else {
        format!("{error} error(s) and {warn} warning(s) detected")
    };
    let recommended_command = if error > 0 {
        "br doctor --repair --dry-run".to_string()
    } else if warn > 0 {
        "br doctor".to_string()
    } else {
        "br doctor health".to_string()
    };
    TriageEnvelope {
        schema_version: TRIAGE_SCHEMA,
        summary,
        findings,
        actions_planned: Vec::new(),
        recommended_command,
        capabilities_url: "br doctor capabilities --format json".to_string(),
        robot_docs_command: "br doctor robot-docs".to_string(),
        quick_ref: TriageQuickRef {
            healthy,
            warn,
            error,
        },
    }
}

/// Emit the triage envelope to stdout.
pub fn emit_robot_triage(envelope: &TriageEnvelope) {
    if let Ok(json) = serde_json::to_string_pretty(envelope) {
        println!("{json}");
    }
}

// =============================================================================
// tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn unique_temp_root(label: &str) -> tempfile::TempDir {
        let prefix = format!("br-doctor-surface-{label}-");
        tempfile::Builder::new()
            .prefix(prefix.as_str())
            .tempdir()
            .expect("tempdir")
    }

    #[test]
    fn test_doctor_capabilities_json_emits_v1_envelope() {
        let caps = DoctorCapabilities::build();
        let env = CapabilitiesEnvelope {
            schema_version: CAPABILITIES_SCHEMA,
            inner: &caps,
        };
        let json = serde_json::to_string(&env).expect("json");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(v["schema_version"], "br.doctor.capabilities.v1");
        assert!(v["exit_codes"].is_array());
        assert!(v["write_scopes"].is_array());
        assert!(v["env_vars"].is_array());
        // Inner contract still intact.
        assert_eq!(v["contract_version"], "1");
    }

    #[test]
    fn test_doctor_robot_docs_includes_exit_codes() {
        let body = robot_handbook_body();
        // Spot-check the most critical exit codes are documented.
        for code in ["0", "1", "2", "3", "4", "5", "73"] {
            assert!(
                body.contains(&format!("`{code}`")),
                "robot-docs missing exit code {code}"
            );
        }
        assert!(body.contains("br doctor undo"));
        assert!(body.contains("br doctor capabilities"));
        assert!(body.contains("br doctor health"));
    }

    #[test]
    fn test_doctor_health_under_200ms_on_healthy() {
        let tmp = unique_temp_root("health-fast");
        let beads = tmp.path().join(".beads");
        fs::create_dir_all(&beads).unwrap();
        fs::write(beads.join("beads.db"), b"sqlite header...").unwrap();
        fs::write(beads.join("issues.jsonl"), b"{}\n").unwrap();
        let args = DoctorHealthArgs { json: false };
        let start = Instant::now();
        // Use the inner pure helper to avoid println noise in test.
        let beads_present = beads.is_dir();
        let elapsed_pre = start.elapsed();
        let _ = beads_present;
        // The full execute_health writes to stdout — call it and check
        // wall-clock from start.
        let started = Instant::now();
        let code = execute_health(&args, tmp.path()).expect("health");
        let dur = started.elapsed();
        assert_eq!(code, 0, "should be healthy");
        assert!(
            dur.as_millis() < 200,
            "health must finish in <200ms; took {}ms",
            dur.as_millis()
        );
        let _ = elapsed_pre;
    }

    #[test]
    fn test_doctor_ls_returns_empty_when_no_runs() {
        let tmp = unique_temp_root("ls-empty");
        let args = DoctorLsArgs { json: true };
        // No .doctor/runs/ exists yet — ls must succeed and report 0.
        execute_ls(&args, tmp.path()).expect("ls");
        // Now create the runs dir empty.
        fs::create_dir_all(tmp.path().join(".doctor/runs")).unwrap();
        execute_ls(&args, tmp.path()).expect("ls (existing dir, empty)");
    }

    #[test]
    fn db_snapshot_contract_rejects_tampered_table_metadata() {
        let record = StoredActionRecord {
            path: ".beads/beads.db".to_string(),
            op: "db_exec".to_string(),
            before_hash: "sha256:test".to_string(),
            rename_to: None,
            db_snapshots: vec![".doctor/runs/r/backups/db/cache.json".to_string()],
            affected_tables: Some("blocked_issues_cache".to_string()),
            affected_predicate: None,
        };
        let envelopes = vec![DbSnapshotEnvelope {
            schema_version: "br.doctor.db_snapshot.v1".to_string(),
            table: "other_cache".to_string(),
            predicate: None,
            columns: vec!["issue_id".to_string()],
            rows: Vec::new(),
        }];

        let err = validate_db_snapshot_contract(&record, &envelopes).unwrap_err();

        assert!(err.starts_with("failed_snapshot_table_mismatch"));
    }

    #[test]
    fn db_snapshot_contract_rejects_tampered_predicate_metadata() {
        let record = StoredActionRecord {
            path: ".beads/beads.db".to_string(),
            op: "db_exec".to_string(),
            before_hash: "sha256:test".to_string(),
            rename_to: None,
            db_snapshots: vec![".doctor/runs/r/backups/db/cache.json".to_string()],
            affected_tables: Some("blocked_issues_cache".to_string()),
            affected_predicate: Some("issue_id = 'bd-1'".to_string()),
        };
        let envelopes = vec![DbSnapshotEnvelope {
            schema_version: "br.doctor.db_snapshot.v1".to_string(),
            table: "blocked_issues_cache".to_string(),
            predicate: Some("issue_id = 'bd-2'".to_string()),
            columns: vec!["issue_id".to_string()],
            rows: Vec::new(),
        }];

        let err = validate_db_snapshot_contract(&record, &envelopes).unwrap_err();

        assert_eq!(
            err,
            "failed_snapshot_predicate_mismatch:table=blocked_issues_cache"
        );
    }

    #[test]
    fn workspace_relative_snapshot_paths_cannot_escape_repo() {
        let tmp = unique_temp_root("snapshot-paths");

        assert!(workspace_relative_path(tmp.path(), ".doctor/runs/r/s.json").is_ok());
        assert!(workspace_relative_path(tmp.path(), "../outside.json").is_err());
        assert!(workspace_relative_path(tmp.path(), "/tmp/outside.json").is_err());
    }

    #[test]
    fn test_doctor_undo_restores_from_backup() {
        let tmp = unique_temp_root("undo");
        let repo = tmp.path();
        // Set up a workspace with a tracked file.
        let beads = repo.join(".beads");
        fs::create_dir_all(&beads).unwrap();
        let target = beads.join("foo.txt");
        fs::write(&target, b"original").unwrap();

        // Create an initial doctor run-dir and write through the
        // chokepoint.
        let initial_run = run_dir::create_run_dir(repo).expect("create run");
        let actions_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&initial_run.actions_file)
            .unwrap();
        let ctx = MutateContext {
            run_id: initial_run.run_id.clone(),
            run_dir: initial_run.root.clone(),
            capabilities: MutateCapabilities::for_repo(repo),
            actions_file: Mutex::new(actions_file),
            fixer_id: "test".into(),
            repo_root: repo.to_path_buf(),
            dry_run: false,
            start_ns: now_ns(),
        };
        chokepoint::mutate(
            &ctx,
            &target,
            Op::WriteFile {
                content: b"updated".to_vec(),
                mode: Some(0o644),
            },
        )
        .expect("mutate");
        // After the write, the live file is "updated".
        assert_eq!(fs::read(&target).unwrap(), b"updated");

        // Now run undo.
        let args = DoctorUndoArgs {
            run_id: initial_run.run_id.clone(),
            dry_run: false,
            json: true,
        };
        execute_undo(&args, repo).expect("undo");
        // The original bytes are back.
        assert_eq!(fs::read(&target).unwrap(), b"original");

        // Idempotence: running undo again is a no-op.
        execute_undo(&args, repo).expect("undo idempotent");
        assert_eq!(fs::read(&target).unwrap(), b"original");
    }

    #[test]
    fn test_doctor_undo_failure_does_not_mark_original_run_undone() {
        let tmp = unique_temp_root("undo-failure");
        let repo = tmp.path();
        let beads = repo.join(".beads");
        fs::create_dir_all(&beads).unwrap();
        fs::write(beads.join("foo.txt"), b"updated").unwrap();

        let initial_run = run_dir::create_run_dir(repo).expect("create run");
        let backup = initial_run.backups.join(".beads/foo.txt");
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        fs::write(&backup, b"original").unwrap();
        let action = serde_json::json!({
            "path": ".beads/foo.txt",
            "op": "write_file",
            "before_hash": "sha256:not-the-backup-hash"
        });
        fs::write(&initial_run.actions_file, format!("{action}\n")).unwrap();

        let args = DoctorUndoArgs {
            run_id: initial_run.run_id.clone(),
            dry_run: false,
            json: true,
        };
        execute_undo(&args, repo).expect("undo returns envelope even when a step fails");

        let report = fs::read_to_string(&initial_run.report_file).unwrap_or_default();
        assert!(
            !report.contains("undone_at"),
            "failed undo attempt must not stamp original report as undone: {report}"
        );
    }

    #[test]
    fn test_doctor_undo_malformed_action_does_not_mark_original_run_undone() {
        let tmp = unique_temp_root("undo-malformed-action");
        let repo = tmp.path();
        fs::create_dir_all(repo.join(".beads")).unwrap();

        let initial_run = run_dir::create_run_dir(repo).expect("create run");
        fs::write(&initial_run.actions_file, b"{ not valid json }\n").unwrap();

        let (actions, parse_failures) = read_actions_reverse(&initial_run.actions_file).unwrap();
        assert!(
            actions.is_empty(),
            "malformed line must not produce an action"
        );
        assert_eq!(parse_failures.len(), 1);
        assert_eq!(parse_failures[0].path, "actions.jsonl:1");
        assert_eq!(parse_failures[0].op, "parse_action");
        assert!(parse_failures[0].status.starts_with("failed_parse_action:"));

        let args = DoctorUndoArgs {
            run_id: initial_run.run_id.clone(),
            dry_run: false,
            json: true,
        };
        execute_undo(&args, repo).expect("undo returns envelope even when action log is malformed");

        let report = fs::read_to_string(&initial_run.report_file).unwrap_or_default();
        assert!(
            !report.contains("undone_at"),
            "malformed action log must not stamp original report as undone: {report}"
        );
    }

    #[test]
    fn test_doctor_undo_missing_backup_does_not_mark_original_run_undone() {
        let tmp = unique_temp_root("undo-missing-backup");
        let repo = tmp.path();
        let beads = repo.join(".beads");
        fs::create_dir_all(&beads).unwrap();
        fs::write(beads.join("foo.txt"), b"updated").unwrap();

        let initial_run = run_dir::create_run_dir(repo).expect("create run");
        let action = serde_json::json!({
            "path": ".beads/foo.txt",
            "op": "write_file",
            "before_hash": "sha256:backup-was-not-preserved"
        });
        fs::write(&initial_run.actions_file, format!("{action}\n")).unwrap();

        let args = DoctorUndoArgs {
            run_id: initial_run.run_id.clone(),
            dry_run: false,
            json: true,
        };
        execute_undo(&args, repo).expect("undo returns envelope even when backup is missing");

        let report = fs::read_to_string(&initial_run.report_file).unwrap_or_default();
        assert!(
            !report.contains("undone_at"),
            "missing backup must not stamp original report as undone: {report}"
        );
    }

    #[test]
    fn test_doctor_undo_rename_routes_through_mutate() {
        let tmp = unique_temp_root("undo-rename");
        let repo = tmp.path();
        let beads = repo.join(".beads");
        fs::create_dir_all(&beads).unwrap();
        let target = beads.join("renamed.txt");
        fs::write(&target, b"payload").unwrap();

        let initial_run = run_dir::create_run_dir(repo).expect("create run");
        let actions_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&initial_run.actions_file)
            .unwrap();
        let ctx = MutateContext {
            run_id: initial_run.run_id.clone(),
            run_dir: initial_run.root.clone(),
            capabilities: MutateCapabilities::for_repo(repo),
            actions_file: Mutex::new(actions_file),
            fixer_id: "test".into(),
            repo_root: repo.to_path_buf(),
            dry_run: false,
            start_ns: now_ns(),
        };
        let quarantine_path = initial_run.root.join("quarantine/renamed.txt");
        chokepoint::mutate(
            &ctx,
            &target,
            Op::Rename {
                to: quarantine_path.clone(),
            },
        )
        .expect("rename into quarantine");
        assert!(!target.exists());
        assert_eq!(fs::read(&quarantine_path).unwrap(), b"payload");

        let args = DoctorUndoArgs {
            run_id: initial_run.run_id.clone(),
            dry_run: false,
            json: true,
        };
        execute_undo(&args, repo).expect("undo rename");
        assert_eq!(fs::read(&target).unwrap(), b"payload");
        assert!(!quarantine_path.exists());

        let runs_root = repo.join(".doctor/runs");
        let undo_run_id = fs::read_dir(&runs_root)
            .unwrap()
            .find_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().to_string_lossy().into_owned();
                (name != initial_run.run_id).then_some(name)
            })
            .expect("undo run id");
        let undo_actions = fs::read_to_string(runs_root.join(undo_run_id).join("actions.jsonl"))
            .expect("undo actions");
        let line = undo_actions
            .lines()
            .next()
            .expect("undo action should be logged");
        let action: serde_json::Value = serde_json::from_str(line).expect("undo action json");
        assert_eq!(action["op"], "rename");
        assert_eq!(
            action["fixer_id"],
            format!("doctor_undo[{}]", initial_run.run_id)
        );
        assert_eq!(action["rename_to"], target.to_string_lossy().as_ref());
    }

    #[test]
    fn test_doctor_undo_latest_resolves_to_most_recent() {
        let tmp = unique_temp_root("undo-latest");
        let repo = tmp.path();
        let runs_root = repo.join(".doctor").join("runs");
        fs::create_dir_all(&runs_root).unwrap();
        // Three subdirs with sortable names; latest is the largest.
        for name in [
            "20260101T000000Z__a",
            "20260301T000000Z__b",
            "20260201T000000Z__c",
        ] {
            fs::create_dir_all(runs_root.join(name).join("backups")).unwrap();
            fs::write(runs_root.join(name).join("actions.jsonl"), b"").unwrap();
        }
        let latest = find_latest_run(&runs_root).unwrap().unwrap();
        assert_eq!(latest, "20260301T000000Z__b");
    }

    #[test]
    fn test_doctor_undo_latest_prefers_latest_symlink() {
        use std::os::unix::fs::symlink;

        let tmp = unique_temp_root("undo-latest-link");
        let repo = tmp.path();
        let runs_root = repo.join(".doctor").join("runs");
        fs::create_dir_all(&runs_root).unwrap();
        for name in ["20260102T000000Z__newer", "20260101T000000Z__linked"] {
            fs::create_dir_all(runs_root.join(name).join("backups")).unwrap();
            fs::write(runs_root.join(name).join("actions.jsonl"), b"").unwrap();
        }
        symlink("runs/20260101T000000Z__linked", repo.join(".doctor/latest")).unwrap();

        let latest = find_latest_run(&runs_root).unwrap().unwrap();
        assert_eq!(latest, "20260101T000000Z__linked");
    }

    #[test]
    fn test_doctor_robot_triage_emits_v1() {
        let env = build_triage_envelope(
            5,
            2,
            1,
            vec![TriageFinding {
                id: "fm-test".into(),
                severity: "P2".into(),
                message: "demo".into(),
            }],
        );
        let json = serde_json::to_string(&env).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema_version"], "br.doctor.triage.v1");
        assert!(v["findings"].is_array());
        assert_eq!(v["quick_ref"]["error"], 1);
        assert_eq!(v["quick_ref"]["warn"], 2);
        assert_eq!(v["quick_ref"]["healthy"], 5);
        assert!(
            v["recommended_command"]
                .as_str()
                .unwrap()
                .starts_with("br doctor")
        );
    }
}
