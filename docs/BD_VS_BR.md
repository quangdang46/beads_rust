# BD (Go) vs BR (Rust) — Scope Matrix

This document tracks command parity, intentional scope differences, and Rust-exclusive features between the Go (`bd`) and Rust (`br`) implementations of Beads.

## Intentionally Out of Scope for `br`

The following `bd` features are **not** planned for `br`:

| Feature | `bd` Command | Rationale |
|---------|-------------|-----------|
| Dolt version control | `bd dolt`, `bd branch`, `bd vc`, `bd merge`, `bd commit`, `--as-of` | `br` uses SQLite + JSONL; Dolt adds complexity with no clear agent benefit |
| External tracker sync | `bd track`, `bd sync-linear`, `bd sync-jira`, `bd sync-ado`, `bd sync-github`, `bd sync-notion` | Agents use MCP / APIs directly; `bd` shell-outs are brittle |
| Semantic compaction | `bd admin compact`, `bd restore` | Haiku-format compaction is experimental; `br` stores full content with SHA-256 dedup |
| Persistent memory | `bd remember`, `bd prime` | Use `br robot-docs` + `br agents` for session context; LLM memory handled externally |
| Mol/Swarm user domain | `bd mol`, `bd swarm`, full formula cook | Swarm orchestration has its own external tooling |
| RPC daemon | `bd daemon rpc`, `bd daemon start` | `br serve` uses MCP over stdio (no network daemon) |

## Command Parity Map

| Task | `bd` (Go) | `br` (Rust) | Notes |
|------|-----------|-------------|-------|
| **Issue CRUD** | | | |
| Create | `bd create` | `br create` | Parity |
| List | `bd list` | `br list` | Parity; `br` has richer robot output |
| Show | `bd show` | `br show` | Parity |
| Update | `bd update` | `br update` | Parity |
| Close | `bd close` | `br close` | Parity |
| Reopen | `bd reopen` | `br reopen` | Parity |
| Delete | `bd delete` | `br delete` | Parity |
| **Dependencies** | | | |
| Add dep | `bd dep add` | `br dep add` | Parity |
| Remove dep | `bd dep remove` | `br dep remove` | Parity |
| List deps | `bd dep list/graph` | `br dep graph` | Parity |
| Ready work | `bd ready` | `br ready` | Parity |
| **Sync** | | | |
| Export JSONL | `bd export jsonl` | `br export --format jsonl` | Parity |
| Import JSONL | `bd import jsonl` | `br sync --import-only` | Parity |
| Sync full | `bd sync` | `br sync` | Parity; `br` adds `--flush-only`/`--import-only` |
| Sync status | `bd status` | `br sync --status` | Parity |
| Sync witness | — | `br sync --witness` | `br`-exclusive |
| **Query** | | | |
| Query DSL | `bd query` | `br list --filter` | Parity; `br` adds `--filter` on `list` |
| **History** | | | |
| Audit log | `bd log` | `br audit` | Parity |
| Diff | `bd diff` | `br diff` | Parity |
| Snapshot | `bd snapshot` | `br snapshot` | Parity |
| Restore | `bd restore` | `br history restore` | Parity |
| Prune | `bd prune` | `br history prune` | Parity |
| **Config** | | | |
| Init | `bd init` | `br init` | Parity |
| Config | `bd config` | `br config` | Parity |
| Doctor | — | `br doctor` | `br`-exclusive; health checks + diagnostics |
| **Worktree** | | | |
| Worktree info | `bd worktree` | `br worktree` | Parity |
| Worktree create | `bd worktree add` | `br worktree add` | Parity |
| **Hooks** | | | |
| Hook install | `bd hooks install` | `br hooks install` | Parity |
| Hook list | `bd hooks list` | `br hooks list` | Parity |
| Hook run | — | `br hooks run` | `br`-exclusive |
| **Scheduler** | | | |
| Scheduler | — | `br scheduler` | `br`-exclusive |
| **Agent Integration** | | | |
| Agent info | — | `br agents` | `br`-exclusive |
| Robot docs | — | `br robot-docs` | `br`-exclusive |
| MCP serve | — | `br serve` | `br`-exclusive |
| Session context | — | `br agents session` | `br`-exclusive |
| **Formula** | | | |
| Formula apply | `bd formula apply` | `br formula apply` | Parity |
| **Wisp** | | | |
| Wisp commands | `bd wisp` | `br wisp` | Parity |
| **Export** | | | |
| CSV export | `bd export csv` | `br export --format csv` | Parity |
| Markdown | `bd export markdown` | `br export --format markdown` | Parity |
| **Triage** | | | |
| Graph analysis | `bv` | `bv` (external) | Both use external `bv` tool |
| **Federation** | | | |
| Federation | — | `br federation` | `br`-exclusive; experimental |

## `br`-Exclusive Features

- **Atomic sync operations** — `--flush-only`, `--import-only`, `--witness`
- **MCP stdio server** — `br serve` exposes issue tracker tools/resources via Model Context Protocol
- **Doctor diagnostics** — `br doctor` performs health checks, schema validation, and invariant testing
- **Agent integration** — `br agents`, `br robot-docs`, `br agents session` for AI coding agent workflows
- **Formulas** — `br formula apply` for creating issues from resolved formulas
- **Wisp** — `br wisp` for lightweight subtask tracking
- **Hooks run** — `br hooks run` to invoke hooks manually (not just via git triggers)
- **Scheduler** — `br scheduler` for cron-like issue reminder and status tracking
- **Federation** — `br federation` for cross-project coordination (experimental)

## `bd`-Exclusive Features (not in `br`)

See the intentionally out-of-scope table above.

## Migration Notes

- **`bd sync`** → `br sync --flush-only` for export, `br sync --import-only` for import
- **`bd query "..."`** → `br list --filter "..."` (with caveats — `br` filters are applied as SQL + in-memory predicate)
- **`bd daemon start`** → use `br serve` (MCP) or direct CLI invocation
- **`bd prime`** → `br robot-docs` + `br agents session` for session context
- **`bd remember`** → external LLM memory system
- **`bd dolt`** → no equivalent; `br` uses SQLite-native sync
