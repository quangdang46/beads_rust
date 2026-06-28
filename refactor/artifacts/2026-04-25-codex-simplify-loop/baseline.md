# Baseline

Run started from `main` at the commit recorded in `head_before.txt`.

## Green Gates Before Refactor

- `cargo fmt --check`
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo check --all-targets`
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo clippy --all-targets -- -D warnings`
- `rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_magentalotus_simplify cargo test --all-targets --no-fail-fast`

## Golden Fixture

Stable CLI read outputs are captured in `goldens/` and hashed by `golden.sha256`.
The fixture workspace is `golden_workspace/`; verification runs should write new
outputs elsewhere and compare against the baseline hashes instead of mutating the
baseline files. `goldens/export.json` documents an initial skipped command and is
not part of the replay hash.
