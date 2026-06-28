# Broad Default List Tail Query No-Go

Bead: `beads_rust-72yf.31`

Date: 2026-05-04

## Goal

Test whether the default-visible first page path for broad `br list --limit ...`
queries could be faster by replacing the shipped priority-bucket loop with a
critical-bucket-plus-tail query shape:

- fetch priority `0` rows first
- if the page is not full, fetch `priority > 0` rows ordered by the same SQL
  sort key

The expected win was reducing repeated indexed probes for broad first pages
without changing visible list ordering.

## Proof

Focused behavior proof passed before the probe was rejected:

```bash
env CARGO_TARGET_DIR=/data/tmp/br_72yf31_local_target \
  cargo test test_list_issues_default_visible_limited_page_matches_sql_order -- --nocapture
```

The candidate was built as `/data/tmp/br_72yf31_candidate_br` and compared
against the shipped control binary `/data/tmp/br_72yf30_candidate_br` on the
large read matrix at `/data/tmp/br-read-matrix-20260504-aTl0u9`.

## Result

The candidate was flat within measurement noise:

| Command | Control | Candidate | Decision |
| --- | ---: | ---: | --- |
| `list --limit 50 --json` | 146.2 +/- 3.9 ms | 146.7 +/- 4.1 ms | flat |
| `list --limit 50 --format toon` | 150.4 +/- 4.0 ms | 149.9 +/- 2.0 ms | flat |
| `list --limit 1 --json` | 72.3 +/- 0.4 ms | 72.5 +/- 1.1 ms | flat |

See `candidate-hyperfine.md` and `candidate-hyperfine.json` for the measured
comparison. `baseline-hyperfine.md` records the pre-probe control baseline.

## Decision

Rejected. The focused ordering proof passed, but the measured result did not
beat the already-shipped priority-bucket implementation. The source change was
reverted; this artifact is retained so future broad-list optimization work does
not repeat the same query shape.
