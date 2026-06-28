# Pass 7: Completion Issue-Type Candidates

## Change

Collapsed duplicated issue-type completion construction in `src/cli/mod.rs`
into `issue_type_candidates`, then reused it from both the plain and
comma-delimited issue-type completers.

## Equivalence Contract

- Standard candidate ordering preserved: the helper still starts with
  `static_candidates(prefix, ISSUE_TYPE_CANDIDATES)`.
- Standard help text preserved: standard issue types still come from the same
  static candidate table.
- Custom candidate ordering preserved: custom issue types still iterate
  `completion_index().types` in existing order.
- Standard/custom de-dup preserved: custom types still skip
  `issue_type_is_standard` using the same case-insensitive predicate.
- Prefix filtering preserved: custom types still use
  `matches_prefix_case_insensitive`.
- Delimited prefixing preserved: the delimited completer still calls
  `split_delimited_prefix` and applies `add_prefix` to every candidate after
  construction.
- Command parsing and completion registration: untouched.

## Verification

- `ubs src/cli/mod.rs` exited 0.
- `cargo fmt --check` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --lib cli::tests::test_issue_type_delimited_completion_preserves_plain_candidate_order` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets` passed.
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings` passed.
