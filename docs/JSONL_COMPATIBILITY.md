# JSONL Interchange Compatibility

> **Last updated:** 2026-07-01  
> **Scope:** JSONL export/import between Rust `br` (beads_rust) and Go `bd` (gastownhall/beads)

## Overview

Both `br` and `bd` use JSONL as their portable interchange format for git-based
sync. This document catalogs known wire-format differences and semantic
mismatches so that operators of mixed toolchains can reason about compatibility.

## Field Map

| JSON key         | `br` (Rust)                       | `bd` (Go)                         | Compatibility |
|------------------|-----------------------------------|-----------------------------------|---------------|
| `id`             | `String`                          | `string`                          | ✅ Same       |
| `title`          | `String`                          | `string`                          | ✅ Same       |
| `description`    | `Option<String>`                  | `string,omitempty`                | ✅ Compatible  |
| `status`         | `enum Status`                     | `string` (+ `status_set`)         | ⚠️  See below |
| `priority`       | `Priority` (P0-P4 integer)        | `int`                             | ✅ Same       |
| `issue_type`     | `enum IssueType`                  | `string`                          | ⚠️  See below |
| `assignee`       | `Option<String>`                  | `string,omitempty`                | ✅ Compatible  |
| `created_at`     | `DateTime<Utc>` (RFC 3339)        | `time.Time` (RFC 3339)            | ✅ Same       |
| `updated_at`     | `DateTime<Utc>` (RFC 3339)        | `time.Time` (RFC 3339)            | ✅ Same       |
| `closed_at`      | `Option<DateTime<Utc>>`           | `*time.Time,omitempty`            | ✅ Compatible  |
| `due_at`         | `Option<DateTime<Utc>>`           | `*time.Time,omitempty`            | ✅ Compatible  |
| `timeout_seconds`| `Option<i64>` (seconds)           | `timeout` (`time.Duration`, ns)   | ⚠️  See below |
| `waiters`        | **Not persisted**                 | `[]string,omitempty`              | ❌ Lost on import→export |
| `_type`          | **Not emitted**                   | `"issue"` / `"memory"` wrapper   | ⚠️  br imports bare `Issue` JSON |
| `actor`          | Event-level only (`Event.actor`)  | Issue-level `string,omitempty`    | ✅ Compatible  |
| `labels`         | `Vec<String>`                     | `[]string`                         | ✅ Same       |
| `dependencies`   | `Vec<Dependency>` (struct)        | `[]string` (IDs only, older bd)   | ⚠️  See below |

## Key Differences

### `timeout` vs `timeout_seconds`

- **`bd` export:** emits `"timeout": <nanoseconds>` (e.g. `3600000000000` for 1h).
- **`br` import:** accepts both `"timeout"` (via serde `alias`) and
  `"timeout_seconds"`. Nanosecond values are **NOT converted** — they are stored
  as-is in the `timeout_seconds` column.
- **`br` export:** always emits `"timeout_seconds": <seconds>`.
- **Consequence:** If a bd-exported JSONL with `"timeout": 3600000000000` is
  imported into br then re-exported, the field becomes
  `"timeout_seconds": 3600000000000` (still nanoseconds, wrong key). Operators
  must manually convert between export→import cycles.

> **Recommendation for dual-write:** Future br versions could detect ns-level
> values and convert on import. For now this is a known manual step.

### Tombstones

- **`bd` (pre-v0.50):** Exports deleted issues with `"status": "tombstone"` and
  `"deleted_at"` set. Import skips these lines (they are not valid statuses in
  modern bd either).
- **`br`:** Already skips `"status": "tombstone"` lines on import (matched via
  `StatusProbe`). br never exports tombstones; deleted issues are simply omitted
  from the JSONL output.
- **br tombstone tracking:** br tracks deletion via `deleted_at`, `deleted_by`,
  and `delete_reason` fields on the `Issue` struct. These are preserved across
  import/export but are not the same as bd's tombstone status.
- **Tombstone retention:** br supports `--retention-days` to expire tombstone
  records from export after a configurable period. Does not affect import.

**Policy:** br considers tombstones a bd legacy concept. br's own deletion
metadata is always exported when present.

### `_type` Discriminator

- **`bd` export:** Wraps each line in a JSON object with `"_type": "issue"` (or
  `"_type": "memory"`). The actual issue fields are nested inside.
- **`br` import:** Expects bare `Issue` JSON at the top level. If a line has
  `_type`, br tries to deserialize it as an Issue and will likely reject it
  (unknown field `_type`).
- **`br` export:** Emits bare `Issue` JSON, no wrapper.
- **Workaround:** Strip the wrapper before importing into br:
  ```bash
  jq 'select(._type == "issue" or ._type == null) | if ._type == "issue" then .fields else . end' input.jsonl > br_compat.jsonl
  ```

### Dependency Format

- **`br`:** Dependencies are structured objects with `issue_id`, `depends_on_id`,
  `dep_type`, `created_at`, `created_by`, `metadata`, and `thread_id`.
- **`bd` (older):** Dependencies are plain string IDs in the `dependencies`
  array (just the issue IDs being depended on).
- **`br` import:** Accepts both formats. Plain strings are converted to
  `Dependency { issue_id: <current>, depends_on_id: <id>, ... }`.
- **`bd` (v0.2.15+):** Uses the same struct format as br.

### Issue Types

Rust `IssueType` supports: `task`, `bug`, `feature`, `epic`, `question`,
`docs`, `goal`, `wisp`, `composite`. Go `bd` has a superset that includes
`molecule`, `patrol`, `gate`, etc. Unknown types are mapped to `task` on
import by br's `FromStr` / `Default` fallback.

### Status

Rust `Status` uses a subset of bd's status set. bd-only statuses (e.g.
`gated`, `monitoring`, `queued`) are mapped to their closest br equivalent
on import. The full mapping is in `src/model/mod.rs` `Status::from_str`.

## Summary

| Concern                        | Status                                |
|--------------------------------|---------------------------------------|
| `timeout` field alignment      | ✅ Import accepts both (alias)        |
| `waiters` field                | ❌ Not persisted (no gate feature)    |
| `_type` wrapper                | ⚠️  br ignores; manual strip needed  |
| `actor` at Issue level         | ⚠️  br only persists at Event level  |
| Tombstone handling             | ✅ Compatible (skipped on import)     |
| Dependency struct vs string[]  | ✅ Both accepted on import            |
| Export wrapper                 | ✅ br uses bare Issue JSON            |

## Testing

```bash
# Verify import of bd-style timeout field
echo '{"id":"test-001","title":"test","status":"open","timeout":3600000000000,"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","priority":2,"issue_type":"task"}' | br import --stdin --quiet

# Verify export still uses timeout_seconds
br export --json | head -1 | jq '.timeout_seconds'
```
