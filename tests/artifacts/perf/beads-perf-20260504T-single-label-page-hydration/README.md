# Single-Label Page Hydration Probe

Bead: `beads_rust-72yf.23`

Dataset: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline: `aa3ef353` built as
`/data/tmp/br-baseline-single-label-page-20260504-before-72yf23`

Candidate probe: local working-tree build at
`/data/tmp/br-target-list-query-release/release/br`

Probe idea: for default first-page queries with exactly one label, reuse the
materialized label candidate IDs, hydrate candidate issues in bounded chunks,
filter default-visible rows in Rust, sort by the canonical default order, and
truncate to the requested page. This would avoid a large `id IN (...)` page
query if full hydration plus Rust sorting were cheaper.

Quick wall-clock result:

| Command | Baseline | Candidate | Outcome |
| --- | ---: | ---: | --- |
| `list --limit 50 --json --label export` | 3.41 s | 6.19 s | rejected |
| `list --limit 50 --no-color --label export` | not rerun | 6.13 s | rejected |

Decision: do not ship. Full candidate issue hydration is slower than the
post-`aa3ef353` SQL page path for the high-cardinality `export` label.
