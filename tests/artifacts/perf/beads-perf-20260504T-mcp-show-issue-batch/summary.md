# MCP `show_issue` Batch Read

Date: 2026-05-04

Bead: `beads_rust-72yf.13`

## Objective

Give agents a batch read path for known issue IDs without adding an eighth MCP
tool. The existing `show_issue` tool now accepts either:

- `id`: legacy single-issue mode with the unchanged response shape.
- `ids`: batch mode returning a per-item envelope.

Batch mode performs one read-storage open for the batch builder, preserves item
order, and reports partial failures as item-level errors instead of failing the
whole call after the input array itself has been validated.

## Opportunity Matrix

| Lever | Impact | Confidence | Effort | Score |
|---|---:|---:|---:|---:|
| Batch known-ID MCP issue reads through `show_issue` | 4 | 4 | 2 | 8.0 |

Alien-graveyard mapping: this is the batching/flat-combining shape applied at
the MCP envelope boundary. Many agent reads share one storage-open cost while
the fallback remains the original single-ID `show_issue` call.

Alien-artifact mapping: expected-loss policy is conservative. Invalid batch
shape fails fast; valid batches return all available issue details plus
structured per-item errors for missing, tombstoned, or placeholder IDs.

## Schema Contract

Batch output:

```json
{
  "items": [
    {"id": "br-a", "ok": true, "issue": {}},
    {"id": "br-missing", "ok": false, "error": {}}
  ],
  "count": 2,
  "ok_count": 1,
  "error_count": 1
}
```

Single-ID output is unchanged and still returns the issue details object
directly.

## Performance Probe

Command:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_dustypuma_mcp_batch \
  cargo test --features mcp mcp_show_issue_batch_perf_probe -- --ignored --nocapture
```

Result:

```json
{
  "issues": 25,
  "iterations": 10,
  "repeated_single_total_ns": 3940938537,
  "batch_total_ns": 453169054,
  "speedup": 8.696398181240328,
  "equality": "batch envelope matches repeated single-item issue JSON"
}
```

The probe constructs the same envelope from repeated single-ID reads and from
the batch builder, then asserts equality before reporting timings.

## Isomorphism Proof

- Single-ID behavior: unchanged; `{"id": "..."}` still returns the original
  issue details JSON object.
- Batch ordering: preserved by iterating `ids` in input order and appending one
  item per ID.
- Error semantics: invalid batch shape fails before storage work; per-ID
  lookup/placeholder failures are represented in the corresponding item.
- Freshness: batch mode uses the existing MCP read snapshot witness path; the
  cache invalidation test updates JSONL witness data and verifies fresh output.

## Verification

- `cargo test --features mcp show_issue -- --nocapture`
- `cargo test --features mcp mcp_show_issue_batch_perf_probe -- --ignored --nocapture`
- `cargo fmt --check`
- `git diff --check`
- `cargo check --all-targets`
- `cargo check --all-targets --features mcp`
- `cargo clippy --all-targets -- -D warnings`
- `cargo clippy --all-targets --features mcp -- -D warnings`
