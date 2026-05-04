# Scheduler evidence batch optimization

Date: 2026-05-04
Bead: beads_rust-72yf.18

## Corpus

- Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`
- Source corpus: `/data/tmp/br-parallel-export-work-20260504T0628/.beads/issues.jsonl`
- Shape: 12,000 visible issues, 36,000 label rows

## Baseline

After commit `3bf7ca1f`, the refreshed read matrix showed:

```text
scheduler --json:                         947.6 ms +/- 13.0 ms
ready --json --limit 512 --sort priority: 155.3 ms +/- 3.6 ms
scheduler --candidate-limit 20:           199.6 ms +/- 8.5 ms
```

The scheduler-specific overhead scales with the 512-candidate evidence window.

## Change

Increase the label and relation-count evidence batch chunk size from 200 to 900
IDs. This keeps the default scheduler candidate window in one evidence-loading
round trip per query while staying below SQLite's common 999-variable ceiling.

## Proof

Normalized output equality (`generated_at` removed):

```text
bd82aa1e2ec24f36429c84637f7f1a86d325cef831275ff13cfdc967391c2422  scheduler-baseline.normalized.json
bd82aa1e2ec24f36429c84637f7f1a86d325cef831275ff13cfdc967391c2422  scheduler-candidate.normalized.json
```

Timing:

```text
baseline:  936.1 ms +/- 19.0 ms
candidate: 903.7 ms +/- 10.0 ms
speedup:   1.04x
```

Focused tests:

```text
cargo test scheduler_evidence_helpers_handle_default_candidate_window --lib -- --nocapture
cargo test count_all_relation_counts_matches_chunked_counts --lib -- --nocapture
```

This is a small but isolated win; the remaining scheduler cost should be
profiled separately.
