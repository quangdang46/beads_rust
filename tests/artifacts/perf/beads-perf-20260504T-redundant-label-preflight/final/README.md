# Redundant Label Preflight No-Go

Bead: `beads_rust-72yf.27`

Control: `c49b89b3` (`perf(labels): skip redundant broad label filters`)

Candidate: preflight broad-label redundancy proof before candidate materialization.

Dataset: `/data/tmp/br-read-matrix-20260504-aTl0u9`

## Result

No source changes were retained.

The retargeted preflight avoided the first attempted anti-join proof, but the measured win on broad covering labels was too small and narrow-label queries regressed.

Broad label `export`:

| Row | Control | Candidate | Result |
| --- | ---: | ---: | ---: |
| `list --limit 50 --json --label export` | 302.1 ms | 295.1 ms | 1.02x faster |
| `search payload --json --label export` | 309.7 ms | 298.1 ms | 1.04x faster |

Narrow label `lane-00` guardrail:

| Row | Control | Candidate | Result |
| --- | ---: | ---: | ---: |
| `list --limit 50 --json --label lane-00` | 243.5 ms | 256.8 ms | regressed |
| `search payload --json --label lane-00` | 214.1 ms | 235.9 ms | regressed |

## Files

- `broad-label-hyperfine.md` / `.json`: broad `export` label benchmark.
- `narrow-label-hyperfine.md` / `.json`: narrow `lane-00` guardrail benchmark.
