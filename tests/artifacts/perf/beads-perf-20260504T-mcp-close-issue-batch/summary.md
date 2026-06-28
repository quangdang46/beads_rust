# MCP `close_issue` batch mutation proof

Date: 2026-05-04
Bead: `beads_rust-72yf.13`
Agent: `TanLeopard`

## Goal

Apply a flat-combining style MCP mutation path for swarm agents that complete
many issues in a burst. The batch mode keeps the existing single-item
`close_issue` response unchanged, while opt-in `ids[]` uses one write lock, one
storage open, and one auto-flush for the batch.

## Change

- `close_issue` still accepts legacy single-item arguments with `id`.
- `close_issue` now also accepts `ids[]`.
- Batch output is `{items,count,ok_count,error_count}`.
- Successful items return `{index,id,ok:true,result}` where `result` is the
  legacy single-close JSON shape.
- Failed items return `{index,id,ok:false,error}` with the same MCP structured
  error payload used by the other batch surfaces.
- Partial failures do not fail the whole batch.
- Batch `reason` is shared across all items in the call.

## Alien-artifact / graveyard contract

- Primitive: flat combining / write combining under a contended mutation path.
- Queueing target: reduce lock-open-flush service count from `N` to `1` for a
  batch, shrinking both queueing and synchronization components of tail latency.
- Fallback: clients that do not send `ids[]` keep using existing single-item
  `id` calls and receive the unchanged legacy response shape.
- Exhaustion guard: `ids[]` is capped at 100 issue IDs per call.
- Failure policy: malformed batch envelopes fail before mutation; per-item
  placeholder, missing, tombstone, or storage failures are returned in item
  errors while other valid items continue.

## Isomorphism proof

- Ordering preserved: yes. Batch item order is input order with stable `index`.
- Tie-breaking unchanged: N/A. Each close targets an explicit issue ID.
- Floating-point: N/A.
- RNG seeds: N/A.
- Legacy behavior: `close_issue_legacy_single_result_shape_is_unchanged` proves
  single-item calls do not grow `items` or `count` fields.
- Mutation proof: `close_issue_batch_returns_ordered_items_partial_errors_and_flushes`
  proves ordered per-item success/error records, dependent-unblocked metadata,
  close status/reason persistence, and JSONL auto-flush.
- Safety proof: `close_issue_batch_rejects_ambiguous_single_and_batch_args`
  proves clients cannot ambiguously mix single and batch modes.

## Measurement

Command:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_tanleopard_close_batch \
  cargo test --features mcp mcp_close_issue_batch_perf_probe -- --ignored --nocapture
```

Output:

```json
{"issues":25,"iterations":5,"repeated_single_total_ns":8143732168,"batch_total_ns":1697059754,"speedup":4.798730362207388,"last_batch_ok_count":25,"equality":"batch closes verified by final storage state and per-item ok counts"}
```

## Focused verification

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_tanleopard_close_batch \
  cargo test --features mcp close_issue -- --nocapture
```

Result: passed. Focused MCP coverage included the new batch tests plus existing
close conformance filters.
