# Ready Limited-Window Hydration

Date: 2026-05-03

Workload: `/tmp/br-blocked-projection-real2-tTf4Nx`

Baseline binary:

- `/data/tmp/br-f469921e-baseline`
- commit `f469921e`

Candidate binary:

- `/data/tmp/br-ready-window-candidate`
- source: `1c575f22` plus the uncommitted `src/storage/sqlite.rs` limited-window cache-filter patch

## Output Parity

All checks used `--no-auto-import --no-auto-flush`.

| Command | Bytes | SHA-256 | Parity |
| --- | ---: | --- | --- |
| `ready --limit 20 --format text` | 958 | `d84ed10c2032e3dba548e312c88fb63bea40e33ba3404fbee95a2ba320a19499` | match |
| `ready --limit 0 --format text` | 286942 | `7919e4ed10b9ace83dd62e42bd40c58bc4439cd436120ea093f2134488b762e7` | match |
| `ready --limit 20 --json` | 46242 | `5a1f54bf948a17d04ffa80fbba0a5c0102257e996a6a40f848234be7091ff93c` | match |
| `ready --limit 0 --json` | 13872002 | `da69580faf2ba19d9bec96a1ef90c9dea2dab5e06d2657dbb580b2453784fe50` | match |

## Rejected Probe

`ready-window-old-vs-summary.json` records the first cold-column-only projection
probe. It preserved output but did not move the workload:

- old `ready --limit 20 --format text`: 3.107s mean
- summary-only candidate: 3.210s mean
- old `ready --limit 0 --format text`: 3.239s mean
- summary-only candidate: 3.229s mean

The bottleneck was the `blocked_issues_cache` anti-join, not the cold fields.

## Accepted Probe

`ready-limited-hybrid-cache-filter-old-vs-window.json` records the accepted
candidate. The limited hybrid path now:

1. reads compact high-bucket candidates without the SQL anti-join,
2. filters them against the blocked-cache IDs in memory,
3. hydrates only the final visible ID window at the requested projection,
4. falls back to the previous global query if the high bucket cannot satisfy the
   requested limit.

Measured means:

| Command | Baseline | Candidate | Speedup |
| --- | ---: | ---: | ---: |
| `ready --limit 20 --format text` | 3.137s | 0.300s | 10.46x |
| `ready --limit 20 --json` | 3.079s | 0.326s | 9.43x |

Candidate `/usr/bin/time -v` for `ready --limit 20 --format text`:

- elapsed: 0.31s
- user: 0.18s
- system: 0.13s
- max RSS: 199748 KB
