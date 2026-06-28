# MCP `list_issues` batch read proof

Date: 2026-05-04
Bead: `beads_rust-72yf.13`
Agent: `SilverCreek`

## Goal

Apply the same flat-combining style read envelope used by the other MCP batch
tools to filtered backlog exploration. Swarm agents often need several
filtered list views at once, such as open work, assigned work, priority buckets,
and search/title probes. The new opt-in `queries[]` path keeps the legacy
single-filter `list_issues` response unchanged while evaluating multiple list
queries with one read storage open and one response envelope.

## Change

- `list_issues` still accepts legacy single-filter arguments: `status`, `type`,
  `priority`, `assignee`, `labels`, `title`, `search`, `include_closed`,
  `limit`, and `sort`.
- `list_issues` now also accepts `queries[]`.
- Batch output is `{items,count,ok_count,error_count}`.
- Successful items return `{index,query,ok:true,result}` where `result` is the
  legacy single-list JSON shape.
- Failed items return `{index,query,ok:false,error}` with the shared MCP
  structured error payload.
- Partial failures do not fail the whole batch.
- Batch list queries are evaluated in input order, and each query preserves the
  same filter coercions and result ordering as a legacy single call.

## Alien-artifact / graveyard contract

- Primitive: flat combining / read batching on a high-frequency MCP discovery
  path.
- Queueing target: reduce MCP list-query storage-open service count from `N` to
  `1` for a batch, shrinking queueing, I/O, and synchronization components of
  tail latency.
- Fallback: clients that do not send `queries[]` keep using existing single-list
  calls and receive the unchanged legacy response shape.
- Exhaustion guard: `queries[]` is capped at 100 filter objects per call.
- Failure policy: malformed batch envelopes fail before any read; malformed
  per-item filter objects produce item errors while other valid queries
  continue.

## Isomorphism proof

- Ordering preserved: yes. Batch item order is input order with stable `index`;
  each item delegates to the same `list_issues_json` implementation.
- Tie-breaking unchanged: yes. Per-query sort, ID tie-breakers, and list/search
  storage behavior are unchanged.
- Floating-point: N/A.
- RNG seeds: N/A.
- Legacy behavior: `list_issues_legacy_single_result_shape_is_unchanged` proves
  single-list calls do not grow `items`, `ok_count`, or `error_count` fields.
- Batch proof: `list_issues_batch_returns_ordered_items_partial_errors_and_coercions`
  proves ordered per-item success/error records, filter coercions, and partial
  failure visibility.
- Safety proof: `list_issues_batch_rejects_ambiguous_single_and_batch_args`
  proves clients cannot ambiguously mix single-filter and batch modes.
- Freshness proof: `list_issues_batch_snapshot_matches_direct_json_and_invalidates`
  proves the opt-in read snapshot cache still matches direct JSON and invalidates
  on witness changes.

## Measurement

Command:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_silvercreek_list_batch \
  cargo test --features mcp mcp_list_issues_batch_perf_probe -- --ignored --nocapture
```

Output:

```json
{"queries":25,"seeded_issues":150,"iterations":5,"repeated_single_total_ns":3885900175,"batch_total_ns":340505580,"speedup":11.4121482972467,"equality":"batch envelope matches repeated single-query list_issues JSON"}
```

## Focused verification

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_silvercreek_list_batch \
  cargo test --features mcp list_issues -- --nocapture
```

Result: passed. Focused MCP coverage included legacy single-list shape, batch
partial failures, ambiguous-mode rejection, snapshot invalidation, and the
ignored perf probe selection.
