# orphaned_write_lock

- **FM**: `fm-concurrency_primitives-orphaned-write-lock` (P1)
- **Subsystem**: concurrency_primitives
- **Detect**: `write_lock` check goes to `warn` when `.beads/.write.lock`
  exists as a regular file whose mtime is older than the staleness
  threshold (`BR_DOCTOR_STALE_LOCK_THRESHOLD_SECS`, default 300s).
  Surfaces the path, mtime as RFC3339, age in seconds, the
  threshold value, and the canonical operator-fix command (a
  manual `mv .write.lock .write.lock.stale-<ISO8601>` rename
  gated on operator-confirming no live `br` is in the workspace).
- **Repair contract**: SAFETY — detect-only. The doctor NEVER
  removes `.write.lock` automatically: touching a lock file
  that a live process holds would corrupt that process's
  locking discipline. Operator must verify, then move aside.
- **Round-trip**: N/A — no chokepointed mutation.
- **Expected exit codes**:
    - detect: 1
    - repair: 0 or 2 (warning persists; no destructive action)
    - undo: 0

This fixture uses `BR_DOCTOR_STALE_LOCK_THRESHOLD_SECS=0` to
force the staleness branch on a freshly-planted lock file
without needing `filetime`-style mtime manipulation (which
would pull in a new dev-dependency).
