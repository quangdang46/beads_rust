# Empty-target force import integrity gate

Date: 2026-05-04
Bead: `beads_rust-72yf.5`
Agent: `PinkTiger`

## Change

`br sync --import-only --force` now gates the expensive post-import
`VACUUM`/`REINDEX`/`VACUUM INTO` maintenance path for fresh empty imports.
When all of these are true:

- the command is `--force` without `--rebuild`,
- `--rename-prefix` is not active,
- the target DB had no issues and no preserved tombstones before import,
- the import rewrote storage,
- `integrity_check` reports exactly clean after import,

the command skips the heavy maintenance block. If integrity is not clean, the
old maintenance and repair path still runs.

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

## Results

Baseline binary: `/data/tmp/br-candidate-comment-import-20260504`
Candidate binary: `/data/tmp/br-candidate-empty-import-gate-20260504`

| Run | Wall | User | Max RSS | FS outputs |
| --- | ---: | ---: | ---: | ---: |
| Baseline | 5:04.32 | 302.70s | 173,784 KB | 595,072 |
| Candidate | 4:54.75 | 294.03s | 148,620 KB | 165,840 |

Observed effect:

- Wall time: about 1.03x faster than the paired baseline, essentially flat
  against the previous best import proof (`4:53.11`).
- User CPU: about 2.9% lower than the paired baseline.
- Max RSS: about 14.5% lower than the paired baseline.
- Filesystem outputs: about 72.1% lower than the paired baseline.

This is kept as a write-amplification/resource-pressure win for large swarm
hosts, not as a material CPU improvement.

## Safety checks

The candidate workspace was
`/data/tmp/br-import-empty-gate-candidate-a-20260504-SrJgXW`.

Post-import output:

```json
{"created":12000,"updated":0,"skipped":0,"tombstone_skipped":0,"orphans_removed":0,"blocked_cache_rebuilt":true}
```

`br sync --status --json` reported dirty count `0`,
`jsonl_newer=false`, `db_newer=false`, and the expected JSONL content hash.

`br doctor --json` reported:

- `ok: true`
- `workspace_health: healthy`
- `sqlite.integrity_check: ok`
- `sqlite3.integrity_check: ok`
- DB vs JSONL counts both `12,000`

Focused unit verification:

```bash
env CARGO_TARGET_DIR=/data/tmp/beads-rust-target-empty-import-gate-20260504 \
  cargo test fresh_force_import_maintenance_gate --lib -- --nocapture
```

