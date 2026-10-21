# orphaned_write_lock

- **FM**: `fm-concurrency_primitives-orphaned-write-lock` (P1)
- **Subsystem**: concurrency_primitives
- **Detect**: the `write_lock` check probes the advisory lock on
  `.beads/.write.lock` instead of guessing from its mtime.
    - The lock file is present but **no process holds it** → `ok`, with
      `details.reason == "not_held"`.
    - The lock is **held by a live process** → `warn`, with
      `details.reason == "lock_held"` and a "wait for the running `br`"
      remediation.
    - The lock state could not be determined → `ok`, with
      `details.reason == "lock_state_unknown"`.
- **Why the probe, not the clock**: `.beads/.write.lock` is an advisory
  `flock`. The kernel releases it when the holder exits, and the file is
  deliberately never unlinked — unlinking it would race with a waiter that
  already holds an open handle to the old inode. Every `br` command
  therefore leaves the file behind, including the read-only ones and the
  doctor itself. The previous detector warned whenever the file's mtime
  was older than `BR_DOCTOR_STALE_LOCK_THRESHOLD_SECS` (default 300s), so
  `br doctor` exited non-zero on any healthy repository that had not been
  written to for five minutes. Asking the lock answers the question the
  check exists to answer: is a writer holding this workspace right now?
- **Repair contract**: SAFETY — detect-only. The doctor NEVER removes
  `.write.lock` automatically: touching a lock file that a live process
  holds would corrupt that process's locking discipline.
- **Round-trip**: N/A — no chokepointed mutation.
- **Expected exit codes**:
    - detect: 0 (a leftover, unheld lock file is healthy)
    - repair: 0 (no destructive action)
    - undo: 0

The "lock is held" branch needs a live holder, which bash cannot create
portably (`flock(1)` is Linux-only), so it is covered by the Rust unit
test `check_orphaned_write_lock_held_is_warn`. This fixture guards the
other half — that a leftover lock file is *not* reported as a problem.
