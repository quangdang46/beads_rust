# br stats summary fast path - 2026-05-04

Workload: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary: `/data/tmp/br_72yf33_count_candidate_br`

Candidate binary: `/data/tmp/br-target-stats-summary/release/br`

## Behavior proof

`baseline-stats.json` and `candidate-stats.json` are byte-identical for:

- `br stats --json`
- `br stats --no-activity --json`

## Timing

Primary paired run: `hyperfine.json`

- `stats --no-activity --json`: 144.3 ms noisy baseline -> 125.2 ms
- `stats --json`: noisy because the default activity path adds independent
  filesystem/git/cache variance; see `hyperfine-stats-json-rerun.json`

Focused no-activity rerun: `hyperfine-no-activity-rerun.json`

- `stats --no-activity --json`: 131.1 ms -> 124.7 ms

## No-go recorded during the pass

An aggregate multi-query version was tested first and rejected: it made
`stats --json` slower because several small fsqlite queries cost more than one
narrow scan. The committed candidate keeps the original one-scan summary
algorithm and only skips hydrating priority/assignee columns when no breakdown
dimension is requested.
