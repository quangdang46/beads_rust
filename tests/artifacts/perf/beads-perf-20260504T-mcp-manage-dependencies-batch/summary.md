# MCP `manage_dependencies` batch mutation proof

Date: 2026-05-04
Bead: `beads_rust-72yf.13`
Agent: `SilverCreek`

## Goal

Apply a flat-combining style MCP graph-edit path for swarm agents that create
many dependency edges while operationalizing plans. The batch mode keeps the
existing single-operation `manage_dependencies` responses unchanged, while
opt-in `operations[]` uses one storage envelope and one response envelope for
ordered add/remove/list work.

## Change

- `manage_dependencies` still accepts legacy single-operation arguments with
  `action`, `id`, `depends_on`, and `dep_type`.
- `manage_dependencies` now also accepts `operations[]`.
- Batch output is `{items,count,ok_count,error_count}`.
- Successful items return `{index,action,id,ok:true,result}` where `result` is
  the legacy single-operation JSON shape for `add`, `remove`, or `list`.
- Failed items return `{index,action,id,ok:false,error}` with the shared MCP
  structured error payload.
- Partial failures do not fail the whole batch.
- Batch operations are evaluated in input order, so later graph operations see
  earlier successful edge mutations.

## Alien-artifact / graveyard contract

- Primitive: flat combining / write combining under a contended graph-mutation
  path.
- Queueing target: reduce MCP dependency-edit storage-open and auto-flush
  service count from `N` to `1` for a batch, shrinking queueing and
  synchronization components of tail latency.
- Fallback: clients that do not send `operations[]` keep using existing
  single-operation calls and receive the unchanged legacy response shapes.
- Exhaustion guard: `operations[]` is capped at 100 dependency operations per
  call.
- Failure policy: malformed batch envelopes fail before mutation; per-item
  placeholder, missing, cycle, or storage failures are returned in item errors
  while other valid items continue.

## Isomorphism proof

- Ordering preserved: yes. Batch item order is input order with stable `index`.
- Tie-breaking unchanged: yes. Each operation targets explicit issue IDs.
- Floating-point: N/A.
- RNG seeds: N/A.
- Legacy behavior: `manage_dependencies_legacy_single_result_shapes_are_unchanged`
  proves single-operation calls do not grow `items` or `count` fields.
- Mutation proof: `manage_dependencies_batch_returns_ordered_items_partial_errors_and_flushes`
  proves ordered per-item success/error records, sequential cycle detection
  after a prior successful batch edge, dependency removal, final list output,
  and JSONL auto-flush evidence.
- Safety proof: `manage_dependencies_batch_rejects_ambiguous_single_and_batch_args`
  proves clients cannot ambiguously mix single and batch modes.

## Measurement

Command:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_silvercreek_dep_batch \
  cargo test --features mcp mcp_manage_dependencies_batch_perf_probe -- --ignored --nocapture
```

Output:

```json
{"issues":25,"iterations":5,"repeated_single_total_ns":7647584004,"batch_total_ns":1012750212,"speedup":7.551303286224343,"last_batch_ok_count":25,"equality":"batch dependency adds verified by final storage state and per-item ok counts"}
```

## Focused verification

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_silvercreek_dep_batch \
  cargo test --features mcp manage_dependencies -- --nocapture
```

Result: passed. Focused MCP coverage included the new batch tests plus the
existing non-blocking reverse-link regression.
