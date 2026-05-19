# root_gitignore_not_writable

- **FM**: `fm-permissions-gitignore-not-writable-blocks-repair` (P2) —
  the repo-root `.gitignore` exists but the current process lacks
  owner-write permission. The existing `doctor.gitignore_repair`
  fixer rewrites this file when the `.beads/` shadow rule is
  missing; this detector lets that fixer refuse the write before
  it bypasses an intentionally locked file.
- **Subsystem**: permissions
- **Detect**: `permissions.root_gitignore` check goes to `warn`
  when the file exists, is a regular file (not symlink), and has
  the 0o200 owner-write bit cleared.
- **Repair contract**: DETECT-ONLY — operators may have intentionally
  locked the repo-root `.gitignore` (compliance-controlled file,
  vendored shared config). `--repair` must NOT silently chmod it.
- **Remediation**: `chmod u+w .gitignore` or hand-edit, then
  re-run `--repair`.

This is the third detect-only fixture in the permissions diagnostic
family, completing the pattern alongside `doctor_runs_not_creatable`
(cycle 49) and `recovery_dir_not_writable` (cycle 51). All three
pin the SACRED INVARIANT: doctor must NOT silently chmod
operator-locked filesystem objects, even when doing so would
unblock an otherwise-correct repair.

The `doctor.gitignore_repair` fixer correctly consults the lock at
`doctor.rs:7700-7709` and refuses to write when warn fires.
However, there is one documented carveout: `ensure_doctor_in_gitignore`
in `run_dir.rs:295` runs BEFORE the chokepoint to add `.doctor/` to
the repo-root `.gitignore` (chicken-and-egg: the chokepoint's
run-dir lives at `<repo>/.doctor/runs/<id>/`, so `.doctor/` must
be ignored before the chokepoint can record its first action).
This carveout does NOT consult `permissions.root_gitignore` and
will overwrite a chmod-locked file via tmp+rename. The fixture
works around this by pre-seeding `.doctor/` in the planted
`.gitignore`, which makes the carveout a no-op (line 307
`already` branch). If `ensure_doctor_in_gitignore` is ever
upgraded to consult the lock, this corrupt-time seed becomes
optional.

Unix-only — the check uses POSIX mode bits and is a no-op on
Windows where the underlying ownership model differs.
