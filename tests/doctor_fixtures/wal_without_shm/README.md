# wal_without_shm

- **FM**: `fm-state_files-wal-shm-sidecar-orphan` (P1) — WAL-without-SHM
  variant (benign for frankensqlite).
- **Subsystem**: state_files
- **Detect**: `db.sidecars` check goes to `warn` with "expected for
  frankensqlite" message — confirms the detector distinguishes this from the
  true-error SHM-without-WAL variant.
- **Repair contract**: SAFETY — `--repair` MUST NOT delete the WAL (it may
  carry committed-but-not-checkpointed data).
- **Round-trip**: N/A — no destructive auto-fix.
- **Expected exit codes**:
    - detect: 1 (warning present)
    - repair: 0 or 2 (warning persists)
    - undo: 0
