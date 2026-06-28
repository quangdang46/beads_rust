# MCP Read Snapshot Prototype

Bead: `beads_rust-72yf.3`

This artifact covers the first opt-in MCP read snapshot prototype. The committed
probe exercises three supported read surfaces:

- `project_overview`
- `list_issues`
- `show_issue`

The implementation does not cache SQLite connections. It caches JSON read
projections only when `BR_MCP_READ_SNAPSHOT` is truthy and validates each hit
against a DB, WAL, SHM, and JSONL metadata witness. Mutations clear the in-memory
cache before writing. Any witness capture failure, mismatch, unsupported command,
or disabled env var falls back to direct storage.

## Command

```bash
env CARGO_TARGET_DIR=/data/tmp/beads_rust_scheduler_final cargo test --features mcp mcp::tools::tests::mcp_read_snapshot_perf_probe -- --ignored --nocapture
```

## Result

The probe created 250 issues and executed 250 repeated reads per surface.

| Surface | Direct total ns | Cached total ns | Speedup |
| --- | ---: | ---: | ---: |
| `project_overview` | 13589217999 | 11954750 | 1136.72x |
| `list_issues` | 11944344416 | 284558668 | 41.97x |
| `show_issue` | 11518930782 | 7386033 | 1559.56x |

The test asserted cached JSON equality against the direct builder output before
timing and after the cached loop for each surface.

## Freshness Tests

Focused MCP tests also cover:

- stable witness cache hits
- JSONL witness mismatch rejection
- mutation-time cache clearing
- `project_overview` stale-count invalidation
- `list_issues` stale-count invalidation
- `show_issue` stale-title invalidation
