# TOON length-pass probe (rejected)

Bead: `beads_rust-72yf.21`

Hypothesis: `toon_with_stats` computes encoded TOON line lengths even when
stats are disabled. Moving that length pass behind the stats branch should
preserve bytes exactly and reduce large TOON output latency.

Result: rejected. The output was byte-for-byte identical, but timing did not
improve.

## Semantic check

Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary:
`/data/tmp/br-candidate-scheduler-evidence-loading-20260504`

Candidate binary:
`/data/tmp/br-candidate-toon-length-pass-20260504`

Command:

```bash
list --limit 0 --format toon
```

Output SHA-256:

```text
e09e19235b3fd1b0a3956724a4bbd79c8586198e5419b69b5ef0423a2f4244a7  baseline
e09e19235b3fd1b0a3956724a4bbd79c8586198e5419b69b5ef0423a2f4244a7  candidate
```

## Timing

```text
baseline:  312.9 ms +/- 10.7 ms
candidate: 317.6 ms +/-  8.9 ms
```

Hyperfine summary:

```text
baseline ran 1.02 +/- 0.04 times faster than candidate
```

Conclusion: the extra line-length pass is not a meaningful bottleneck for this
workload. The source change was reverted; use `hyperfine.json` as the raw proof.
