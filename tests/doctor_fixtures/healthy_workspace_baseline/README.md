# healthy_workspace_baseline (control)

- **FM**: none (control fixture)
- **Subsystem**: meta
- **Detect**: No `error`-level checks on a fresh `br init` workspace.
  (`db.sidecars` may be `warn` for the expected frankensqlite WAL-without-SHM
  state.)
- **Repair contract**: `--repair` is a no-op idempotent invocation; does NOT
  introduce new errors.
- **Round-trip**: trivial — undo has nothing to restore on a healthy
  workspace.
- **Expected exit codes**:
    - detect: 0 or 1 (warnings only)
    - repair: 0 or 2
    - undo: 0

This is the "doctor doesn't break a clean workspace" regression — the
weakest fixture in the suite but the most important guarantee.
