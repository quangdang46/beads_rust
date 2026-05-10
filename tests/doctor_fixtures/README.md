# `br doctor` Real-World Fixture Suite (Phase 9)

This directory holds runnable fixtures that exercise `br doctor` against real
corrupt workspaces end-to-end:

```
corrupt.sh  →  br doctor --json  →  assert.sh (Stage A: detect)
            →  br doctor --repair --json  →  assert.sh (Stage B: post-repair)
            →  br doctor undo latest --json (best-effort round-trip)
```

Each fixture directory contains:

- `corrupt.sh <target_dir>` — deterministic recipe to plant the failure inside
  a fresh tempdir. Receives `TOOL_BIN` env (path to the `br` binary). Must
  leave the target in the planted-failure state. Captures a baseline snapshot
  under `<target_dir>/.fixture_baseline/` so the harness can verify what was
  planted survived doctor's read-only stages.
- `assert.sh <target_dir> <stage>` — invoked in two stages:
    - `assert.sh DIR detect` — runs `br doctor --json` and asserts the
      planted failure surfaces in the expected check name + status.
    - `assert.sh DIR post_repair` — invoked after `br doctor --repair`; asserts
      the failure is gone (or quarantined / repaired per the contract).
- `README.md` — one-paragraph description: what FM, what severity, expected
  detect status, expected exit codes.

## Round-trip caveat

`br doctor undo latest` only restores files the **chokepoint** (`mutate()`)
touched. Some current `--repair` paths predate WP3/WP4 chokepoint migration
(notably the JSONL→DB rebuild path) and route writes directly through `fsqlite`
or `std::fs`. For those fixtures, `undo latest` will report `restored: 0`
without failing — that is *not* a fixture failure, it is documented chokepoint
coverage. The `gitignore_leaking_beads` fixture *does* round-trip fully and
serves as the chokepoint regression test.

## Driver

`run_all.sh` is the bash driver. It iterates each fixture directory, sets up a
tempdir, runs the recipe, runs the assertions, runs `--repair` + assertions,
then runs `undo latest` and verifies it exits cleanly. Exits 0 if every
fixture passes; non-zero on first failure with a clear diagnostic.

Per AGENTS.md: no `Command::new("git")` from runtime `br` code; the fixture
recipes themselves may call `git init` for setup (e.g. to materialise a real
`.git/info/exclude`).
