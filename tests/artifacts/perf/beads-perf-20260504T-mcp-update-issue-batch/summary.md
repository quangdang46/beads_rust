# MCP `update_issue` batch mutation proof

Date: 2026-05-04
Bead: `beads_rust-72yf.13`
Agent: `YellowHare`

## Goal

Apply a flat-combining style MCP mutation path for swarm agents that need to
update many issues at once. The batch mode keeps the existing single-item
`update_issue` response unchanged, while opt-in `updates[]` uses one write lock,
one storage open, and one auto-flush for the batch.

## Change

- `update_issue` still accepts legacy single-item arguments with `id`.
- `update_issue` now also accepts `updates[]`.
- Batch output is `{items,count,ok_count,error_count}`.
- Successful items return `{index,id,ok:true,result}` where `result` is the
  legacy single-update JSON shape.
- Failed items return `{index,id,ok:false,error}` with the same MCP structured
  error payload used by other batch read surfaces.
- Partial failures do not fail the whole batch.

## Alien-artifact / graveyard contract

- Primitive: flat combining / write combining under a contended mutation path.
- Queueing target: reduce lock-open-flush service count from `N` to `1` for a
  batch, shrinking both queueing and synchronization components of tail latency.
- Fallback: clients that do not send `updates[]` keep using existing single-item
  `id` calls and receive the unchanged legacy response shape.
- Exhaustion guard: `updates[]` is capped at 100 items per call.
- Failure policy: malformed batch envelopes fail before mutation; per-item
  validation/existence/comment failures are returned in item errors while other
  valid items continue.

## Isomorphism proof

- Ordering preserved: yes. Batch item order is input order with stable `index`.
- Tie-breaking unchanged: N/A. Each update targets an explicit issue ID.
- Floating-point: N/A.
- RNG seeds: N/A.
- Legacy behavior: `update_issue_legacy_single_result_shape_is_unchanged` proves
  single-item calls do not grow `items` or `count` fields.
- Mutation proof: `update_issue_batch_returns_ordered_items_partial_errors_and_flushes`
  proves ordered per-item success/error records, actor/comment side effects,
  label side effects, status coercion, and JSONL auto-flush.
- Safety proof: `update_issue_batch_rejects_invalid_comment_before_item_field_mutation`
  proves an invalid per-item comment does not apply that item's field update.

## Measurement

Command:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_yellowhare_update_batch \
  cargo test --features mcp mcp_update_issue_batch_perf_probe -- --ignored --nocapture
```

Output:

```json
{"issues":25,"iterations":5,"repeated_single_total_ns":4715556694,"batch_total_ns":659323772,"speedup":7.152110835767044,"last_batch_ok_count":25,"equality":"batch updates verified by final storage state and per-item ok counts"}
```

## Focused verification

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_yellowhare_update_batch \
  cargo test --features mcp update_issue -- --nocapture
```

Result: passed. Focused MCP coverage included the new batch tests plus existing
single-update validation and storage update tests.
