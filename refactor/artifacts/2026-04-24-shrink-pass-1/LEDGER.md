# Ledger - 2026-04-24-shrink-pass-1

| Order | ID | Commit | File(s) | LOC before | LOC after | Delta | Tests | Lints |
|-------|----|--------|---------|------------|-----------|-------|-------|-------|
| 0 | Baseline repair | working tree | test fixtures, snapshots, create dependency lookup, workspace replay guard, temp-path redaction, version baseline | 110514 | 110601 | +87 | `cargo test --no-fail-fast` green after fixes | `cargo clippy --all-targets -- -D warnings` green |
| 1 | D1 | working tree | `src/format/markdown.rs`, `src/format/syntax.rs` | 110601 | 110569 | -32 | `cargo test --no-fail-fast` green | `cargo clippy --all-targets -- -D warnings` green |

## Summary

The pass initially stopped at the required safety gate because the full test
baseline was red. The baseline was repaired first, then D1 was applied
isomorphically.

- Test repair covered stale snapshots, workspace failure fixture expectations,
  create dependency resolution for exact title matches, workspace replay
  serialization, temp-path redaction for `/data/tmp`, and the version baseline.
- D1 collapsed duplicated test-only `OutputContext` helper functions in the
  markdown and syntax formatter tests into one parameterized helper per module.
- Production formatter behavior and public APIs were unchanged.
- Final observed gates are green: `cargo fmt --check`, `cargo check
  --all-targets`, `cargo clippy --all-targets -- -D warnings`, targeted
  `format::markdown` / `format::syntax` tests, and full `cargo test
  --no-fail-fast`.

Net LOC delta from D1 source edits: -32.
Net tracked Rust LOC delta after baseline repair plus D1: +55.
