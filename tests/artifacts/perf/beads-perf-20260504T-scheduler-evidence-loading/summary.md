# Scheduler evidence-loading large-window win

Bead: `beads_rust-72yf.20`

Change: for scheduler candidate windows with at least 256 issue IDs, load the
existing full list relation metadata map and project it onto the scheduler
candidate set. Smaller windows keep the targeted batched label and relation
count queries.

Why: the previous 512-ID `IN (...) GROUP BY` evidence probes dominated
scheduler CPU on the large read matrix. The full metadata scan is already used
by structured list output and is much faster for broad scheduler windows.

## Semantic check

Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Baseline binary:
`/data/tmp/br-candidate-scheduler-evidence-batch-20260504`

Candidate binary:
`/data/tmp/br-candidate-scheduler-evidence-loading-20260504`

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
be9655a0515db73b73207a9d2f79b19002d705240bdea3a0b6237d18cdd97e0b  baseline
be9655a0515db73b73207a9d2f79b19002d705240bdea3a0b6237d18cdd97e0b  candidate
```

## Main timing

Command:

```bash
scheduler --json --candidate-limit 512 --limit 20 >/dev/null
```

```text
baseline:  924.1 ms +/- 12.2 ms
candidate: 225.4 ms +/-  5.8 ms
speedup:   4.10x +/- 0.12
```

Raw hyperfine JSON: `hyperfine.json`

## Small-window guard

Command:

```bash
scheduler --json --candidate-limit 64 --limit 20 >/dev/null
```

```text
baseline:  265.4 ms +/- 4.8 ms
candidate: 261.7 ms +/- 4.0 ms
speedup:   1.01x +/- 0.02
```

Raw hyperfine JSON: `small-window-hyperfine.json`

## Unlimited candidate guard

Command:

```bash
scheduler --json --candidate-limit 0 --limit 20 >/dev/null
```

Candidate single run:

```text
elapsed 0.27 user 0.21 sys 0.05
```

Previous same-workspace baseline from the pre-change diagnostic run was about
17.396 seconds for the same unlimited-candidate command, so the full metadata
path removes the pathological large-`IN` evidence-loading cost.
