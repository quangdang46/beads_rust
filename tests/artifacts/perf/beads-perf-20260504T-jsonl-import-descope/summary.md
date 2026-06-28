# JSONL Import Parallelism De-Scope Proof

Date: 2026-05-04
Bead: `beads_rust-72yf.5.1`
Agent: `PinkTiger`

## Decision

Do not add another `beads_rust`-level parallel JSONL import executor now.

The retained `beads_rust` import path is already a streaming, deterministic
planner wrapped around a single SQLite write transaction. The remaining cost is
per-row `fsqlite` execution for issue rows, labels, comments, export hashes, and
projection rebuilds. The previous bounded parallel import-preparation probe
preserved behavior but regressed the measured workload and increased RSS, which
means parallelizing the JSONL/planner layer is the wrong lever.

The next high-EV work belongs below `beads_rust`, in the `fsqlite` execution /
storage stack, or in an explicitly different product shape such as an offline
bulk-load format. `beads_rust` should keep the conservative serial import path
until the storage layer provides a faster primitive that preserves the same
transaction semantics.

## Current Source Evidence

Retained import execution in `src/sync/mod.rs` is serial:

- `import_from_jsonl` parses/validates the JSONL file, preloads collision
  metadata, computes the JSONL hash/witness, and opens one write transaction.
- Inside that transaction, `stream_import_actions_in_tx` calls
  `for_each_jsonl_import_issue` and processes each issue action in order.
- Each issue calls `process_import_action`, which executes `insert`, `update`,
  or `skip` against `SqliteStorage`.
- Export hashes are batched only in small serial chunks via
  `insert_export_hashes_after_clear_in_tx`.

Relevant source ranges inspected:

- `src/sync/mod.rs:4358-4458`
- `src/sync/mod.rs:4233-4315`
- `src/sync/mod.rs:4465-4547`
- `src/storage/sqlite.rs:10836-11056`

Parallelism retained in the sync code is limited to witness/export paths:

- `--witness-parallelism` and `build_jsonl_merkle_witness_parallel`
- `--export-parallelism` and `prepare_export_issues_jsonl_parallel`

No retained import-parallelism CLI flag or import worker path exists in the
current tree. The source scan found `thread::scope` in `src/sync/mod.rs` only in
the export line-preparation path.

## Current Release Probe

Binary:

```text
/data/tmp/cargo_target_beads_rust_pinktiger_72yf51_release/release/br
sha256=161a02f590698dc8f4fd9915cb53ed4b2227ba4606be008ccace9ec6cf9e4afc
```

Corpus:

```text
/data/tmp/br-parallel-export-work-20260504T0628/.beads/issues.jsonl
records=12000
sha256=30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a
```

Timed command:

```bash
/usr/bin/time -v \
  /data/tmp/cargo_target_beads_rust_pinktiger_72yf51_release/release/br \
  --db /data/tmp/br-jsonl-import-descope-current-20260504-EBgRQt/.beads/beads.db \
  sync --import-only --force --json
```

Result:

```json
{"created":12000,"updated":0,"skipped":0,"tombstone_skipped":0,"orphans_removed":0,"blocked_cache_rebuilt":true}
```

Timing/resource profile:

```text
Elapsed wall: 3:19.96
User CPU: 198.43s
System CPU: 0.94s
CPU utilization: 99%
Max RSS: 168676 KB
Filesystem outputs: 597256
```

Post-import status:

```json
{
  "dirty_count": 0,
  "jsonl_content_hash": "30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a",
  "jsonl_exists": true,
  "jsonl_newer": false,
  "db_newer": false
}
```

Doctor summary:

```json
{
  "ok": true,
  "workspace_health": "healthy",
  "checks": [
    {"name": "sqlite.integrity_check", "status": "ok"},
    {"name": "sqlite3.integrity_check", "status": "ok"}
  ]
}
```

Interpretation:

- CPU utilization near one core confirms the retained import executor is serial.
- System time is small relative to user time; the path is not dominated by raw
  filesystem reads.
- The import is behaviorally correct on the 12k corpus, so the bottleneck is
  performance, not recovery or integrity failure.

## Prior Probe Evidence

From `tests/artifacts/perf/beads-perf-20260504T-jsonl-pipeline-closeout-audit/summary.md`:

- Bounded parallel JSONL import preparation preserved focused
  serial-vs-parallel normalization and order semantics.
- It regressed the 12,000 record fresh force-import workload from
  `4:36.17 wall / 275.57s user / 145540 KB RSS` to
  `4:46.67 wall / 286.12s user / 171208 KB RSS`.
- The source diff was reverted.

Retained serial-path wins on the same corpus:

- Comment ID collision probe optimization:
  `tests/artifacts/perf/beads-perf-20260504T-parallel-jsonl-import/summary.md`
- Fresh empty forced-import maintenance gate:
  `tests/artifacts/perf/beads-perf-20260504T-empty-force-import-integrity-gate/summary.md`
- Import export-hash post-clear insert path:
  `tests/artifacts/perf/beads-perf-20260504T-import-export-hash-post-clear/summary.md`
- Fresh relation insert path:
  `tests/artifacts/perf/beads-perf-20260504T-new-issue-relation-insert/summary.md`

The strongest retained win came from skipping unnecessary relation deletes on
true fresh inserts: `4:38.70 wall` to `3:02.91 wall` on the measured corpus.
That was a database statement-count reduction, not a parallel JSONL planner.

## Alien-Graveyard / Alien-Artifact Mapping

Relevant graveyard primitives:

- Vectorized execution + morsel-driven parallelism: valuable when applied at the
  VDBE/operator layer, where independent row batches can execute without
  repeatedly crossing the SQL execution boundary.
- B-epsilon trees / write-optimized indexes: relevant to label/comment/export
  hash write amplification, but they are storage-engine primitives.
- Parallel WAL / MVCC: relevant to multi-writer scalability, but outside the
  current single-import transaction semantics.
- Flat combining: useful for many contending writers, but not for one long
  single-process import transaction.

Alien-artifact proof obligations for any future lower-layer fix:

- Same JSONL import semantics: IDs, external refs, comments, labels,
  dependencies, tombstones, dirty state, and export hashes must match the current
  serial import.
- Deterministic order: any morselized executor must commit in a stable order or
  prove order independence for every affected table.
- Transaction boundary: failure must roll back the full import exactly as today.
- Fallback: existing serial `beads_rust` import remains available.
- Evidence: paired timing, RSS, filesystem outputs, `sync --status`, `doctor`,
  label-count/hash checks, and golden JSONL hash.

## EV Matrix

| Candidate | Impact | Confidence | Effort | Score | Decision |
| --- | ---: | ---: | ---: | ---: | --- |
| Add another `beads_rust` parallel import-prep layer | 1 | 2 | 3 | 0.67 | Reject; already regressed and raised RSS. |
| Batch more relation writes in `beads_rust` using multi-values SQL | 2 | 1 | 3 | 0.67 | Reject for now; prior label batching regressed and exposed `fsqlite` multi-values correctness risk. |
| Lower-layer `fsqlite` prepared/bulk DML fix for repeated row inserts | 4 | 3 | 3 | 4.0 | Best next target, but belongs in the path dependency. |
| VDBE/storage morselized bulk-load primitive | 5 | 3 | 5 | 3.0 | Good research target after lower-level profiling, not a quick `beads_rust` patch. |

## Closure Criteria Check

`beads_rust-72yf.5.1` acceptance offered two ways to close:

1. Land a measured parallel import execution path with serial fallback.
2. Produce a proof-backed de-scope showing the remaining lever belongs in
   `fsqlite` rather than `beads_rust`.

This artifact satisfies path 2. It does not claim the original `.5` wording was
literally implemented for import. It records why the remaining parallel import
work should move to the storage execution layer instead of adding another
planner-level parallel stage in `beads_rust`.
