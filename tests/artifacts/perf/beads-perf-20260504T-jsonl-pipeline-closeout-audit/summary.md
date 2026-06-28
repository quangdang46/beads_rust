# JSONL Pipeline Closeout Audit

Date: 2026-05-04
Bead: `beads_rust-72yf.5`
Agent: `PinkTiger`

## Verdict

`beads_rust-72yf.5` is not ready to close as written.

Implemented and verified:

- Deterministic Merkle-style JSONL witnesses with bounded parallel chunk hashing.
- Candidate-ordered witness reuse, parallel work-batch planning, and
  seek-based materialization proof.
- Bounded parallel JSONL export line preparation with byte-identical serial
  fallback.
- Several measured serial-path import optimizations for fresh forced imports.

Remaining acceptance gap:

- No actual parallel JSONL import execution path is retained in the current
  tree. A bounded parallel import-preparation probe preserved semantics but
  regressed the measured workload and increased RSS, so it was reverted. The
  current import path is faster than before, but its wins come from reducing
  serial SQLite/fsqlite work rather than exploiting multiple cores during import.

## Evidence

### Witness / Merkle Layer

Source:

- `src/sync/witness.rs`: `build_jsonl_merkle_witness_parallel` reads JSONL in
  order, hashes complete chunks with a fixed worker set, and reassembles by
  chunk index before computing the root hash.
- `src/sync/witness.rs`: `plan_jsonl_witness_reuse`,
  `plan_jsonl_witness_parallel_work`, and
  `materialize_jsonl_witness_reuse_plan` prove unchanged-chunk reuse,
  deterministic batch order, and exact candidate-byte reconstruction.
- `src/cli/commands/sync.rs`: `br sync --witness` exposes
  `base_comparison`, `base_reuse_plan`, `base_parallel_work_plan`, and
  `base_reuse_materialization`.

Tests:

- `parallel_witness_matches_serial_witness`
- `parallel_witness_parallelism_one_uses_serial_output`
- `reuse_materialization_reconstructs_candidate_bytes_from_mixed_actions`
- `parallel_work_plan_batches_candidate_actions_before_metadata_drops`
- `e2e_sync_witness_json_is_deterministic_and_read_only`
- `e2e_sync_witness_reports_base_snapshot_drift`

### Parallel Export

Source:

- `src/sync/mod.rs`: `prepare_export_issues_jsonl_parallel` prepares JSONL
  lines and per-issue content hashes in worker chunks, sorts chunks by original
  start index, then the writer emits and hashes entries serially in issue order.
- `src/cli/mod.rs` and `src/cli/commands/sync.rs`: `--export-parallelism`
  controls the worker cap; `--export-parallelism 1` is the serial fallback.
- `BR_DISABLE_PARALLEL_JSONL_EXPORT=1` disables the parallel export stage.

Artifact:

- `tests/artifacts/perf/beads-perf-20260504T-parallel-jsonl-export/summary.md`

Result:

- 12,000 issue/comment-heavy synthetic export workload.
- Serial fallback: 725.94 ms mean.
- Parallel export with 64 workers: 686.04 ms mean.
- Speedup: 1.058x vs serial fallback.
- Serial and parallel JSON stdout hashes matched.
- Serial and parallel exported JSONL SHA-256 matched:
  `30eb851908afb1a054da8248c6e406d79d2cc8caabc135d4c61e1326a4a7cf8a`.

Regression test:

- `e2e_sync_flush_export_parallelism_preserves_jsonl_bytes`

Audit verification run:

```bash
env CARGO_TARGET_DIR=/data/tmp/cargo_target_beads_rust_pinktiger_72yf5_audit \
  cargo test --test e2e_basic_lifecycle e2e_sync_witness -- --nocapture

env CARGO_TARGET_DIR=/data/tmp/cargo_target_beads_rust_pinktiger_72yf5_audit \
  cargo test --test e2e_basic_lifecycle \
    e2e_sync_flush_export_parallelism_preserves_jsonl_bytes -- --nocapture

env CARGO_TARGET_DIR=/data/tmp/cargo_target_beads_rust_pinktiger_72yf5_audit \
  cargo test --lib parallel_witness -- --nocapture
```

All three focused audit checks passed on 2026-05-04.

### Import Work

Implemented serial-path import wins:

- Comment ID collision probe optimization:
  `tests/artifacts/perf/beads-perf-20260504T-parallel-jsonl-import/summary.md`
- Fresh empty forced-import maintenance gate:
  `tests/artifacts/perf/beads-perf-20260504T-empty-force-import-integrity-gate/summary.md`
- Import export-hash post-clear insert path:
  `tests/artifacts/perf/beads-perf-20260504T-import-export-hash-post-clear/summary.md`
- Fresh import relation insert fast path:
  `tests/artifacts/perf/beads-perf-20260504T-new-issue-relation-insert/summary.md`

Best retained import result:

- Baseline after prior import work: 4:38.70 wall / 277.55s user.
- Candidate fresh relation insert path: 3:02.91 wall / 181.77s user.
- Speedup: 1.52x on the 12,000 record fresh forced-import corpus.
- JSONL SHA, doctor, sync status, and label-count checks matched.

Rejected actual parallel import probe:

- Bounded parallel JSONL import preparation preserved focused
  serial-vs-parallel normalization and order semantics.
- It regressed the paired 12,000 record fresh forced-import corpus from
  4:36.17 wall / 275.57s user / 145,540 KB RSS to
  4:46.67 wall / 286.12s user / 171,208 KB RSS.
- The source diff was reverted. No parallel import execution path is retained.

## Acceptance Matrix

| Acceptance criterion | Current state |
| --- | --- |
| Parallel import/export preserves semantics and stable ordering | Partially met. Export and witness work are parallel and byte-stable; import is not parallel in retained code. |
| Merkle witnesses detect unchanged chunks and drift without hiding conflicts | Met by witness comparison, reuse plan, work plan, materialization proof, and e2e drift coverage. |
| Memory stays within declared budget on adversarial relation-heavy corpora | Partially met. Export avoids whole-workspace hydration above the threshold and retained import wins track RSS, but there is no formal import memory-budget gate for the full acceptance wording. |
| Serial fallback remains available and covered by parity tests | Met for witness and export (`max_parallelism == 1`, `--export-parallelism 1`); not applicable to import because no parallel import path is retained. |

## Recommended Next Step

Keep `beads_rust-72yf.5` open or split its remaining import acceptance into a
follow-up that explicitly targets the per-row SQLite/fsqlite execution cost.
The latest failed probes show that JSONL parsing and planning are no longer the
useful import lever; the remaining cost is in serial row execution during
relation/hash recording.
