# Read-Only Fast-Open Lock Bypass

Date: 2026-05-04

Bead: `beads_rust-72yf.3`

## Objective

Let CLI read commands that already qualify for `read_only_fast_open` attempt the
current-schema read-only SQLite open before joining the `.beads/.write.lock`
writer serialization path. Fallback behavior remains conservative: if the
read-only fast open misses, recovery/import still reacquires `.write.lock`
before any writable open or rebuild.

## Opportunity Matrix

| Lever | Impact | Confidence | Effort | Score |
|---|---:|---:|---:|---:|
| Defer startup `.write.lock` for `read_only_fast_open` commands | 4 | 4 | 1 | 16.0 |

Alien-graveyard mapping: this is the follower-read / RCU-shaped version of the
snapshot-service idea. Proven-current reads use an immutable read-only DB view;
uncertain reads fall back to the existing serialized writer path.

## Baseline

Command:

```bash
/data/tmp/rch_target_beads_rust_list_toon_page_baseline/release/br \
  --no-auto-import --no-auto-flush --lock-timeout 200 list --limit 1 --json
```

With another process holding `.beads/.write.lock` for 2s:

- Exit code: `7`
- Wall time: `210ms`
- Error: `Timed out after 200ms waiting for write lock`

Uncontended golden SHA-256:

```text
cd338282fb8f1deb336ed14df146155cf34082b973379ca371a2b95986917bb2
```

## Candidate

Command:

```bash
/data/tmp/beads_rust_dustypuma_local/release/br \
  --no-auto-import --no-auto-flush --lock-timeout 200 list --limit 1 --json
```

With another process holding `.beads/.write.lock` for 2s:

- Exit code: `0`
- Wall time: `21ms`
- Output SHA-256: `cd338282fb8f1deb336ed14df146155cf34082b973379ca371a2b95986917bb2`
- Golden equality: byte-identical to baseline uncontended output

Default auto-import/autoflush command under the same held lock also returned in
`21ms` with the same SHA-256, proving the read-only freshness probe path did not
need the writer lock when the DB was current.

## Isomorphism Proof

- Ordering preserved: yes. The command dispatch and list query are unchanged.
- Tie-breaking unchanged: yes. The list command receives the same args and
  storage query path.
- Floating-point: N/A.
- RNG seeds: N/A.
- Golden output: candidate uncontended and candidate lock-contended JSON are
  byte-identical to baseline uncontended JSON.

## Fallback Proof

`cargo test read_only_fast_open_miss_waits_for_write_lock_before_rebuild -- --nocapture`
passed after the change. A fast-open miss still waits for `.write.lock` before
rebuilding from JSONL.
