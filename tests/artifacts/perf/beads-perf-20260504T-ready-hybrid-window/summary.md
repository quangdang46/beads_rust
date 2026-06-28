# Ready hybrid window optimization

Date: 2026-05-04
Bead: beads_rust-72yf.17

## Corpus

- Workspace: `/data/tmp/br-read-matrix-20260504-aTl0u9`
- Source corpus: `/data/tmp/br-parallel-export-work-20260504T0628/.beads/issues.jsonl`
- Shape: 12,000 visible issues, 36,000 label rows

## Change

For limited hybrid ready queries with a healthy blocked cache, the high-priority
window now applies `LIMIT` in SQL with the requested projection. The previous
path loaded the entire high-priority bucket as summary rows, filtered blocked
IDs, truncated, then hydrated the visible JSON/TOON rows with a second query.

The existing fallback remains in place when the high-priority bucket has fewer
rows than the requested page.

## Proof

Output equality:

```text
f28b669c624d396c66819e3af1ab67cf6e5fbeb5d4af201094ed439ff4c99b13  ready-baseline.json
f28b669c624d396c66819e3af1ab67cf6e5fbeb5d4af201094ed439ff4c99b13  ready-candidate.json
```

Timing:

```text
baseline:  152.2 ms +/- 4.6 ms
candidate: 132.4 ms +/- 3.8 ms
speedup:   1.15x
```

Focused behavior tests:

```text
cargo test limited_ready_hybrid --lib -- --nocapture
```
