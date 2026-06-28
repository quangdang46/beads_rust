# Pass 2: Count Parse Helper Consolidation

## Change

Collapsed the three private count-filter parsers into one generic
`parse_trimmed_values<T>` helper in `src/cli/commands/count.rs`.

## Equivalence Contract

- Inputs covered: `--status`, `--type`, and `--priority` count filters.
- Ordering preserved: iterator order is unchanged.
- Tie-breaking: N/A.
- Error semantics: unchanged; each value still goes through the same `FromStr`
  implementation and returns the same `BeadsError`.
- Laziness: unchanged; all values are collected into a `Vec`.
- Short-circuit evaluation: unchanged; `collect::<Result<Vec<_>>>()` stops on
  the first parse error.
- Floating point: N/A.
- RNG/hash order: N/A.
- Observable side effects: none.
- Type narrowing: compile-time type inference selects the same target types.

## Verification

- `ubs src/cli/commands/count.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib test_parse_count_filters_trim_delimited_whitespace` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib cli::commands::count::tests` passed.
