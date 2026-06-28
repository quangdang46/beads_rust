# Duplication Map - 2026-04-24-shrink-pass-1

Generated: 2026-04-24T22:06:37Z
Tools: `rg`, `ast-grep`, `wc`, codebase architecture search

## Scanner inventory

- `skill_inventory.json`: sibling skill availability from the simplify bootstrap script.
- `ast_grep_functions.json`: structural Rust function inventory from `ast-grep`.
- `slop_scan_seed.txt`: capped `rg` seed scan for unwrap/expect and simple error-forwarding patterns.
- `bv_triage.json`: graph-aware task context.
- `loc_before.txt`: Rust LOC snapshot.

## Candidates

| ID | Kind | Locations | Type | Notes |
|----|------|-----------|------|-------|
| D1 | test helper clone | `src/format/markdown.rs:386`, `src/format/syntax.rs:227` | II | Each file repeats five `OutputContext::with_mode` helpers with only `OutputMode` differing. |
| D2 | storage row extraction cluster | `src/storage/sqlite.rs` row accessors around issue/dependency/comment parsing | III | Several local row parsing/accessor shapes can likely be factored into narrow helpers, but the file is reserved by another agent. |

## D1 callsite census

`src/format/markdown.rs` defines `plain_ctx`, `json_ctx`, `quiet_ctx`, `toon_ctx`, and `rich_ctx`, then calls them at lines 409, 420, 427, 434, 441, and 557.

`src/format/syntax.rs` defines the same five helpers, then calls them at lines 250, 258, 265, 272, 279, 286, 296, 304, 311, and 393.

The mechanically equivalent collapse would replace the five helpers in each test module with one `ctx(mode: OutputMode) -> OutputContext` helper and pass the mode at each callsite.

## D2 callsite census

The initial architecture/candidate search identified repeated row parsing and row accessor patterns in `src/storage/sqlite.rs`, but `src/storage/sqlite.rs` had an active exclusive Agent Mail reservation held by `JadeCondor`. No deeper edit card was prepared because touching that file would conflict with active peer work.
