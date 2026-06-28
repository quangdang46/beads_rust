# Redundant Broad Label Filter Fast Path

Bead: `beads_rust-72yf.26`

Control: `d824a117` (`perf(search): speed label-filtered queries`)

Candidate: working tree release build from `/data/tmp/br_72yf26_local_target/release/br`

Dataset: `/data/tmp/br-read-matrix-20260504-aTl0u9`

## Result

Default-visible single-label list/search now bypasses the label membership filter when the label candidate set is broad enough and an exact coverage check proves the label covers the whole default-visible issue universe.

Broad label `export`:

| Row | Control | Candidate | Speedup |
| --- | ---: | ---: | ---: |
| `list --limit 50 --json --label export` | 3.223 s | 0.299 s | 10.80x |
| `search payload --json --label export` | 3.141 s | 0.310 s | 10.52x |

Narrow label `lane-00` guardrail:

| Row | Control | Candidate | Result |
| --- | ---: | ---: | ---: |
| `list --limit 50 --json --label lane-00` | 240.8 ms | 236.8 ms | no regression |
| `search payload --json --label lane-00` | 221.1 ms | 219.3 ms | no regression |

## Behavior Proof

Golden output hashes match for broad and narrow label list/search cases; see `golden-sha256.txt`.

Focused regression:

```text
cargo test test_redundant_single_label_fast_path_preserves_list_and_search_results -- --nocapture
1 passed
```

## Files

- `broad-label-hyperfine.md` / `.json`: broad `export` label benchmark.
- `narrow-label-hyperfine.md` / `.json`: narrow `lane-00` guardrail benchmark.
- `*-baseline.json` / `*-candidate.json`: golden command outputs for equality proof.
- `golden-sha256.txt`: paired SHA-256 hashes for golden outputs.
