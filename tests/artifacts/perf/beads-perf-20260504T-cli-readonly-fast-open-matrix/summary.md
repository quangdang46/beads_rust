# CLI Read-Only Fast-Open Matrix

Date: 2026-05-04

Bead: `beads_rust-72yf.3`

## Objective

Extend the hot read-path proof surface from one CLI command to the full default
CLI command family currently allowed to use read-only fast-open. The slice also
adds an explicit conservative fallback knob:

```bash
BR_DISABLE_READ_ONLY_FAST_OPEN=1 br list --json --limit 1
```

Truthy values (`1`, `true`, `yes`, `on`) disable current-schema read-only
fast-open and force the locked/direct storage path.

## Opportunity Matrix

| Lever | Impact | Confidence | Effort | Score |
|---|---:|---:|---:|---:|
| Add conservative fast-open kill switch plus CLI read matrix proof | 4 | 4 | 2 | 8.0 |
| Promote `comments list` to safe auto-import read-only probe coverage | 3 | 4 | 1 | 12.0 |
| Expand proof to every default fast-open CLI read family | 4 | 5 | 1 | 20.0 |

Alien-graveyard mapping: this follows the fallback-safe fast-path pattern. The
optimized path has a direct conservative escape hatch, and the matrix proves the
candidate path against that fallback.

Alien-artifact mapping: the proof obligation is behavioral equivalence under a
stable witness: same workspace, same command args, same stdout bytes. The
failure-mode policy is conservative: disable the optimization and rerun through
the locked path.

## Matrix Commands

- `list --json --limit 5`
- `show <id> --format json`
- `search Fast-open --format json --limit 5`
- `ready --json --limit 5`
- `scheduler --json --limit 5 --candidate-limit 10`
- `blocked --json --limit 5`
- `count --json`
- `count --by label --json`
- `stale --days 0 --json`
- `lint --json`
- `sync --status --json`
- `stats --no-activity --json`
- `status --no-activity --json`
- `changelog --since 2100-01-01 --robot`
- `graph --all --compact`
- `orphans --robot`
- `comments list <id> --json`
- `comments <id> --json`
- `epic status --json`
- `label list`
- `label list-all --json`
- `dep list <id> --format json`
- `dep tree <id> --json`
- `dep cycles --json`
- `query run fast-open-p1 --format json`
- `query list --json`

## Proofs

Routine matrix test:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_purplesnow_fastopen \
  cargo test --test e2e_read_only_fast_open -- --nocapture
```

This asserts:

- Every matrix command succeeds with `BR_DISABLE_READ_ONLY_FAST_OPEN=1`.
- Every matrix command succeeds through default read-only fast-open.
- Fast-open stdout is byte-identical to conservative-path stdout, except for
  explicitly normalized volatile JSON clock fields (`scheduler.generated_at`,
  `changelog.until`).
- With `.beads/.write.lock` held, every default fast-open matrix command still
  succeeds.
- With `.beads/.write.lock` held, the conservative path times out and reports
  lock contention.

Perf probe:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_purplesnow_fastopen \
  cargo test --test e2e_read_only_fast_open \
  cli_read_only_fast_open_matrix_perf_probe -- --ignored --nocapture
```

Result:

```json
{
  "commands": [
    "list_json",
    "show_json",
    "search_json",
    "ready_json",
    "scheduler_json",
    "blocked_json",
    "count_json",
    "count_by_label_json",
    "stale_json",
    "lint_json",
    "sync_status_json",
    "stats_no_activity_json",
    "status_no_activity_json",
    "changelog_robot",
    "graph_all_compact",
    "orphans_robot",
    "comments_json",
    "comments_shorthand_json",
    "epic_status_json",
    "label_list_unique",
    "label_list_all_json",
    "dep_list_json",
    "dep_tree_json",
    "dep_cycles_json",
    "query_run_json",
    "query_list_json"
  ],
  "rounds": 5,
  "conservative_total_ns": 2806575002,
  "fast_open_total_ns": 2170888046,
  "speedup_milli": 1292,
  "equality": "routine matrix test asserts byte-identical stdout per command"
}
```

## Isomorphism Proof

- Ordering preserved: yes. The same command implementations render both paths;
  only startup storage-open strategy differs.
- Tie-breaking unchanged: yes. Sorting and filtering live below the storage
  handle and are shared by both paths.
- Floating-point: N/A.
- RNG seeds: N/A.
- Golden outputs: the matrix asserts byte-identical stdout per command, with
  explicit normalization only for volatile JSON clock fields.
- Fallback: `BR_DISABLE_READ_ONLY_FAST_OPEN=1` forces the direct locked path.

## Verification

- `cargo fmt --check`
- `git diff --check`
- `cargo test read_only_fast_open -- --nocapture`
- `cargo test --test e2e_read_only_fast_open -- --nocapture`
- `cargo test --test e2e_read_only_fast_open cli_read_only_fast_open_matrix_perf_probe -- --ignored --nocapture`
- `cargo check --all-targets`
- `cargo clippy --all-targets -- -D warnings`
- `ubs src/main.rs docs/SWARM_SCALE_TUNING.md tests/artifacts/perf/beads-perf-20260504T-cli-readonly-fast-open-matrix/summary.md`
  passed with 0 critical issues.
- `ubs src/main.rs tests/common/cli.rs tests/e2e_read_only_fast_open.rs docs/SWARM_SCALE_TUNING.md tests/artifacts/perf/beads-perf-20260504T-cli-readonly-fast-open-matrix/summary.md`
  exited 1 because UBS inventories existing test-helper `panic!` and
  assertion/unwrap surfaces in test files. Its embedded fmt, clippy, cargo
  check, and test-build subchecks were clean.
