# Pass 1: Domain Enum Parser Collapse

## Change

Collapsed duplicated known-value parsing in `Status` and `IssueType` into
private `known_value` helpers.

## Equivalence Contract

- Inputs covered: `Status` and `IssueType` serde deserialization and `FromStr`.
- Ordering preserved: N/A; each parser is a single string match.
- Tie-breaking: unchanged; `in_progress` and `inprogress` still resolve to `InProgress`.
- Error semantics: unchanged; these parsers still accept unknown strings as custom values.
- Laziness: unchanged for observable behavior; custom fallback now avoids eager construction.
- Short-circuit evaluation: unchanged; first matching string arm wins.
- Floating point: N/A.
- RNG/hash order: N/A.
- Observable side effects: none.
- Type narrowing: N/A.

## Verification

- `ubs src/model/mod.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib custom` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib from_str` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --test proptest_model_roundtrip` passed.
- Golden replay in `cards/pass1_golden_replay/` matched baseline outputs with empty diffs.
