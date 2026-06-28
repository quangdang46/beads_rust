# Swarm Capacity Planning Report - 2026-05-04

This report is the pilot capacity artifact for `beads_rust-72yf.41`.
It is derived from the NUMA/high-core read-command profile in
`tests/artifacts/perf/beads-perf-20260504T-numa-read-command-profile/` and the
sequential drift-guard contract in `src/policy.rs`.

## Inputs

- Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`
- Issues: 12,000
- Sync state: `dirty_count = 0`, DB and JSONL in sync
- Doctor: `ok = true`; one expected frankensqlite WAL-sidecar warning
- Host class: 64 physical cores, 128 logical CPUs, about 512 GB RAM, one NUMA node
- Binary: `br 0.2.5` release profile from the NUMA profile bundle

## Read-Command Evidence

Default-scheduler p95 readings on the 12k corpus:

| Command | P95 ms |
| --- | ---: |
| `list --json --limit 100` | 198 |
| `ready --json --limit 100` | 151 |
| `scheduler --json --candidate-limit 100` | 235 |
| `search agent --json --limit 100` | 76 |
| `stats --no-activity --json` | 134 |
| `label list-all --json` | 85 |

The report model uses this br-heavy read mix:

| Command family | Weight |
| --- | ---: |
| list | 0.30 |
| ready | 0.25 |
| scheduler | 0.15 |
| search | 0.10 |
| stats | 0.10 |
| label list-all | 0.10 |

Weighted p95 is about 162 ms. The raw CPU-service upper bound on a 64-core host
is about 395 mixed read commands per second, but the recommended bands below are
intentionally much lower because the pilot does not include live write-lock
contention or cross-node NUMA evidence.

## Recommended Bands

For this high-core host class, assuming each agent runs at most one br read
command per second and commands use `--no-auto-import --no-auto-flush`:

| Band | Agents | Recommendation |
| --- | ---: | --- |
| Green | 1-64 | Safe for read-heavy swarms while `dirty_count` stays 0 and p95 remains within budget. |
| Yellow | 65-128 | Acceptable for bursts or monitored runs; rerun the profile when corpus or p95 changes. |
| Red | >128 | Do not treat as safe without contention replay and write-lock evidence. |

For laptops and small VMs:

| Band | Agents | Recommendation |
| --- | ---: | --- |
| Green | 1-4 | Keep sync explicit and avoid high-frequency scheduler/list polling. |
| Yellow | 5-8 | Expect visible p95 jitter; batch status checks where possible. |
| Red | >8 | Move br-heavy swarm work to a high-core host or reduce command cadence. |

## Invalidation Rules

Rerun this report when:

- `dirty_count` is nonzero before the run;
- issue count or JSONL hash changes materially;
- weighted read p95 regresses by more than 5 percent;
- candidate command errors exceed baseline errors;
- a host has more than one NUMA node and no cross-node profile exists.

The sequential drift guard added for `beads_rust-72yf.40` is the machine-readable
mechanism for disabling adaptive recommendations when those evidence conditions
are no longer true.

## Confidence

Confidence is medium for read-heavy no-auto-import/no-auto-flush workloads on
single-node high-core hosts. Confidence is low for write-heavy swarms until this
report is paired with `bench_contention_replay` lock-wait traces.
