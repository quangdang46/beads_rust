# Agent-Friendly Changelog

This file tracks agent-facing changes (docs, robot output surfaces, schemas, safety behavior).

## 2026-07-16

- Added `br reflect` — read-only agent instruction emitter to reconcile open beads with git/code since the last `.beads/issues.jsonl` change (prime-style; no issue mutations).
- Surfaces: human markdown, `--json` (`br.reflect.v1`), `--mcp`, `--since <rev>`, `--export`, optional `.beads/REFLECT.md` protocol override.
- Wired into `br capabilities`, `br robot-docs guide`, README, CLI_REFERENCE, and AGENT_INTEGRATION.

## 2026-01-25

- Added agent-first doc entrypoints under `docs/agent/`.
- Added `agent_baseline/` snapshots (README/help/schema + small example outputs).
- Added `agent_baseline/examples/robot_mode_examples.jsonl` and `agent_baseline/schemas/cli_schema.json` as static, machine-readable artifacts.
- Removed `rm -rf` usage from local scripts/tests to comply with the no-deletion policy in `AGENTS.md`.
