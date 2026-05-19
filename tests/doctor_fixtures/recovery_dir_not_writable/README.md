# recovery_dir_not_writable

- **FM**: `fm-permissions-recovery-dir-not-writable` (P2) — the
  `.beads/.br_recovery/` directory is not writable by the current
  user. The recovery dir is where `--repair` moves quarantined DB
  families to before destructive rebuilds; a read-only parent
  makes the backup move fail and aborts the recoverable-anomaly
  fixer.
- **Subsystem**: permissions
- **Detect**: `permissions.recovery_dir` check goes to `warn` when
  the dir exists but the owner-write bit is unset.
- **Repair contract**: **DETECT-ONLY** — operators may have
  intentionally locked the recovery dir (immutable backup folder).
  Auto-chmoding would stomp that decision. The check surfaces the
  problem; the operator handles the chmod.
- **Round-trip**: create `.beads/.br_recovery/`, plant a seed
  marker, chmod 0o555 → detect warn → `--repair` runs (other
  fixers may fire) → assert mode unchanged → `doctor undo`.
- **Expected exit codes**:
    - detect: 1 (warn present)
    - repair: variable (no fixer runs for this FM)
    - undo: 0 (state preserved)
- **Platform**: Unix-only.
