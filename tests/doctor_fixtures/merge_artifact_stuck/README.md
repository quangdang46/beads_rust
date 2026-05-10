# merge_artifact_stuck

- **FM**: `fm-state_files-merge-artifact-stuck` (P2)
- **Subsystem**: state_files
- **Detect**: `jsonl.merge_artifacts` check goes to `warn`
- **Repair contract**: SAFETY — currently no auto-delete (detect-only).
  Operator must remove `.base.jsonl`/`.left.jsonl`/`.right.jsonl` manually
  after reviewing the three-way merge. Doctor must not silently destroy them.
- **Round-trip**: N/A — no chokepointed mutation.
- **Expected exit codes**:
    - detect: 1
    - repair: 0 or 2 (warning persists, no destructive action)
    - undo: 0
