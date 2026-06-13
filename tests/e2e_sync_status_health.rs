//! E2E coverage for the `br sync --status --json` additions:
//!
//! - beads_rust#338: read-only `git_export` block (tracked/dirty JSONL
//!   visibility; `{available:false}` outside a git repo).
//! - beads_rust#334: `workspace_health` + `reliability_audit` fields in
//!   the same write-gate vocabulary as `br doctor --json`.

mod common;

use common::cli::{BrWorkspace, extract_json_payload, run_br};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args([
            "-c",
            "user.name=br-e2e",
            "-c",
            "user.email=br-e2e@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git")
}

fn git_ok(root: &Path, args: &[&str]) {
    let out = git(root, args);
    assert!(
        out.status.success(),
        "git {args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn sync_status_json(workspace: &BrWorkspace, label: &str) -> Value {
    let status = run_br(workspace, ["sync", "--status", "--json"], label);
    assert!(
        status.status.success(),
        "sync --status failed: {}",
        status.stderr
    );
    serde_json::from_str(&extract_json_payload(&status.stdout)).expect("sync status json")
}

#[test]
fn e2e_sync_status_git_export_committed_vs_dirty_jsonl() {
    let _log = common::test_log("e2e_sync_status_git_export_committed_vs_dirty_jsonl");
    let workspace = BrWorkspace::new();

    git_ok(&workspace.root, &["init", "--initial-branch=main"]);

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let create = run_br(&workspace, ["create", "Git status issue"], "create");
    assert!(create.status.success(), "create failed: {}", create.stderr);

    let flush = run_br(&workspace, ["sync", "--flush-only"], "flush");
    assert!(flush.status.success(), "flush failed: {}", flush.stderr);

    // Quiesce first: a `sync --status` open may auto-export the JSONL
    // (the export embeds fresh timestamps), so run it once and re-flush
    // before committing so the committed bytes are br's canonical output
    // and a later status call won't rewrite the worktree under us.
    let _ = sync_status_json(&workspace, "status_quiesce");
    let reflush = run_br(&workspace, ["sync", "--flush-only"], "reflush");
    assert!(reflush.status.success(), "reflush failed: {}", reflush.stderr);

    // Untracked JSONL: available, but not tracked and not worktree-clean.
    let untracked = sync_status_json(&workspace, "status_untracked");
    let git_export = &untracked["git_export"];
    assert_eq!(git_export["available"], true, "{untracked}");
    assert_eq!(git_export["tracked"], false, "{untracked}");
    assert_eq!(git_export["worktree_clean"], false, "{untracked}");
    assert_eq!(git_export["index_clean"], true, "{untracked}");
    assert!(git_export["head_hash"].is_null(), "{untracked}");
    assert!(git_export["worktree_hash"].is_string(), "{untracked}");

    // Commit the JSONL: tracked, and the committed copy is now visible.
    git_ok(&workspace.root, &["add", ".beads/issues.jsonl"]);
    git_ok(&workspace.root, &["commit", "-m", "track issues.jsonl"]);
    let committed_head =
        git_committed_blob_hash(&workspace.root, ".beads/issues.jsonl").expect("head blob hash");

    let committed = sync_status_json(&workspace, "status_committed");
    let git_export = &committed["git_export"];
    assert_eq!(git_export["available"], true, "{committed}");
    assert_eq!(git_export["tracked"], true, "{committed}");
    assert_eq!(git_export["index_clean"], true, "{committed}");
    // The reported HEAD blob hash must agree with what git records for
    // the committed copy (independent of any worktree re-export jitter).
    assert_eq!(
        git_export["head_hash"].as_str().expect("head hash"),
        committed_head,
        "{committed}"
    );
    assert_eq!(committed_head.len(), 40, "{committed}");

    // Dirty the tracked JSONL: previously invisible to sync --status.
    let create2 = run_br(&workspace, ["create", "Second issue"], "create2");
    assert!(
        create2.status.success(),
        "create2 failed: {}",
        create2.stderr
    );
    let flush2 = run_br(&workspace, ["sync", "--flush-only"], "flush2");
    assert!(flush2.status.success(), "flush2 failed: {}", flush2.stderr);

    let dirty = sync_status_json(&workspace, "status_dirty");
    let git_export = &dirty["git_export"];
    assert_eq!(git_export["available"], true, "{dirty}");
    assert_eq!(git_export["tracked"], true, "{dirty}");
    assert_eq!(git_export["worktree_clean"], false, "{dirty}");
    assert_ne!(
        git_export["head_hash"].as_str().expect("head hash"),
        git_export["worktree_hash"].as_str().expect("worktree hash"),
        "dirty worktree must hash differently from HEAD: {dirty}"
    );
}

/// Resolve the committed blob hash for `relpath` via git, returning
/// `None` when the path is absent from HEAD.
fn git_committed_blob_hash(root: &Path, relpath: &str) -> Option<String> {
    let out = git(root, &["rev-parse", "--verify", "--quiet", &format!("HEAD:{relpath}")]);
    if !out.status.success() {
        return None;
    }
    let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if hash.is_empty() { None } else { Some(hash) }
}

#[test]
fn e2e_sync_status_git_export_unavailable_outside_repo() {
    let _log = common::test_log("e2e_sync_status_git_export_unavailable_outside_repo");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let status = sync_status_json(&workspace, "status_no_git");
    let git_export = &status["git_export"];
    assert_eq!(git_export["available"], false, "{status}");
    for absent in [
        "tracked",
        "worktree_clean",
        "index_clean",
        "head_hash",
        "worktree_hash",
    ] {
        assert!(
            git_export.get(absent).is_none(),
            "{absent} must be omitted when git is unavailable: {status}"
        );
    }
}

#[test]
fn e2e_sync_status_reports_workspace_health_and_reliability_audit() {
    let _log = common::test_log("e2e_sync_status_reports_workspace_health_and_reliability_audit");
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let create = run_br(&workspace, ["create", "Health issue"], "create");
    assert!(create.status.success(), "create failed: {}", create.stderr);

    // Unflushed create → DB newer than JSONL → degraded with db_newer.
    let pending = sync_status_json(&workspace, "status_pending_export");
    assert_eq!(pending["db_newer"], true, "{pending}");
    assert_eq!(pending["workspace_health"], "degraded", "{pending}");
    let audit = &pending["reliability_audit"];
    assert_eq!(audit["source"], "sync.status", "{pending}");
    assert_eq!(audit["health"], "degraded", "{pending}");
    let codes: Vec<&str> = audit["anomalies"]
        .as_array()
        .expect("anomalies array")
        .iter()
        .filter_map(|a| a["code"].as_str())
        .collect();
    assert!(
        codes.contains(&"db_newer"),
        "expected db_newer anomaly code, got {codes:?}: {pending}"
    );

    // After flush the drift clears and the workspace is healthy.
    let flush = run_br(&workspace, ["sync", "--flush-only"], "flush");
    assert!(flush.status.success(), "flush failed: {}", flush.stderr);

    let healthy = sync_status_json(&workspace, "status_healthy");
    assert_eq!(healthy["workspace_health"], "healthy", "{healthy}");
    assert_eq!(
        healthy["reliability_audit"]["anomaly_count"], 0,
        "{healthy}"
    );
    assert_eq!(
        healthy["reliability_audit"]["health"], "healthy",
        "{healthy}"
    );
}
