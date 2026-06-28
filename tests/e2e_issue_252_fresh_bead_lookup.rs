//! Regression test for issue #252: `br show` / `br update` / `br defer` /
//! `br sync --flush-only` intermittently reported "Issue not found" for
//! freshly-created beads that `br list` resolved cleanly.
//!
//! This test is intentionally phrased in behavioral terms rather than
//! implementation details. The original failure mode involved freshly-created
//! IDs becoming invisible to direct lookup-style commands while still showing
//! up in broader scans. The storage layer has since been simplified back to
//! direct keyed reads, so this file now serves as a guard that those lookup
//! and mutation paths stay correct regardless of the internal query strategy.
//!
//! This test creates a batch of beads in one process and then, in a second
//! process, calls every ID-lookup CLI entry point on every bead.  Any
//! "Issue not found" error fails the test.  The batch size and repetition
//! are chosen to cross the threshold where empirical repros of #252 (on
//! darwin_arm64) reported 2-3 failures out of every 5 fresh beads.

#![allow(clippy::uninlined_format_args)]

mod common;

use common::cli::{BrWorkspace, run_br};

fn parse_created_id(stdout: &str) -> String {
    let line = stdout.lines().next().unwrap_or("");
    let normalized = line
        .strip_prefix("✓ ")
        .or_else(|| line.strip_prefix("✗ "))
        .unwrap_or(line);
    normalized
        .strip_prefix("Created ")
        .and_then(|rest| rest.split(':').next())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Issue #252 primary repro: create N beads back-to-back, then resolve each
/// via `show`, `update`, and `defer`. Any fresh-ID lookup failure should
/// immediately fail the regression.
#[test]
fn e2e_issue_252_show_update_defer_find_all_freshly_created_beads() {
    const N: usize = 60;

    let ws = BrWorkspace::new();
    let init = run_br(&ws, ["init", "--prefix", "i252"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        let title = format!("issue-252 repro {}", i);
        let out = run_br(
            &ws,
            [
                "create",
                "--title",
                &title,
                "--type",
                "task",
                "--priority",
                "3",
                "--description",
                "repro for #252",
            ],
            &format!("create_{}", i),
        );
        assert!(out.status.success(), "create {} failed: {}", i, out.stderr);
        let id = parse_created_id(&out.stdout);
        assert!(!id.is_empty(), "could not parse id from: {:?}", out.stdout);
        ids.push(id);
    }

    for id in &ids {
        let show = run_br(&ws, ["show", id], &format!("show_{}", id));
        assert!(
            show.status.success(),
            "br show {} failed (issue #252): stdout={:?} stderr={:?}",
            id,
            show.stdout,
            show.stderr
        );
        assert!(
            !show.stderr.contains("Issue not found") && !show.stdout.contains("Issue not found"),
            "br show {} reported 'Issue not found' for a freshly-created bead (issue #252): {}",
            id,
            show.stderr
        );
    }

    for (i, id) in ids.iter().enumerate() {
        let update = run_br(
            &ws,
            ["update", id, "--notes", &format!("touch {}", i)],
            &format!("update_{}", id),
        );
        assert!(
            update.status.success(),
            "br update {} failed (issue #252): stderr={:?}",
            id,
            update.stderr
        );
        assert!(
            !update.stderr.contains("Issue not found")
                && !update.stdout.contains("Issue not found"),
            "br update {} reported 'Issue not found' (issue #252): {}",
            id,
            update.stderr
        );
    }

    for id in ids.iter().take(N / 2) {
        let defer = run_br(
            &ws,
            ["defer", id, "--until", "2099-01-01"],
            &format!("defer_{}", id),
        );
        assert!(
            defer.status.success(),
            "br defer {} failed (issue #252): stderr={:?}",
            id,
            defer.stderr
        );
        assert!(
            !defer.stderr.contains("issue not found") && !defer.stderr.contains("Issue not found"),
            "br defer {} reported 'Issue not found' (issue #252): {}",
            id,
            defer.stderr
        );
    }
}

/// Issue #252 flush-path repro: after creating N beads and touching them,
/// `br sync --flush-only` must export all N to `issues.jsonl`.  The original
/// report observed freshly-created beads silently dropped from export
/// because the same single-id lookup backed the export path for individual
/// dirty-issue reads.
#[test]
fn e2e_issue_252_sync_flush_only_exports_every_freshly_created_bead() {
    const N: usize = 40;

    let ws = BrWorkspace::new();
    let init = run_br(&ws, ["init", "--prefix", "i252flush"], "init");
    assert!(init.status.success(), "init failed: {}", init.stderr);

    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        let title = format!("flush-252 repro {}", i);
        let out = run_br(
            &ws,
            [
                "create",
                "--title",
                &title,
                "--type",
                "task",
                "--priority",
                "3",
            ],
            &format!("create_{}", i),
        );
        assert!(out.status.success(), "create {} failed: {}", i, out.stderr);
        let id = parse_created_id(&out.stdout);
        assert!(!id.is_empty(), "could not parse id from: {:?}", out.stdout);
        ids.push(id);
    }

    let flush = run_br(&ws, ["sync", "--flush-only"], "flush");
    assert!(
        flush.status.success(),
        "sync --flush-only failed: stdout={:?} stderr={:?}",
        flush.stdout,
        flush.stderr
    );

    let jsonl_path = ws.root.join(".beads").join("issues.jsonl");
    assert!(
        jsonl_path.exists(),
        "issues.jsonl missing after flush-only: {:?}",
        jsonl_path
    );
    let jsonl = std::fs::read_to_string(&jsonl_path).expect("read issues.jsonl");
    let exported_count = jsonl.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(
        exported_count, N,
        "flush dropped freshly-created beads (issue #252): exported {} of {}\nids: {:?}",
        exported_count, N, ids
    );

    for id in &ids {
        assert!(
            jsonl.contains(&format!("\"id\":\"{}\"", id)),
            "bead {} missing from flushed JSONL (issue #252)",
            id
        );
    }
}
