---
name: beads
description: Use when working in a repository that uses br or Beads for durable project task tracking, issue dependencies, blocker management, multi-session handoff, or shared work memory. Trigger when the user asks to find ready work, claim or close tasks, create follow-up work, inspect blockers, recover project context, or choose between local planning and persistent project tracking.
---

# Beads

Use Beads as the shared project task system. Local plans, scratch files, and personal memories are useful, but they are not the durable source of truth for project work.

## First Step

Run:

```bash
br prime
```

If that prints nothing, check whether the repository has an active Beads workspace:

```bash
br where
```

## Preferred Route

Use the `br` CLI when shell access is available. It is the most compact and direct Beads interface.

## Core CLI Workflow

1. Find work:

```bash
br ready
br list --status=open
br list --status=in_progress
```

2. Inspect before editing:

```bash
br show <id>
```

3. Claim work atomically:

```bash
br update <id> --claim
```

4. Create durable follow-up work when implementation reveals new tasks:

```bash
br create "Short title" --description="Why this exists and what needs to be done" --type=task --priority=2
```

5. Close completed work:

```bash
br close <id> --reason="Completed"
```

## What Belongs In Beads

Use Beads for:

- shared project tasks
- blockers and dependencies
- discovered follow-up work
- work that must survive thread reset, compaction, or handoff
- status that another person or agent should be able to resume

Use agent-local planning tools only for the current turn's execution checklist. Do not treat them as shared project state.

## Rules

- Do not create markdown TODO files as the source of truth when Beads is available.
- `br` has no interactive editor command; use `br update` flags instead.
- Prefer `--json` when parsing `br` output programmatically.
- If hooks are installed, `br prime` may already be injected. Run it manually when context is missing.
- Do not auto-close or mutate tasks unless the work is actually complete.
