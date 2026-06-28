# Fresh import relation insert fast path

## Summary

Candidate skips per-issue relation table deletes on `CollisionAction::Insert` only
when the issue row insert actually succeeds. Import fallback/update paths still
use the existing delete-and-sync relation path.

Result on the same 12k fresh JSONL import corpus used by the prior accepted
export-hash slice:

| Binary | Workspace | Wall | User | Max RSS | FS outputs |
| --- | --- | ---: | ---: | ---: | ---: |
| `/data/tmp/br-candidate-export-hash-post-clear-20260504` | `/data/tmp/br-new-relation-baseline-20260504-hmNoGj` | `4:38.70` | `277.55s` | `173440 KB` | `596752` |
| `/data/tmp/br-candidate-new-relation-insert-20260504` | `/data/tmp/br-new-relation-candidate-20260504-k6mLnZ` | `3:02.91` | `181.77s` | `175384 KB` | `596752` |

Wall speedup: `278.70s / 182.91s = 1.52x`.

## Workload

Corpus:
`/data/tmp/br-parallel-export-work-20260504T0628/.beads/issues.jsonl`

Corpus SHA-256:
`30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a`

Command:

```bash
/usr/bin/time -v br sync --import-only --force --json
```

Both runs imported 12,000 new issues:

```json
{"created":12000,"updated":0,"skipped":0,"tombstone_skipped":0,"orphans_removed":0,"blocked_cache_rebuilt":true}
```

Both workspaces preserved the JSONL corpus SHA after import.

## Correctness Checks

Baseline and candidate `sync --status --json` both reported:

```json
{"dirty_count":0,"jsonl_content_hash":"30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a","jsonl_exists":true,"jsonl_newer":false,"db_newer":false}
```

Baseline and candidate `doctor --json` both reported `ok: true` and
`workspace_health: healthy`; the only warning was the expected frankensqlite WAL
sidecar-without-SHM note.

DB label grouping stayed equal across baseline and candidate:

```text
count --by label --json SHA-256:
f893044d40ff4cf9c5aa354897c9c4fdd6ab69cd860e85c7a5d8c3f713a1c2de
```

Focused tests:

```bash
cargo test test_insert_new_issue_relations_for_import_skips_relation_deletes --lib -- --nocapture
cargo test --test e2e_workspace_commands e2e_sync_import_only_accepts_mixed_prefixes_and_keeps_default_prefix_for_new_ids -- --nocapture
cargo test --test e2e_comments e2e_comments_sync_roundtrip -- --nocapture
```

## Proof Obligations

- Fresh insert path skips relation `DELETE` statements only after
  `insert_new_issue_for_import` reports a successful new issue row insert.
- Primary-key or unique-key insert fallback returns `false` and uses the
  existing relation sync path, so possible stale labels, dependencies, and
  comments are deleted before reinsertion.
- Update, merge, replace, tombstone, and remove-orphan actions continue to use
  the existing sync/delete relation path.
- Comment ID collision semantics are preserved by reusing
  `insert_comment_for_import`; the focused unit test covers cross-issue comment
  ID collision fallback.
- Label import remains duplicate-tolerant with `INSERT OR IGNORE`, while the
  existing JSONL normalization and DB label-count equality checks preserve the
  expected label semantics on the measured corpus.
