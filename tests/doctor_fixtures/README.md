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

## Idempotence replay gate (pass-3, opt-in)

`run_all.sh` supports an OPT-IN idempotence-replay gate between Stage 3
(`--repair`) and Stage 4 (`post_repair` assertions). When enabled, the harness
runs `--repair` a SECOND time on the already-repaired workspace and asserts
the new run-dir's `actions.jsonl` is empty — proving the fixer is idempotent
per the chokepoint contract:

```bash
REPLAY_IDEMPOTENCE=1 bash tests/doctor_fixtures/run_all.sh
```

The gate is opt-in because a second `--repair` creates a new run-dir which
becomes "latest", causing Stage 5's `undo latest` to reverse the no-op replay
instead of the original repair. Fixtures whose `post_undo` stage asserts
"undo restored the corruption" (e.g., the gitignore fixers) are incompatible
with the replay flow under the standard harness.

**Per-fixture opt-out**: drop a `.skip_replay` marker file in the fixture
directory. The gitignore fixtures ship with one and document why.

**Suite-level opt-out**: `REPLAY_IDEMPOTENCE_SKIP="name1 name2"` to skip
specific fixtures regardless of the marker file.

The pass-3 design intent is to fold a per-fixture replay assertion INTO
each fixture's `assert.sh post_repair` stage in a future pass, sidestepping
the run-dir / undo-latest interaction. Until then the env-gated suite-level
gate is the documented mechanism.

