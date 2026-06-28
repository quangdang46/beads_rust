# Import export-hash post-clear insert path

Date: 2026-05-04
Bead: `beads_rust-72yf.5`
Agent: `PinkTiger`

## Change

`br sync --import-only --force` clears `export_hashes` at the start of the
import transaction. The import path now records post-import export hashes with a
dedicated `insert_export_hashes_after_clear_in_tx` path instead of the general
`set_export_hashes_in_tx` helper, which deletes each target row before
reinserting it.

The general helper is unchanged for normal update paths. The import-only helper
deduplicates duplicate issue IDs with the same "last value wins" rule, then
inserts rows one at a time with `INSERT OR REPLACE`. A more aggressive multi-row
`INSERT` variant was rejected during unit testing because this `fsqlite` path
reported success while failing to materialize a later row.

## Workload

Corpus:
`/data/tmp/br-parallel-export-work-20260504T0628/.beads/issues.jsonl`

- Records: 12,000
- Shape: one issue plus one imported comment per record
- Size: 10,116,894 bytes
- SHA-256: `30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a`

Each run used a fresh `br init` workspace, copied the corpus into
`.beads/issues.jsonl`, then ran:

```bash
/usr/bin/time -v br sync --import-only --force --json
```

## Paired Release Result

Baseline binary: `/data/tmp/br-candidate-empty-import-gate-20260504`
Candidate binary: `/data/tmp/br-candidate-export-hash-post-clear-20260504`

| Run | Workspace | Wall | User | Max RSS | FS outputs |
| --- | --- | ---: | ---: | ---: | ---: |
| Baseline | `/data/tmp/br-import-export-hash-baseline-20260504-Wb9pUs` | 4:42.03 | 281.41s | 142,404 KB | 165,744 |
| Candidate | `/data/tmp/br-import-export-hash-candidate-20260504-NDpuP1` | 4:26.05 | 265.48s | 146,556 KB | 165,840 |

Observed effect:

- Wall time: about 1.06x faster.
- User CPU: about 1.06x lower.
- Max RSS: about 2.9% higher on this run.
- Filesystem outputs: effectively flat, as expected after the earlier force
  import maintenance gate.

Both runs reported:

```json
{"created":12000,"updated":0,"skipped":0,"tombstone_skipped":0,"orphans_removed":0,"blocked_cache_rebuilt":true}
```

Both workspaces preserved the input JSONL SHA-256:

```text
30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a
```

Candidate post-import checks:

- `br doctor --json`: `ok: true`, `workspace_health: healthy`,
  `sqlite.integrity_check: ok`, `sqlite3.integrity_check: ok`, DB and JSONL
  counts both 12,000.
- `br sync --status --json`: `dirty_count: 0`, `jsonl_newer: false`,
  `db_newer: false`, expected `jsonl_content_hash`.

## Verification

Focused unit proof:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads-rust-target-export-hash-post-clear-20260504 \
  cargo test test_insert_export_hashes_after_clear_skips_stale_rows --lib -- --nocapture
```

The unit test verifies stale rows are removed by the preceding clear, duplicate
issue IDs keep the last hash, and the final table has one row per unique issue.
