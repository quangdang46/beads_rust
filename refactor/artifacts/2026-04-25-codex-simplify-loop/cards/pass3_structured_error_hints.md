# Pass 3: Structured Error Hint Cleanup

## Change

Collapsed repeated option-producing hint branches in `src/error/structured.rs`
using shared hint constants and a small `flag_value_hint` formatter.

## Equivalence Contract

- Inputs covered: invalid priority, status, and type errors.
- Ordering preserved: N/A; each branch is a single detector call.
- Tie-breaking: unchanged; detector functions were not modified.
- Error semantics: unchanged; error codes, retryability, and context JSON are
  untouched.
- Hint presence: unchanged; public constructors still always emit hints,
  `generate_hint` still returns `None` for undetected status/type, and priority
  keeps its fallback hint.
- Hint text: byte-preserved via shared constants and identical format strings.
- Observable side effects: none.

## Verification

- `ubs src/error/structured.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib structured` passed.
