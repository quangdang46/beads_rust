# Pass 8: Config Layer Lookup Helper

## Change

Added `ConfigLayer::get` in `src/config/mod.rs` and used it in
`src/cli/commands/config.rs` where config rendering previously repeated the
same `runtime.get(key).or_else(|| startup.get(key))` lookup.

## Equivalence Contract

- Precedence preserved: runtime keys are checked before startup keys.
- Canonicalization preserved: the helper does not canonicalize; callers pass
  the same key they passed to the duplicated expression before.
- `br config get` behavior preserved: it still canonicalizes the user-provided
  key before lookup.
- Rich/plain list behavior preserved: render paths still iterate the existing
  map keys and use those exact keys for lookup.
- Startup-only and runtime-only grouped text output: untouched.
- Config loading, source resolution, and merge behavior: untouched.

## Verification

- `ubs src/config/mod.rs src/cli/commands/config.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test config_layer_get_checks_runtime_then_startup_without_canonicalizing` passed.
- Worker fresh-eyes ran `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_codex_pass8 cargo test config::tests::`, passing 149 config tests.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
