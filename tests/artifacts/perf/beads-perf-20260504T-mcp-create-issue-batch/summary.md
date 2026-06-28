# MCP `create_issue` batch mutation proof

Date: 2026-05-04
Bead: `beads_rust-72yf.13`
Agent: `SilverCreek`

## Goal

Apply a flat-combining style MCP issue-creation path for swarm agents that need
to create many planning or triage issues while decomposing work. The batch mode
keeps the existing single-issue `create_issue` response unchanged, while opt-in
`issues[]` uses one mutation envelope and one response envelope for ordered
issue creation.

## Change

- `create_issue` still accepts legacy single-issue arguments with `title`,
  `description`, `type`, `priority`, `assignee`, `labels`, and `parent`.
- `create_issue` now also accepts `issues[]`.
- Batch output is `{items,count,ok_count,error_count}`.
- Successful items return `{index,title,id,ok:true,result}` where `result` is
  the legacy single-create JSON shape.
- Failed items return `{index,title,ok:false,error}` with the shared MCP
  structured error payload.
- Partial failures do not fail the whole batch.
- Batch issue creation is evaluated in input order, so child-number generation
  and collision checks see earlier successful creates in the same batch.

## Alien-artifact / graveyard contract

- Primitive: flat combining / write combining under a contended issue-creation
  path.
- Queueing target: reduce MCP issue-create storage-open and auto-flush service
  count from `N` to `1` for a batch, shrinking queueing and synchronization
  components of tail latency.
- Fallback: clients that do not send `issues[]` keep using existing
  single-issue calls and receive the unchanged legacy response shape.
- Exhaustion guard: `issues[]` is capped at 100 issue objects per call.
- Failure policy: malformed batch envelopes fail before mutation; per-item
  title, label, parent, validation, or storage failures are returned in item
  errors while other valid items continue.

## Isomorphism proof

- Ordering preserved: yes. Batch item order is input order with stable `index`.
- Tie-breaking unchanged: yes. Hash IDs and child IDs use the same generator and
  same storage collision checks as legacy single calls.
- Floating-point: N/A.
- RNG seeds: N/A.
- Legacy behavior: `create_issue_legacy_single_result_shape_is_unchanged`
  proves single-create calls do not grow `items` or `count` fields.
- Mutation proof: `create_issue_batch_returns_ordered_items_partial_errors_and_flushes`
  proves ordered per-item success/error records, type/priority coercions, label
  writes, parent-child dependency creation, missing-parent failure, and JSONL
  auto-flush evidence.
- Safety proof: `create_issue_batch_rejects_ambiguous_single_and_batch_args`
  proves clients cannot ambiguously mix single and batch modes.

## Measurement

Command:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_silvercreek_create_batch \
  cargo test --features mcp mcp_create_issue_batch_perf_probe -- --ignored --nocapture
```

Output:

```json
{"issues":25,"iterations":5,"repeated_single_total_ns":4834787858,"batch_total_ns":567303524,"speedup":8.522400537741063,"last_batch_ok_count":25,"equality":"batch issue creates verified by final storage count and per-item ok counts"}
```

## Focused verification

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_silvercreek_create_batch \
  cargo test --features mcp create_issue -- --nocapture
```

Result: passed. Focused MCP coverage included the new batch tests plus the
existing create-command and MCP create regressions selected by the filter.
