//! E2E coverage for CLI read-only fast-open behavior.
//!
//! These tests compare the optimized current-schema read-only path against the
//! conservative locked path, then prove representative read commands still run
//! while another process holds `.beads/.write.lock`.

mod common;

use common::cli::{BrRun, BrWorkspace, parse_created_id, run_br, run_br_with_env};
use serde_json::json;
use std::fs::OpenOptions;
use std::time::{Duration, Instant};

const DISABLE_FAST_OPEN_ENV: (&str, &str) = ("BR_DISABLE_READ_ONLY_FAST_OPEN", "1");

struct SeededWorkspace {
    workspace: BrWorkspace,
    blocker_id: String,
    blocked_id: String,
}

struct MatrixCommand {
    label: &'static str,
    args: Vec<String>,
}

fn assert_success(run: &BrRun, label: &str) {
    assert!(
        run.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        run.stdout,
        run.stderr
    );
}

fn seed_workspace() -> SeededWorkspace {
    let workspace = BrWorkspace::new();

    let init = run_br(&workspace, ["init"], "init");
    assert_success(&init, "init");

    let blocker = run_br(
        &workspace,
        [
            "create",
            "Fast-open blocker issue",
            "-p",
            "1",
            "--type",
            "bug",
        ],
        "create_blocker",
    );
    assert_success(&blocker, "create_blocker");
    let blocker_id = parse_created_id(&blocker.stdout);

    let blocked = run_br(
        &workspace,
        [
            "create",
            "Fast-open blocked issue",
            "-p",
            "2",
            "--type",
            "task",
        ],
        "create_blocked",
    );
    assert_success(&blocked, "create_blocked");
    let blocked_id = parse_created_id(&blocked.stdout);

    let ready = run_br(
        &workspace,
        [
            "create",
            "Fast-open ready issue",
            "-p",
            "0",
            "--type",
            "feature",
        ],
        "create_ready",
    );
    assert_success(&ready, "create_ready");

    let comment = run_br(
        &workspace,
        [
            "comments",
            "add",
            &blocker_id,
            "--author",
            "fast-open-test",
            "Snapshot matrix comment",
        ],
        "add_comment",
    );
    assert_success(&comment, "add_comment");

    let dep = run_br(
        &workspace,
        ["dep", "add", &blocked_id, &blocker_id],
        "dep_add",
    );
    assert_success(&dep, "dep_add");

    let save_query = run_br(
        &workspace,
        ["query", "save", "fast-open-p1", "--priority", "1"],
        "query_save",
    );
    assert_success(&save_query, "query_save");

    let flush = run_br(&workspace, ["sync", "--flush-only", "--json"], "sync_flush");
    assert_success(&flush, "sync_flush");

    SeededWorkspace {
        workspace,
        blocker_id,
        blocked_id,
    }
}

fn matrix_commands(seed: &SeededWorkspace) -> Vec<MatrixCommand> {
    vec![
        MatrixCommand {
            label: "list_json",
            args: strings(["--lock-timeout", "50", "list", "--json", "--limit", "5"]),
        },
        MatrixCommand {
            label: "show_json",
            args: vec![
                "--lock-timeout".into(),
                "50".into(),
                "show".into(),
                seed.blocker_id.clone(),
                "--format".into(),
                "json".into(),
            ],
        },
        MatrixCommand {
            label: "ready_json",
            args: strings(["--lock-timeout", "50", "ready", "--json", "--limit", "5"]),
        },
        MatrixCommand {
            label: "blocked_json",
            args: strings(["--lock-timeout", "50", "blocked", "--json", "--limit", "5"]),
        },
        MatrixCommand {
            label: "comments_json",
            args: vec![
                "--lock-timeout".into(),
                "50".into(),
                "comments".into(),
                "list".into(),
                seed.blocker_id.clone(),
                "--json".into(),
            ],
        },
        MatrixCommand {
            label: "dep_list_json",
            args: vec![
                "--lock-timeout".into(),
                "50".into(),
                "dep".into(),
                "list".into(),
                seed.blocked_id.clone(),
                "--format".into(),
                "json".into(),
            ],
        },
        MatrixCommand {
            label: "query_run_json",
            args: strings([
                "--lock-timeout",
                "50",
                "query",
                "run",
                "fast-open-p1",
                "--format",
                "json",
            ]),
        },
    ]
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.into_iter().map(str::to_string).collect()
}

fn run_command(workspace: &BrWorkspace, command: &MatrixCommand, disable_fast_open: bool) -> BrRun {
    let args = command.args.iter().map(String::as_str);
    if disable_fast_open {
        run_br_with_env(
            workspace,
            args,
            [DISABLE_FAST_OPEN_ENV],
            &format!("{}_conservative", command.label),
        )
    } else {
        run_br(workspace, args, &format!("{}_fast", command.label))
    }
}

#[test]
fn cli_read_only_fast_open_matrix_matches_conservative_outputs() {
    let _log = common::test_log("cli_read_only_fast_open_matrix_matches_conservative_outputs");
    let seed = seed_workspace();

    for command in matrix_commands(&seed) {
        let conservative = run_command(&seed.workspace, &command, true);
        assert_success(&conservative, command.label);

        let fast = run_command(&seed.workspace, &command, false);
        assert_success(&fast, command.label);

        assert_eq!(
            fast.stdout, conservative.stdout,
            "{} stdout changed between read-only fast-open and conservative locked path",
            command.label
        );
    }
}

#[test]
fn cli_read_only_fast_open_matrix_bypasses_held_write_lock() {
    let _log = common::test_log("cli_read_only_fast_open_matrix_bypasses_held_write_lock");
    let seed = seed_workspace();
    let lock_path = seed.workspace.root.join(".beads/.write.lock");
    let write_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open write lock");
    write_lock.lock().expect("hold write lock");

    for command in matrix_commands(&seed) {
        let fast = run_command(&seed.workspace, &command, false);
        assert_success(&fast, command.label);
    }

    let blocked_conservative = run_command(
        &seed.workspace,
        &MatrixCommand {
            label: "list_json_locked_conservative",
            args: strings(["--lock-timeout", "50", "list", "--json", "--limit", "1"]),
        },
        true,
    );
    assert!(
        !blocked_conservative.status.success(),
        "disabled fast-open should wait for the held write lock and time out"
    );
    let combined = format!(
        "{} {}",
        blocked_conservative.stdout, blocked_conservative.stderr
    )
    .to_ascii_lowercase();
    assert!(
        combined.contains("lock") || combined.contains("timed out"),
        "conservative failure should mention lock contention, got: {combined}"
    );
}

fn run_matrix_round(workspace: &BrWorkspace, commands: &[MatrixCommand], disable_fast_open: bool) {
    for command in commands {
        let run = run_command(workspace, command, disable_fast_open);
        assert_success(&run, command.label);
    }
}

fn duration_ns_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[test]
#[ignore = "perf probe for CLI read-only fast-open matrix evidence"]
fn cli_read_only_fast_open_matrix_perf_probe() {
    let seed = seed_workspace();
    let commands = matrix_commands(&seed);
    let rounds = 5_u32;

    let conservative_start = Instant::now();
    for _ in 0..rounds {
        run_matrix_round(&seed.workspace, &commands, true);
    }
    let conservative = conservative_start.elapsed();

    let fast_start = Instant::now();
    for _ in 0..rounds {
        run_matrix_round(&seed.workspace, &commands, false);
    }
    let fast = fast_start.elapsed();

    let conservative_ns = duration_ns_u64(conservative);
    let fast_ns = duration_ns_u64(fast);
    println!(
        "{}",
        json!({
            "commands": commands.iter().map(|command| command.label).collect::<Vec<_>>(),
            "rounds": rounds,
            "conservative_total_ns": conservative_ns,
            "fast_open_total_ns": fast_ns,
            "speedup_milli": conservative_ns.saturating_mul(1000) / fast_ns.max(1),
            "equality": "routine matrix test asserts byte-identical stdout per command",
        })
    );
}
