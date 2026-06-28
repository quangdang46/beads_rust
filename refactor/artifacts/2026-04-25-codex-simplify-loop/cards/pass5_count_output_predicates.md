# Pass 5: Count Output Predicate Normalization

## Change

Replaced local `matches!(ctx.mode(), OutputMode::Quiet)` and
`matches!(ctx.mode(), OutputMode::Rich)` checks in `src/cli/commands/count.rs`
with the existing `OutputContext` predicates.

## Equivalence Contract

- Inputs covered: count command output mode selection.
- Ordering preserved: quiet still returns before all render branches; toon/json
  still precede rich/plain rendering.
- Error semantics: unchanged.
- Observable side effects: unchanged; predicates are direct equality checks
  against the same `OutputMode` variants.
- Type/import behavior: `OutputMode` import was removed after it became unused.

## Verification

- `ubs src/cli/commands/count.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
- Clean detached-worktree verification with only this pass applied passed: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_pass5_clean cargo test --lib cli::commands::count::tests`.
- Main worktree verification later passed after the unrelated storage edit cleared: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib cli::commands::count::tests`.
