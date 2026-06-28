# Pass 10: Update Output Partition Helpers

## Change

Added private `update_uses_machine_output` and `update_uses_human_output`
helpers in `src/cli/commands/update.rs`, then reused them in the routed update
path and per-route output assembly.

## Equivalence Contract

- JSON and TOON modes still collect structured `UpdatedIssueOutput` values.
- Quiet mode still collects neither structured output nor human render items.
- Rich and plain modes still collect human render items.
- The final emission branch remains unchanged: TOON emits TOON, JSON emits JSON,
  and non-quiet human modes print render items.
- Update mutation, routing, ID resolution, auto-flush, and blocked-cache behavior
  are untouched.

## Verification

- Fresh-eyes worker reviewed the change and made no edits.
- `ubs src/cli/commands/update.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_pass10 cargo test --lib test_update_output_partition_matches_previous_mode_checks` passed.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_pass10 cargo test --lib cli::commands::update::tests` passed with 21 tests.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_pass10 cargo check --all-targets` passed.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_pass10 cargo clippy --all-targets -- -D warnings` passed.
