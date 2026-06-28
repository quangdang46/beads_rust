# Count Default Grouping Perf Slice

Baseline binary: `/data/tmp/br_72yf32_candidate_br`

Candidate binary: `/data/tmp/br_72yf33_count_candidate_br`

Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Change: default-visible `count --by status|priority|type|assignee` now uses the lean stats-row scan/grouping path instead of fsqlite aggregate `GROUP BY` queries. Label grouping remains on the existing label-specialized path.

Behavior proof: before/after JSON outputs in this directory are byte-identical for default status, priority, type, assignee, label, and include-closed control cases.

Timing summary from `hyperfine.json`:

| Command | Before | After | Speedup |
| --- | ---: | ---: | ---: |
| `count --by status --json` | 95.1 ms | 52.5 ms | 1.81x |
| `count --by priority --json` | 99.0 ms | 53.8 ms | 1.88x |
| `count --by type --json` | 95.2 ms | 53.8 ms | 1.81x |
| `count --by assignee --json` | 105.3 ms | 53.7 ms | 2.00x |
| `count --by label --json` | 123.6 ms | 122.5 ms | unchanged control |
