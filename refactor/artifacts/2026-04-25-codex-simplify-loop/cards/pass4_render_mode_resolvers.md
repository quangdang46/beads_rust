# Pass 4: Render Mode Resolver Cleanup

## Change

Collapsed the duplicated JSON-precedence early-return shape in changelog and
orphans render-mode resolvers into tuple matches.

## Equivalence Contract

- Inputs covered: `json` robot flag and all `OutputMode` variants.
- Ordering preserved: explicit robot JSON still has first priority.
- Tie-breaking: `(true, _)` still maps to JSON for every output mode.
- Error semantics: N/A; pure resolver functions do not fail.
- Hint/context behavior: N/A.
- Observable side effects: none; `OutputMode` is copied and matched.

## Truth Table

- `json == true` with any output mode -> JSON.
- `json == false && OutputMode::Json` -> JSON.
- `json == false && OutputMode::Quiet` -> Quiet.
- `json == false && OutputMode::Toon` -> Toon.
- `json == false && OutputMode::Rich` -> Rich.
- `json == false && OutputMode::Plain` -> Plain.

## Verification

- `ubs src/cli/commands/changelog.rs src/cli/commands/orphans.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib resolve_render_mode` passed.
