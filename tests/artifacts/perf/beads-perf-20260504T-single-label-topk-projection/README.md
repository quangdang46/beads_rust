# Single-Label Top-K Projection Probe

Bead: `beads_rust-72yf.24`

Dataset: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary: `/data/tmp/br-baseline-single-label-page-20260504-before-72yf23`

Candidate binary: `/data/tmp/br-target-list-query-release/release/br`

Probe idea: for default first-page queries with exactly one label, query only a
lean projection (`id`, `status`, `priority`, `created_at`, `is_template`) for
the label candidate IDs, filter default-visible rows, sort by the canonical
default order (`priority ASC`, `created_at DESC`, `id ASC`), select top K IDs,
then hydrate only those full issue rows.

This was intended to avoid the full-hydration no-go from
`beads_rust-72yf.23`, while preserving exact ordering and totals.

Quick wall-clock result:

| Command | Baseline | Candidate | Outcome |
| --- | ---: | ---: | --- |
| `list --limit 50 --json --label export` | 3.43 s | 6.46 s | rejected |
| `list --limit 50 --json --label lane-00` | not rerun | 0.28 s | inconclusive |

Decision: do not ship. Chunked `id IN (...)` projection still loses to the
current `aa3ef353`/`8b758717` page path for the high-cardinality `export`
label. The experimental code was removed.
