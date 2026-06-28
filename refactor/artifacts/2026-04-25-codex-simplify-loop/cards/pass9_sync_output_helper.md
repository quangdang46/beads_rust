# Pass 9: Sync Output Predicate Helper

## Change

Renamed the sync command's inverted `suppress_human_sync_output` predicate to
`should_render_human_sync_output` and updated the call sites to use positive
logic.

## Equivalence Contract

- Quiet human output remains suppressed.
- JSON and robot output remain emitted even when quiet mode is active.
- Rich/plain rendering branches keep their existing order.
- Sync import, export, merge, recovery, and path-safety internals are untouched.
- The change is limited to command wrapper output gating.

## Verification

- Worker focused test: `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_codex_pass9 cargo test should_render_human_sync_output_preserves_quiet_json_semantics` passed.
- Static no-git check: `grep -rn 'Command::new.*git' src/sync/ src/cli/commands/sync.rs` returned no matches.
- Dependency check: `grep -E '^(git2|gitoxide|libgit)' Cargo.toml` returned no matches.
- Sync allowlist inspection confirmed the existing internal allowlist and external JSONL opt-in boundary.
- `ubs src/cli/commands/sync.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_simplify cargo test --lib should_render_human_sync_output_preserves_quiet_json_semantics` passed.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_simplify cargo test --lib sync:: --release` passed with 228 sync-related tests.
- `rch exec -- env TMPDIR=/data/tmp CARGO_TARGET_DIR=/data/tmp/rch_target_magentalotus_simplify cargo test e2e_sync --release` passed.
