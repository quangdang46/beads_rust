# List Page TOON Streaming

Date: 2026-05-04
Agent: PinkTiger
Bead: beads_rust-72yf.22

## Hypothesis

`list --limit 0 --format toon` spent avoidable CPU and allocation work by
materializing the full `ListPage` as `serde_json::Value` and `toon_rust::JsonValue`
before encoding. A specialized list-page TOON writer can preserve the current
nested TOON bytes for `IssueWithCounts` rows without dependency/comment payloads,
including labels, while falling back to the generic encoder for stats mode and
unsupported relation payloads.

## Workload

Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`

Command:

```bash
br list --limit 0 --format toon
```

Baseline binary:

```text
/data/tmp/br-candidate-scheduler-evidence-loading-20260504
```

Candidate binary:

```text
/data/tmp/br-candidate-list-page-toon-stream-20260504
```

## Correctness

The full TOON output was byte-identical.

```text
e09e19235b3fd1b0a3956724a4bbd79c8586198e5419b69b5ef0423a2f4244a7  baseline
e09e19235b3fd1b0a3956724a4bbd79c8586198e5419b69b5ef0423a2f4244a7  candidate
```

Both outputs were 7,308,063 bytes.

Focused unit proof:

```bash
rch exec -- env CARGO_TARGET_DIR=/data/tmp/beads-rust-target-list-page-toon-stream-20260504 \
  cargo test output::context::tests::write_toon_list_page_to_writer_matches_materialized_encode_output --lib -- --nocapture
```

## Timing

Hyperfine, 3 warmups and 10 runs:

```text
baseline:  303.8 ms +/- 8.9 ms
candidate: 218.0 ms +/- 7.6 ms
speedup:   1.39x +/- 0.06x
```

The raw hyperfine JSON is stored in `hyperfine.json`.

## Decision

Accepted. The optimization preserves full output bytes and removes about 85.8 ms
from the 12k-issue full-list TOON path.
