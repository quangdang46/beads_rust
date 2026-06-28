# Baseline - 2026-04-24-shrink-pass-1

Captured: 2026-04-24T22:06:37Z
Branch: main
Initial observed HEAD during pass: aa7c055fa0b9
Final observed clean HEAD: bed322362174
Project: beads_rust
Agent: CopperOsprey

## Required source-of-truth reads

- Read `AGENTS.md` completely before touching the repo.
- Read `README.md` completely before touching the repo.
- Read `docs/ARCHITECTURE.md` entry architecture section for dispatch/storage/config/sync context.
- Ran codebase architecture search before choosing candidates.

## Coordination

- Agent Mail registration: `CopperOsprey`.
- Artifact reservation granted: `refactor/artifacts/2026-04-24-shrink-pass-1/**`.
- `src/storage/sqlite.rs` reservation request was rejected because `JadeCondor` holds an active exclusive reservation.
- Narrow alternate reservations were granted for `src/format/syntax.rs` and `src/format/markdown.rs`.
- No source edits were made because the baseline was not green and later concurrent work dirtied `src/sync/mod.rs`.

## Test suite

- Command: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo test --no-fail-fast`
- Output: `tests_before.txt`
- Result: RED before any source edit in this pass.
- Failing targets:
  - `e2e_sync_failure_injection`: 157 passed, 2 failed.
  - `snapshots`: 224 passed, 3 failed.
  - `storage_golden_snapshot`: 0 passed, 1 failed.
  - `storage_id_hash_parity`: 15 passed, 1 failed.
  - `test_create_deps_colon`: 0 passed, 1 failed.
  - `workspace_failure_replay`: 141 passed, 3 failed.

## LOC

- Command: `git ls-files 'src/**/*.rs' 'tests/**/*.rs' 'benches/**/*.rs' | xargs wc -l`
- Output: `loc_before.txt`
- Total Rust LOC counted: 110514.

## Typecheck, lint, format

- Command: `cargo fmt --check`
- Output: `fmt_before.txt`
- Result: GREEN at first baseline.

- Command: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo check --all-targets`
- Output: `check_before.txt`
- Result: GREEN at first baseline with one warning in `src/sync/mod.rs`.

- Command: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo clippy --all-targets -- -D warnings`
- Output: `clippy_before.txt`
- Result: RED at first baseline because the `src/sync/mod.rs` warning became an error and clippy also reported `needless_pass_by_value`.

## Concurrent branch movement and final refresh

While this pass was collecting evidence, `main` advanced through sync refactor commits. A transient uncommitted `src/sync/mod.rs` diff appeared and then cleared. The final clean observed HEAD for this pass was `bed322362174`.

A refreshed current-head gate was run:

- Command: `cargo fmt --check`
- Output: `fmt_current.txt`
- Result: GREEN.

- Command: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo check --all-targets`
- Output: `check_current.txt`
- Result: GREEN.

- Command: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo clippy --all-targets -- -D warnings`
- Output: `clippy_current.txt`
- Result: GREEN.

- Command: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_copper_osprey cargo test --no-fail-fast`
- Output: `tests_current.txt`
- Result: RED. Current clean HEAD still fails six targets:
  - `e2e_sync_failure_injection`: 157 passed, 2 failed.
  - `snapshots`: 224 passed, 3 failed.
  - `storage_golden_snapshot`: 0 passed, 1 failed.
  - `storage_id_hash_parity`: 15 passed, 1 failed.
  - `test_create_deps_colon`: 0 passed, 1 failed.
  - `workspace_failure_replay`: 139 passed, 5 failed.

## Refactor gate decision

The simplify skill requires a green test baseline before behavior-preserving source edits. The final current-head type/lint gates are green, but the full test baseline is still red. This pass therefore produced artifacts and candidate cards only; it did not modify source.
