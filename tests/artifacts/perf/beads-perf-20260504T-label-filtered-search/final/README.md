# Label-filtered search optimization

Bead: `beads_rust-72yf.25`

Change: `search_issues_with_projection` now materializes label candidate IDs once, then filters `issues.id` membership, instead of adding correlated label `EXISTS` predicates to every searched issue row.

Corpus: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary:
`/data/tmp/br-baseline-single-label-page-20260504-before-72yf23`

Current binary:
`/data/tmp/br_72yf25_local_target/release/br`

Measured command:
`br search payload --json --label export >/dev/null`

Results:
- Baseline: did not complete within the 20s bound; earlier combined hyperfine attempt also made no progress after more than 90s on this row.
- Current: 3.092s mean +/- 0.046s over 5 runs.
- Conservative improvement: at least 6.47x versus the 20s bound.
- Current unfiltered search guardrail: `search payload --json` remains fast at 208.8ms mean +/- 2.1ms.

Behavior proof:
- Inline unit test: `test_search_issues_materialized_label_candidates_preserve_semantics`.
- Small fixture byte equality:
  - `small-baseline-label-export.json` equals `small-current-label-export.json`.
  - `small-baseline-label-and.json` equals `small-current-label-and.json`.
  - `small-baseline-label-any.json` equals `small-current-label-any.json`.

Large output hash:
`current-large-label-export.json` sha256 `2953a7defa431547a9bb3acb7504405f1a3ba3c68b969c1c87ef70cfd348b92b`.
