# Scheduler materialization probe (rejected)

Bead: `beads_rust-72yf.19`

Hypothesis: deferring `ReadyIssue` conversion and rationale construction until after
score/sort/truncate would reduce scheduler per-candidate CPU/allocation cost for a
large ready queue.

Result: rejected. Output semantics were preserved, but timing did not improve.

## Semantic check

Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary:
`/data/tmp/br-candidate-scheduler-evidence-batch-20260504`

Candidate binary:
`/data/tmp/br-candidate-scheduler-materialization-20260504`

Command:

```bash
scheduler --json --candidate-limit 512 --limit 20
```

Normalization:

```bash
jq 'del(.generated_at)'
```

Normalized SHA-256:

```text
698a15907f68b51cb29734cbaf2246ce0b9790b06052cf3f1701d45173a58233  baseline
698a15907f68b51cb29734cbaf2246ce0b9790b06052cf3f1701d45173a58233  candidate
```

## Timing

```text
baseline:  893.2 ms +/- 18.6 ms
candidate: 903.6 ms +/- 13.1 ms
```

Hyperfine summary:

```text
baseline ran 1.01 +/- 0.03 times faster than candidate
```

Conclusion: scheduler cost at this workload is not dominated by JSON
recommendation materialization. Do not ship this source change; keep searching
for a storage/scoring lever with a real measured win.
