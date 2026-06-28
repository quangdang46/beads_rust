# Pass 6: Blocked DTO Conversion Helper

## Change

Collapsed duplicated JSON and TOON `BlockedIssueOutput` construction in
`src/cli/commands/blocked.rs` into private helper functions.

## Equivalence Contract

- Inputs covered: `blocked_issues` returned by the blocked command after
  filtering, sorting, and limiting.
- Ordering preserved: helper maps the same slice iterator in order.
- Field values preserved: helper body is the same struct-literal mapping used
  by both previous output branches.
- Blocker normalization preserved: `blocker_id_from_ref` is still applied to
  each blocker reference before string allocation.
- Output branch behavior preserved: JSON still calls `ctx.json_pretty`; TOON
  still calls `ctx.toon_with_stats(&output, args.stats)`.
- Text, rich, CSV, filtering, sorting, and storage behavior: untouched.

## Verification

- `ubs src/cli/commands/blocked.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib cli::commands::blocked::tests` passed.
