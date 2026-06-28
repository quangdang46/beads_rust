# Verify Report - 2026-04-24-shrink-pass-1

## Commands run

| Command | Artifact | Result |
|---------|----------|--------|
| `bash /home/ubuntu/.codex/skills/simplify-and-refactor-code-isomorphically/scripts/check_skills.sh refactor/artifacts/2026-04-24-shrink-pass-1` | `skill_inventory.json` | Completed. |
| `git ls-files 'src/**/*.rs' 'tests/**/*.rs' 'benches/**/*.rs' \| xargs wc -l` | `loc_before.txt` | Completed; total 110514. |
| `rg -n "match .*Err\\(e\\) => Err\\(e\\)\|\\.unwrap\\(\\)\|\\.expect\\(" -t rust src tests \| head -200` | `slop_scan_seed.txt` | Completed; capped seed scan. |
| `ast-grep run -l Rust -p 'fn $N($$$A) { $$$B }' src --json` | `ast_grep_functions.json` | Completed. |
| `bv --robot-triage` | `bv_triage.json` | Completed. |
| `cargo fmt --check` | `fmt_before.txt` | Green before source edits. |
| `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo test --no-fail-fast` | `tests_before.txt` | Red before source edits. |
| `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo check --all-targets` | `check_before.txt` | Green before source edits, with one warning. |
| `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo clippy --all-targets -- -D warnings` | `clippy_before.txt` | Red before source edits. |
| `cargo fmt --check` after branch movement | `fmt_after_head_moved.txt` | Green. |
| `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo check --all-targets` after branch movement | `check_after_head_moved.txt` | Red due uncommitted `src/sync/mod.rs` change not made by this pass. |
| `cargo fmt --check` on final clean HEAD `bed322362174` | `fmt_current.txt` | Green. |
| `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo check --all-targets` on final clean HEAD `bed322362174` | `check_current.txt` | Green. |
| `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo clippy --all-targets -- -D warnings` on final clean HEAD `bed322362174` | `clippy_current.txt` | Green. |
| `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo test --no-fail-fast` on final clean HEAD `bed322362174` | `tests_current.txt` | Red: six targets failed. |
| `rch exec -- env TMPDIR=/data/tmp/gold_bison_tmp CARGO_TARGET_DIR=/data/tmp/rch_target_gold_bison_full CARGO_INCREMENTAL=0 cargo test --no-fail-fast --test e2e_schema --test snapshots -- --nocapture` | terminal log | Green after targeted baseline fixes. |
| `rch exec -- env TMPDIR=/data/tmp/gold_bison_tmp CARGO_TARGET_DIR=/data/tmp/rch_target_gold_bison_full CARGO_INCREMENTAL=0 cargo test --no-fail-fast` after baseline fixes | terminal log | Green before D1. |
| `rch exec -- env TMPDIR=/data/tmp/gold_bison_tmp CARGO_TARGET_DIR=/data/tmp/rch_target_gold_bison_full CARGO_INCREMENTAL=0 cargo test format::markdown` after D1 | terminal log | Green: 20 tests passed. |
| `rch exec -- env TMPDIR=/data/tmp/gold_bison_tmp CARGO_TARGET_DIR=/data/tmp/rch_target_gold_bison_full CARGO_INCREMENTAL=0 cargo test format::syntax` after D1 | terminal log | Green: 19 tests passed. |
| `cargo fmt --check` after D1 | terminal log | Green. |
| `rch exec -- env TMPDIR=/data/tmp/gold_bison_tmp CARGO_TARGET_DIR=/data/tmp/rch_target_gold_bison_full CARGO_INCREMENTAL=0 cargo check --all-targets` after D1 | terminal log | Green. |
| `rch exec -- env TMPDIR=/data/tmp/gold_bison_tmp CARGO_TARGET_DIR=/data/tmp/rch_target_gold_bison_full CARGO_INCREMENTAL=0 cargo clippy --all-targets -- -D warnings` after D1 | terminal log | Green. |
| `rch exec -- env TMPDIR=/data/tmp/gold_bison_tmp CARGO_TARGET_DIR=/data/tmp/rch_target_gold_bison_full CARGO_INCREMENTAL=0 cargo test --no-fail-fast` after D1 | terminal log | Green. |

## Baseline repair

The original full-suite failures were fixed before refactoring:

- `src/cli/commands/create.rs` now resolves dependency references by exact title
  after ID/hash lookup fails, with an ambiguity error for duplicate exact titles.
- `src/storage/sqlite.rs` exposes the exact-title lookup needed for that path.
- Workspace replay tests now serialize long-lived replay fixture materialization
  through a shared guard, and fixture expectations were updated to match current
  doctor/status behavior.
- Snapshot redaction now handles `/data/tmp/<prefix>/.tmp...` paths, and the
  affected CLI/golden snapshots plus the version baseline were updated.
- The stale `generate_id_seed` doc/test expectation now matches the
  length-prefixed format.

## D1 execution

D1 was executed after the repaired baseline was green. The markdown and syntax
formatter test modules now use one `ctx(OutputMode)` helper each instead of five
duplicated helpers per module. The production formatter paths were not changed.

The direct formatter-test diff is 20 insertions and 52 deletions, for a net
32-line reduction. The final full-suite and lint/type/format gates are green.
