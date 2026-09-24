# Upstream Gap Audit — `beads_rust` vs `Dicklesworthstone/beads_rust` vs `gastownhall/beads`

**Date:** 2026-09-25
**Subject:** local `br` at `69f972d9` (Cargo version `0.1.3`)
**Compared against:**
- **DWS** — `github.com/Dicklesworthstone/beads_rust`, Rust, v`0.6.0`, commit `3a3ad36`
- **GO** — `github.com/gastownhall/beads`, Go, commit `c507f3b`

**Method:** 272-agent multi-agent audit. 12 capability domains scanned, every finding put through adversarial refutation and upstream corroboration, followed by a completeness-critic pass whose findings were verified the same way.

**Status:** read-only static analysis. No code was modified. No binary was executed.

---

## 1. Executive summary

LOCAL `br` is **not dramatically smaller** than its Rust upstream once measured on the same basis — **314 `.rs` files / ~319K LOC** versus DWS's **327 / ~437K** (73% of the LOC) and GO's **2943 `.go` / ~860K** (37%). The gap is **depth of hardening, not code volume**.

LOCAL is a **divergent fork**, not a strict subset. It shares ~20 identical top-level modules with DWS, yet LOCAL *retains* a GO-lineage subsystem set — formula DSL, recipes, worktree, git-hooks, merge-slot, query DSL, web/HTTP — that the DWS fork dropped entirely. **Porting DWS wholesale would therefore be a net regression.** See [§5](#5-where-local-is-ahead--do-not-"fix"-these).

The single most important missing thing is **optimistic-concurrency control on the issue row** — no `--if-unchanged` / `--if-assignee` / `--if-status`, no `if_unchanged` on the MCP `update_issue`, and no `row_version`/revision token. When two agents edit the same issue concurrently, one write silently overwrites the other and the tracker **cannot detect the lost update**. This is the core failure mode of an agent fleet sharing one tracker.

**One theme explains the majority of surviving findings: DWS hardened the write path against concurrent and crashing writers, and LOCAL has not.** The poisoned-WAL-index recovery, the three-layer write authority, the reviewed sync reconcile plan/apply, `if_unchanged`, the linearizability oracle, and the `doctor migrate-schema` lifecycle are all facets of that one missing investment — not six independent features.

After merging ~90 surviving findings down to **21 distinct gaps**, the shape is:

| Cluster | Count | Ranks |
|---|---|---|
| **Data integrity & durability** | **6** | 1, 2, 3, 5, 10, 14 |
| **Graph / query / CLI parity** | 4 | 6, 11, 12, 13 |
| **Testing / CI / supply-chain** (derivative — see note) | 3 | 16, 17, 18 |
| **Go-only enterprise integrations** (lowest priority) | 3 | 19, 20, 21 |
| **Agent surface / MCP** | 2 | 8, 15 |
| **Observability / self-diagnosis** | 1 | 9 |
| **Model lifecycle** | 1 | 7 |
| **Robustness / portability** | 1 | 4 |

**Two honesty notes that matter for deciding what NOT to "fix":**

1. LOCAL is **ahead** of DWS in the seven GO-lineage subsystems listed in §5.
2. The test/CI cluster (#16, #17, #18) is **derivative**, not a data-loss fix in itself — but it is precisely what let every parity gap in this report ship unnoticed, because CI runs only `cargo test --lib` and zero integration tests.

---

## 2. Scale

Measured on a consistent whole-repo basis (excluding `target/`):

| Repo | Files | Total LOC | `src/`-only files | `src/`-only LOC |
|---|---|---|---|---|
| **LOCAL** `quangdang46/beads_rust` | 314 `.rs` | **319,282** | 148 | **193,815** |
| **DWS** `Dicklesworthstone/beads_rust` | 327 `.rs` | ~437,000 | 127 | ~269,000 |
| **GO** `gastownhall/beads` | 2943 `.go` | ~860,000 | — | — |

LOCAL is **~73% of the Rust upstream's LOC** and **~37% of Go's**.

Of LOCAL's 314 files, **155 live in `tests/`** (~125K LOC), plus 8 in `fuzz/`, 2 in `benches/`, 1 `build.rs`. The test suite is substantial in volume — the problem is that CI does not run it (#16).

> **Correction applied during the audit.** The initial scouting pass compared LOCAL's `src/`-only count (148) against DWS's whole-repo count (327), which exaggerated the size gap at ~44%. Re-measured on a like-for-like basis, the figure is ~73%. The "depth, not volume" framing supersedes the earlier number.

---

## 3. Method

**Phase 1 — Scan.** Twelve domain agents, each performing a genuine **three-way** diff (LOCAL vs DWS vs GO), not a two-way. Domains: `cli-surface`, `storage-durability`, `sync-jsonl`, `graph-deps`, `mcp-agent-surface`, `query-search`, `triage-analytics`, `integrations-external`, `observability-ops`, `model-lifecycle`, `cli-ux-agent-contract`, `testing-parity-docs`.

Every finding was required to carry **local-side search evidence** for its absence (e.g. `rg -n 'capacity' src/ → 0 hits`) in addition to an upstream `file:line` citation. Findings resting only on an upstream citation were discarded.

**Phase 2 — Adversarial verification.** Two verifiers per finding:

- **Refuter** — instructed to *disprove* the gap exists in LOCAL. It searches for feature-gated implementations (`cfg(feature = ...)`), renamed commands, MCP-reachable equivalents, sibling modules (`src/query/`, `src/web/`, `src/formula/`, `src/merge_slot/`, `src/worktree/`, `src/hooks/`, `src/recipes/`), alternative mechanisms achieving the same outcome, and `CHANGELOG.md` / `UPGRADE_LOG.md` / `docs/` statements of deliberate removal. **Default verdict: refuted if uncertain.** A finding survives only if the refuter explicitly failed to find an implementation.
- **Corroborator** — opens the cited upstream `file:line`, corrects `present_in` against the *other* upstream, and aggressively downgrades severity for Go-only enterprise concerns, experimental features, and features LOCAL has documented as deliberately excluded.

**Phase 3 — Completeness critic.** A single agent that read DWS's `CHANGELOG.md` (179KB), `CHANGELOG_RESEARCH.md` (105KB) and `UPGRADE_LOG.md` (100KB) plus GO's `CHANGELOG.md` (546KB) to surface capabilities the domain sweep could not have found. Its 7 findings went through the same verification.

### Per-domain yield

| Domain | Reported | Survived | Refuted |
|---|---|---|---|
| `mcp-agent-surface` | 12 | **12** | 0 |
| `graph-deps` | 12 | 11 | 1 |
| `observability-ops` | 12 | 10 | 2 |
| `integrations-external` | 10 | 7 | 3 |
| `query-search` | 10 | 9 | 1 |
| `sync-jsonl` | 12 | 9 | 3 |
| `testing-parity-docs` | 12 | 8 | 4 |
| `cli-ux-agent-contract` | 12 | 8 | 4 |
| `model-lifecycle` | 8 | 7 | 1 |
| `cli-surface` | 12 | 6 | 6 |
| `storage-durability` | 4 | 4 | 0 |
| `triage-analytics` | 6 | 2 | 4 |
| completeness critic | 7 | 6 | 1 |
| **Total** | **129** | **99** | **30** |

**Aggregate:** 272 agents · ~3h wall-clock · 32.7M subagent tokens · 8377 tool calls. A **23% discard rate** (30/129) is the visible cost of the adversarial stage — the refuters did real work.

### Reproducing

```bash
mkdir -p /tmp/beads_gap_audit && cd /tmp/beads_gap_audit
git clone --depth 1 https://github.com/Dicklesworthstone/beads_rust.git dicklesworthstone_beads_rust
git clone --depth 1 https://github.com/gastownhall/beads.git gastownhall_beads
```

The workflow script is persisted at `db77c8b9-086b-4890-81eb-c93b0acfd310/workflows/scripts/beads-gap-audit-wf_f2527e11-845.js` and the full per-agent return values are in that run's `journal.jsonl`.

---

## 4. Ranked gaps

Severity is calibrated for an **agent-first** tracker, not for a general-purpose CLI.

---

### 1. No optimistic-concurrency control on the issue row — concurrent agent edits silently lose data

| | |
|---|---|
| **Severity** | **critical** |
| **Theme** | agent-workflow-blocker / data-integrity |
| **Present in** | **both upstreams** (DWS + GO) |
| **Effort** | medium |

**Why:** Two agents updating the same issue race and one write silently overwrites the other, with no token, precondition, or error to detect the loss. This is the defining failure mode of an agent fleet sharing one tracker — and unlike most other gaps here, *both* upstreams solved it, so there is no design question, only unfinished work.

**Local evidence:** `if_unchanged` / `if-assignee` / `if-status` / `row_version` / `expect_updated_at` / `lost_update` — **0 hits** across `src/` and `tests/`. `BeadsError` (`src/error/mod.rs`) has no `UpdatePreconditionFailed` variant. The `update_issue` MCP schema has no precondition. The only concurrency concept in LOCAL is doctor lock-contention `ConcurrencyLost`, which is unrelated to read-modify-write.

**Upstream evidence:**
- DWS — `src/cli/mod.rs:1341` `if_unchanged`; `src/storage/sqlite.rs:7565-7580` in-transaction compare → `UpdatePreconditionFailed`; MCP `tools.rs:2398` and `:2110`; `e2e_mcp_protocol.rs:743`
- GO — `cmd/bd/update.go:997-998` `--if-assignee` / `--if-status` (mismatch exits 13); `row_version` / `revision` token at `internal/types/types.go:99`

**Independently re-verified** during report writing: the six identifiers above return zero matches in `src/` and `tests/`. Confirmed.

---

### 2. Poisoned WAL index (`-shm`) bricks every mutating command with no detection or recovery

| | |
|---|---|
| **Severity** | **high** |
| **Theme** | storage-durability / crash-safety |
| **Present in** | DWS only |
| **Effort** | large |

**Why:** After a crash leaves a zero-page `-shm` beside a valid WAL, every mutating command fails with an undiagnosable "database is busy" and there is no supported recovery. The workspace is bricked, and committed records may live only in the WAL.

**Local evidence:** `poisoned_index_present` / `wal_index_needs_recovery` / `recover_wal_index_for_startup` — 0 hits. `doctor` only reports sidecar *presence* (`doctor.rs:1784-1824`) and treats WAL-without-SHM as the normal frankensqlite state, so a poisoned zero-page `-shm` matches no branch. No `doctor migrate-schema recover` subcommand exists.

**Upstream evidence:** DWS `franken_sync/wal_index.rs` probe / `poisoned_index_present`; startup gate `main.rs:126-140`; quarantine-on-open `franken_sync.rs:292`; remediation `doctor.rs:1249-1268`; crash-injection harness (`BR_TEST_507_CRASH_STAGE`).

**Independently re-verified:** `src/franken_sync` does not exist in LOCAL. Confirmed.

> This is the scoped slice of a larger surface — DWS's `franken_sync` is a whole alternate storage engine (WAL index, prepared transactions, retry). A full storage-engine diff was **not** performed; see [§7](#7-what-was-dropped--and-what-was-not-covered).

---

### 3. Context-accumulating fields can be destructively truncated on update with no guard

| | |
|---|---|
| **Severity** | **critical** |
| **Theme** | data-integrity |
| **Present in** | DWS only |
| **Effort** | medium |

**Why:** An agent rewriting notes/description/acceptance with a shorter partial string silently wipes the accumulated context other agents wrote, and nothing refuses or warns. Given #1, the agent may not even be the legitimate last writer — it just silently wins.

**Local evidence:** No destructive / half-length guard and no `append_notes` (0 hits in `src/`). `UpdateArgs` exposes a bare `force` + `notes_push` with no refusal logic. `BeadsError` has no truncation/shrink variant.

**Upstream evidence:** DWS `src/cli/commands/update.rs:1576-1673` — three-tier guard (empty, <half length, <half words refused) per issues #467 / #481; `--append-notes` / `--add-acceptance` as the safe path.

**Independently re-verified:** `append.notes` / `add.acceptance` / `half.length` / `truncat` return 0 hits in `src/cli/commands/update.rs` and `src/error/mod.rs`. Confirmed.

---

### 4. Text-mode output aborts with SIGABRT on a closed pipe instead of dying like a Unix filter

| | |
|---|---|
| **Severity** | **critical** |
| **Theme** | robustness / portability |
| **Present in** | DWS only |
| **Effort** | small |

**Why:** `br list | head` — or any text-mode command into a closed pipe — panics, and with `panic = "abort"` that becomes a SIGABRT core dump. Ordinary agent shell pipelines crash rather than exit cleanly.

**Local evidence (precise):** LOCAL *does* have broken-pipe handling, but **only on the structured/JSON path** — `is_broken_pipe_serialization_error` at `src/output/context.rs:55` and `:1093`, guarding serde_json serialization failures. The **text-mode path is not covered**: `src/output/context.rs:1080` is a bare `println!("{content}")`, which panics on EPIPE. `Cargo.toml:151` sets `panic = "abort"`. `src/lib.rs:22` is `#![forbid(unsafe_code)]`, so the DWS fix needs one sanctioned carve-out. `src/shutdown.rs:79-82` registers only SIGHUP/INT/TERM. No broken-pipe test exists.

**Upstream evidence:** DWS issue #434 (`CHANGELOG.md:1450-1468`) restores `SIG_DFL` at startup and dies with signal 13; `tests/e2e_broken_pipe.rs`; implemented in `src/main.rs:45-46` via `beads_rust::shutdown::restore_default_sigpipe()`, gated by `should_restore_default_sigpipe(&cli, structured_output)` (`main.rs:1881`) — i.e. DWS restores `SIG_DFL` for text output but deliberately **keeps SIGPIPE ignored for structured output**, with tests at `main.rs:4060` and `:4083`.

> **Refinement during report writing.** A less careful reading would call this "no broken-pipe handling at all". The accurate statement is the one above: JSON output is guarded, text output is not. That distinction matters because it means the fix is narrow and does not disturb the working JSON path.

---

### 5. No three-layer write authority — one unbound `.write.lock` lets hard-link aliases write concurrently

| | |
|---|---|
| **Severity** | low *(data-integrity class)* |
| **Theme** | storage-durability / concurrency |
| **Present in** | DWS only |
| **Effort** | large |

**Why:** Two hard-link aliases of one database serialize on nothing (SQLite's own lock aside), so independent writers can interleave and corrupt.

**Local evidence:** `DatabaseFamilyWriteLock` / `DatabaseOpenerLease` / `authority` / OFD / `db_inode_lock` — 0 hits. The entire lock surface is `blocking_write_lock` on `.write.lock` (`src/sync/mod.rs:58-166`). Symlink escape is validated (`sync/path.rs`), but the route is not re-checked mid-mutation.

**Upstream evidence:** DWS `DatabaseFamilyWriteLock` + `JsonlFamilyWriteLock` (`sync/mod.rs:440-479`), `db_inode_lock.rs` OFD lock, `DatabaseOpenerLease`, authority-sha256 re-verification. GO has a shared/exclusive flock family.

---

### 6. Dependency table keyed on `(issue, depends_on)` only — a pair can hold exactly one dependency type

| | |
|---|---|
| **Severity** | medium |
| **Theme** | graph-deps / agent-workflow |
| **Present in** | DWS only |
| **Effort** | large |

**Why:** `br dep add A B -t related` after `-t blocks` returns "exists" and drops the type. Agents cannot express a multi-typed edge, and dependency-graph reasoning silently loses information.

**Local evidence:** `src/storage/schema.rs:140` — `PRIMARY KEY (issue_id, depends_on_id)`, 2 columns. The add probe omits type (`sqlite.rs:7656`). Remove has no `--type`; `DepRemoveArgs` has no `dep_type`. The CHANGELOG records only a type-column backfill, not the one-type limit.

**Upstream evidence:** DWS `schema.rs:359` — `PRIMARY KEY (issue_id, depends_on_id, type)`, 3 columns, with a v19 migration cookie; type-scoped add at `sqlite.rs:12618`; ambiguous-remove guard at `sqlite.rs:12886-12911`; `dep remove --type` at `cli/mod.rs:2269`. GO legacysqlite also uses a 3-column PK.

**Independently re-verified by direct source comparison:**

| | LOCAL | DWS |
|---|---|---|
| PK | `schema.rs:140` → `(issue_id, depends_on_id)` | `schema.rs:359` → `(issue_id, depends_on_id, type)` |

Confirmed exactly. Note that LOCAL already carries the supporting indexes (`idx_dependencies_depends_on_type` at `schema.rs:147`) — the index exists without the constraint that would use it, which suggests the 3-column change was partially anticipated and never completed.

---

### 7. Acceptance criteria cannot be ticked or edited item-by-item on any surface

| | |
|---|---|
| **Severity** | **critical** |
| **Theme** | agent-workflow / model-lifecycle |
| **Present in** | DWS only (field exists in both) |
| **Effort** | medium |

**Why:** An agent that finishes one checklist item must rewrite the whole acceptance field — which is exactly the destructive pattern in gap #3, and a common source of lost checklist state.

**Local evidence:** No `check_acceptance` / `uncheck_acceptance` / `add_acceptance` / `acceptance_items` (0 hits in `src/` and `tests/`). Whole-field `acceptance_criteria` only; no acceptance parser. (The 179 field references are storage/format plumbing, not a parser.)

**Upstream evidence:** DWS `src/model/acceptance.rs` (746 lines, byte-preserving), CLI `--check-acceptance` / `--add-acceptance`, read surface `acceptance_items` (`show.rs:920`), MCP write surface (`tools.rs:2445`), issues #477 / #480. GO exposes acceptance over MCP (`models.py:182`).

---

### 8. MCP tool errors are JSON-RPC errors (not `isError` results) and the MCP transport has zero e2e tests

| | |
|---|---|
| **Severity** | medium |
| **Theme** | agent-surface / MCP |
| **Present in** | DWS only (errors) · both upstreams (tests) |
| **Effort** | large + medium |

**Why:** MCP clients expect tool failures as `isError` results with structured content, not protocol-level errors. And with no wire-level test, the entire primary agent surface is unverified.

**Local evidence:** All 7 tool `fn call`s return `McpResult<Vec<Content>>` as `Err`. No `is_error` / `structured_content` / `FinalCallToolResult` (0 hits). `find tests -iname '*mcp*'` yields no `.rs` test; no `fastmcp` in `tests/`.

**Upstream evidence:** DWS `final_tool_result` sets `is_error` + `structured_content` (`tools.rs:1470-1498`); `tests/e2e_mcp_protocol.rs` (3069 lines) asserts `result.isError` and `result.structuredContent`; `e2e_mcp_shutdown.rs`.

---

### 9. `br doctor migrate-schema` — no receipt-bound, reversible schema-migration lifecycle

| | |
|---|---|
| **Severity** | medium |
| **Theme** | observability / self-diagnosis |
| **Present in** | DWS only |
| **Effort** | large |

**Why:** There is no plan/apply/undo path for a schema change, so the crash-recovery and torn-upgrade scenarios that need it have no operator-facing remedy. **This is the recovery entry point for gap #2** — the two are one work item, not two.

**Local evidence:** `migrate-schema` — 0 hits. `DoctorSubcommand` has only Capabilities / RobotDocs / Health / Ls / Undo / Explain. No schema_migration module; no `br.doctor.schema_migration.*` receipt.

**Upstream evidence:** DWS `doctor_subsystems/schema_migration.rs:57-63` — plan / prepared / commit-ready / applied / failed / undo receipts, drift refusal, verified recovery bundle, post-commit all-row integrity. `DoctorMigrateSchemaArgs` with Recover / Plan / Apply / Undo.

---

### 10. `br sync` has no reviewed reconcile plan/apply — only a destructive `--rebuild`

| | |
|---|---|
| **Severity** | medium |
| **Theme** | sync-correctness / data-integrity |
| **Present in** | DWS only |
| **Effort** | large |

**Why:** A bot cannot preview and gate a potentially destructive reconciliation before running it. The only path resets the DB to the JSONL, deleting orphan state.

**Local evidence:** `reconcile` / `reconcile-additive` / `expect_plan_sha256` / `resolve_source_ids` / `apply_sync_reconcile` — 0 hits. Only `--error-policy` / `--force`. `--rebuild` is the opposite: a destructive reset to JSONL.

**Upstream evidence:** DWS `--reconcile` / `--reconcile-additive` (read-only, hash-bound), plan/apply functions (`sync/mod.rs:5058-5123`, `15557-15684`), `--apply` + `--expect-plan-sha256`, CHANGELOG #1083.

---

### 11. Core CLI flag parity: `show` (5 vs 12 flags), `close` (`--reason-file` / `--claim-next`), global `-C` / `--readonly` / `--sandbox`

| | |
|---|---|
| **Severity** | low |
| **Theme** | cli-parity (Rust/Go) |
| **Present in** | both upstreams |
| **Effort** | small |

**Why:** Agents routinely script `br show --json --include-dependents`, or need a global `--readonly` / read-only posture and `-C`. These are cheap flags that are plainly just not ported.

**Local evidence:** `ShowArgs` has only ids / format / wrap / stats / oneline. `CloseArgs` and the global `Cli` lack `--reason-file` / `--claim-next` / `-C` / `--readonly` / `--sandbox` / `--global` / `--ignore-schema-skew` (0 hits). The *payload* exists — `show.rs:498-763` builds dependents — but the caller cannot toggle parts off.

**Upstream evidence:** GO `cmd/bd/show.go:299-311` (include-comments, include-dependents, brief-deps, as-of, children, refs, current, watch, long, local-time), `close.go:388-393`, `main.go:883-898` (`-C`, `--readonly`, `--sandbox`, `--global`, `--ignore-schema-skew`).

---

### 12. WIP capacity admission control (`br capacity`) entirely absent

| | |
|---|---|
| **Severity** | low |
| **Theme** | triage / agent-workflow |
| **Present in** | DWS only |
| **Effort** | large |

**Why:** Without hierarchy-aware WIP limits, a swarm can pile unlimited concurrent work onto a subtree — which is the failure mode agent swarms actually hit.

**Local evidence:** No `Capacity` variant in the 65-variant `Commands` enum. Every `capacity` hit is `Vec`/`HashSet::with_capacity`. No `wip_limit` / `max_in_progress`.

**Upstream evidence:** DWS `cli/mod.rs:765-768` + `commands/capacity.rs` (exempt / renew / revoke / exemptions with approval provider, expiry, audit) + `close_policy.rs` `CapacityAdmissionRule` + hierarchy-aware counting + stats integration.

---

### 13. Query / filter / sort vocabulary materially smaller than GO

| | |
|---|---|
| **Severity** | low-medium |
| **Theme** | query / filtering parity |
| **Present in** | GO + DWS |
| **Effort** | small-medium |

**Why:** `br ready -p 0-1`, a glob label like `tech-*`, or a due-date range errors out or matches nothing. Agents cannot express common scoping in one call and fall back to multiple round-trips.

**Local evidence:** Priority parse rejects `0-1` (`model/mod.rs:172-185`). Label match is exact-only (`query/evaluator.rs:554`). Sort keys limited to 4 (`list.rs:854`). No due / defer / absence filters. Search covers title/description/id only; search JSON omits `total` / `has_more`.

**Upstream evidence:** GO `parse_priority_filter` (`0-1`), `--label-pattern` / `--label-regex`, `--due-after` / `--defer-*`, 9 sort keys, `--no-labels` / `--empty-description`. DWS `parse_priority_filter` + comment-body search + `has_more` + `--fields` projection.

---

### 14. No MIGRATION-FREEZE marker — an operator cannot quiesce a workspace to stop writes

| | |
|---|---|
| **Severity** | medium |
| **Theme** | storage-durability |
| **Present in** | GO only |
| **Effort** | medium |

**Why:** There is no supported way to stop every writer before a risky migration, so a human or agent can slip a write past whatever quiesced the workspace.

**Local evidence:** `MIGRATION-FREEZE` / `freeze_marker` — 0 hits. The only ReadOnly is an internal process-level `DisplayMode` (`write_combining.rs:70`), not an external gate. The `readonly` references in `doctor` are filesystem *observations*, not a write veto.

**Upstream evidence:** GO `internal/migration/freeze.go` — `MIGRATION-FREEZE` walked to fs root, `BD_MIGRATION_FREEZE_FILE` override, `ExitMigrationFrozen`; `CheckReadonly` at every write command; `doctor` deliberately has no override.

---

### 15. MCP agent-experience gaps

| | |
|---|---|
| **Severity** | low-medium |
| **Theme** | agent-surface / MCP |
| **Present in** | GO + DWS |
| **Effort** | medium / large |

**Why:** An agent cannot switch workspaces over one server, discover what the server can do cheaply, get a scoped ready-work answer in one call, attribute closes, or shut the server down cleanly (risking WAL loss).

**Local evidence:** `run_serve` binds one `.beads` dir once (`mcp/mod.rs:699-732`) with no setter. No `discover_tools`. `ready` / `blocked` resources are parameterless. No compact / brief / fields. `run_stdio` has no shutdown path. `close_issue` is only `{id, ids, reason}`, while the CLI `CloseArgs` already carries `agent_name` / `harness` / `model`.

**Upstream evidence:** GO `context` set/show workspace tool, `discover_tools` / `get_tool_info`, filterable `ready_work(brief, fields)`. DWS shutdown watcher (`e2e_mcp_shutdown.rs`) + `close` `agent_name` / `harness` / `model` + `ensure_not_shutting_down` on every call.

---

### 16. CI runs zero integration tests across 155 test files / 121,369 LOC

| | |
|---|---|
| **Severity** | high *(but derivative)* |
| **Theme** | testing / CI |
| **Present in** | both upstreams |
| **Effort** | medium |

**Why:** Every parity gap in this report shipped *because* CI ran only `cargo test --lib`. This is the multiplier that would have caught the rest — not a single data-loss fix.

**Local evidence:** `ci.yml` runs only `cargo fmt` and `cargo test --lib --all-features`. No clippy. No `test --test`. 155 `tests/*.rs` files excluded. `conformance.sh` is never referenced by a workflow.

**Upstream evidence:** DWS `ci.yml` 5-way shard matrix + `test-shard.sh` running every `tests/*.rs`; GO test flags `-v -race -short -coverprofile`; `conformance.yml` in both.

---

### 17. No multi-process linearizability oracle or model-based storage differential test

| | |
|---|---|
| **Severity** | high |
| **Theme** | testing (durability proof) |
| **Present in** | DWS only |
| **Effort** | large |

**Why:** These oracles are the automated proof that the concurrent/crashing-writer hardening (gaps #2, #5, #6, #10) actually holds. Without them, those fixes are unverified.

**Local evidence:** 0 files mention lineariz / reference model. `e2e_concurrency.rs` and `workspace_failure_replay.rs` exist but build no sequential oracle.

**Upstream evidence:** DWS `tests/linearizability_multiprocess.rs` (Wing & Gong linearization search, `PRAGMA integrity_check`, JSONL match) + `tests/model_based_storage.rs` (engine-free `BTreeMap` oracle), both wired into CI reliability-gates.

---

### 18. Repo's own immutable action-pin supply-chain policy is violated and its pin test never runs

| | |
|---|---|
| **Severity** | medium |
| **Theme** | CI / supply-chain |
| **Present in** | both upstreams |
| **Effort** | small |

**Why:** Mutable GitHub-Action tags are a live supply-chain risk, and the repo **already wrote the policy and a 937-line detector for it**. The only thing missing is running it.

**Measured state:**

| Workflow | SHA-pinned | Mutable-tag refs |
|---|---|---|
| `audit.yml` | 3 | 0 |
| `ci.yml` | 0 | 4 |
| `release.yml` | 0 | 29 |
| **Total** | **3** | **33** |

`action-pins.jsonl`, `docs/CI_SUPPLY_CHAIN.md`, and `tests/workflow_action_pins.rs` all exist — and `ci.yml` excludes them. The highest-risk file is `release.yml`, which is **entirely** on mutable tags and is the file that publishes releases.

**Upstream evidence:** DWS and GO pin every action to a 40-char SHA with a version comment.

> **Correction applied during report writing.** The upstream audit reported this as "0/27 pinned". Direct enumeration of `.github/workflows/*.yml` gives **36 `uses:` refs total, 3 SHA-pinned** (`audit.yml:29,32,37`), 33 on mutable tags. The substance stands and is slightly worse than first reported; the original "0/27" figure is superseded. See [`docs/CI_SUPPLY_CHAIN.md`](CI_SUPPLY_CHAIN.md) for the canonical policy this currently violates.

---

### 19. `federation` is a non-functional stub

| | |
|---|---|
| **Severity** | info / low |
| **Theme** | integrations |
| **Present in** | GO only |
| **Effort** | large |

**Why:** Registered `https://` peers are rejected at sync time, and the sync clobbers the peer's JSONL **before** importing it. The feature is worse than absent — a documented stub that looks usable.

**Local evidence:** Federation sync exports to the peer path then imports (`federation.rs:390-425`); rejects non-`file://` remotes (`:377-381`); no `--peer` / `--strategy`; `--password` is a bare argv value with no prompt fallback; the table has unused `username` / `password_encrypted` / `sovereignty` columns.

**Upstream evidence:** GO `federation.go` — `--peer` / `--strategy ours|theirs` / `--sovereignty T1-T4`, credentialed Dolt remote, status command; main `bd sync` full pull-push loop with exit codes 0-4.

---

### 20. No external issue-tracker integrations

| | |
|---|---|
| **Severity** | info *(lowest priority)* |
| **Theme** | integrations |
| **Present in** | GO only |
| **Effort** | large |

**Why:** Explicitly the lowest-value tier for an agent-first local tracker. It is a large enterprise surface, not a blocker for autonomous agent workflows, and is best left to downstream consumers.

**Local evidence:** The only GitHub reference is the self-update release constant. No tracker adapter, command, or registry. The `external_ref` column is never touched by network code.

**Upstream evidence:** GO `internal/tracker/` interface + registry + 6 adapters (github / gitlab / jira / linear / notion / ado) + `cmd/bd/{github,gitlab,jira,linear,notion,ado,mail}.go`.

---

### 21. `br web` binds a mutating HTTP API with permissive CORS and zero auth

| | |
|---|---|
| **Severity** | medium |
| **Theme** | integrations / security |
| **Present in** | GO only (auth) · uncertain (stubs) |
| **Effort** | medium / large |

**Why:** A mutating API reachable from a browser with permissive CORS and no token is a real footgun if ever bound beyond loopback. The stub routes make the surface look more complete than it is.

**Local evidence:** `CorsLayer::permissive()` (`web/mod.rs:174`); PATCH / DELETE / POST routes (`:84-155`); the `host` default has no validator and there is no auth or loopback policy; 9 `stub_*` handlers return empty JSON.

**Upstream evidence:** GO `httpapi` requires `--auth-token-file` for non-loopback (`server.go:451` / `:477`), has a DNS-rebinding `checkHost` (`server.go:1487`), and constant-time token auth (`auth.go:45`); one real handler per surface.

> Carried at **low confidence** — the upstream agent flagged `present_in` as uncertain, and the "~40% of routes are stubs" figure is an estimate, not a measured ratio.

---

## 5. Where LOCAL is ahead — do not "fix" these

This is the most important section for deciding what *not* to port. LOCAL is a **divergent fork**, and the DWS direction is not a superset.

1. **GO-lineage subsystems the DWS fork dropped** — formula DSL (`src/formula/`, 5 files), recipes, worktree, git-hooks, merge-slot, query DSL, and a built-in web/HTTP server all ship in LOCAL and in GO but have **no counterpart in DWS `src/`**. Porting DWS wholesale without re-adding these is a net regression.
2. **TOON output format as a first-class agent output mode** — 48 `src` files here, ~47 in DWS, but minimal in GO. LOCAL is genuinely ahead of the canonical tool on token-efficient robot output agents can parse cheaply.
3. **`#![forbid(unsafe_code)]`** — a stricter memory-safety posture than DWS, which uses `unsafe_code = "deny"` with sanctioned carve-outs (e.g. the SIGPIPE restore in #434). It costs the SIGPIPE fix exactly one carve-out and is a real integrity advantage.
4. **Multi-agent "swarm" vocabulary** — mol / wisp / prime / heartbeat is far more present in LOCAL `src` than DWS (e.g. mol 18 vs 1, prime 10 vs 1, heartbeat 2 vs 0). Ahead of DWS; shared with GO.
5. **A `bv` graph-aware triage companion** (PageRank / betweenness / HITS / k-core) ships alongside `br`, exposing graph metrics neither fork offers in-tree.
6. **The degraded Agent-Mail-coordination workflow** — reservation, `br remember`, self-tracking `.beads` — is a first-class process capability that neither upstream fork encodes.

---

## 6. Recommended first moves

Sequenced by value per unit of effort.

1. **Add the optimistic-concurrency precondition end-to-end** — `--if-unchanged` (plus `--if-assignee` / `--if-status`) on `br update`, `if_unchanged` on MCP `update_issue`, enforced *inside* the storage write transaction with a new `UpdatePreconditionFailed` error. Highest-value, lowest-effort data-loss fix for an agent fleet.
   *Touches:* `src/cli/mod.rs`, `src/cli/commands/update.rs`, `src/storage/sqlite.rs`, `src/mcp/tools.rs`.

2. **Restore SIGPIPE to `SIG_DFL` for text-mode output** — one sanctioned `#[allow(unsafe_code)]` carve-out matching DWS #434, gated so structured output keeps SIGPIPE ignored. Removes a core dump from every agent pipeline.
   *Touches:* `src/main.rs` (or equivalent entry), `src/shutdown.rs`.

3. **Add the destructive-truncation guard on context-accumulating fields** (notes / description / acceptance) — refuse empty or sub-half-length/word rewrites unless `--force`, and ship `--append-notes` / `--add-acceptance` as the safe path. Reuses the existing `force` escape hatch.

4. **Promote dependency `type` into the primary key** — `(issue_id, depends_on_id, type)` with a migration, a type-scoped add, and a disambiguating `dep remove --type`. Fixes the "a pair holds one type" correctness bug at the schema layer.

5. **Implement poisoned-WAL-index detection plus `br doctor migrate-schema recover`** — startup gate (`wal_index_needs_recovery`), quarantine-on-open, doctor remediation text, and the crash-injection test harness. This is the crash-safety keystone that #2, #9 and #14 all hang off.

6. **Bring CI to parity** — shard `cargo test` across the 155 `tests/*.rs` files (a 5-way lib/e2e/storage/misc split like DWS), add a conformance job, and **pin all 33 mutable GitHub-Action refs to 40-char SHAs** so the repo's own already-written action-pin test actually runs and [`docs/CI_SUPPLY_CHAIN.md`](CI_SUPPLY_CHAIN.md) is enforced.

7. **Add MCP protocol e2e tests** (spawn the binary, hand-rolled JSON-RPC client) and emit `isError` tool results with structured content instead of JSON-RPC errors — validates and hardens the primary agent surface.

8. **Add the reviewed sync reconcile plan/apply** (`--reconcile` / `--dry-run` / `--apply` / `--expect-plan-sha256`) so a bot can preview and gate a destructive reconciliation before running it, replacing the all-or-nothing `--rebuild`.

> Items 1-3 and 6 are all **small-to-medium** effort and independent of each other — they can proceed in parallel. Items 4 and 5 are **large** and are the natural follow-on.

---

## 7. What was dropped, and what was not covered

**Aggregation decisions** (recorded so the ranking is auditable):

- Several `[CRIT/low]` micro-parity items were **folded into broader buckets** rather than ranked individually: `list --tree`, gate result history, MCP resource-URI plural form, dep-tree status/depth, `config validate`/`schema`, search `has_more`, `--fields` projection. They are subsumed by #11 (CLI parity), #13 (query parity) and #15 (MCP agent-UX).
- `no row_version / revision token` was merged into #1; `federation --password` plaintext into #19; `MCP close_issue` attribution and `MCP shutdown` into #15; `MCP auto-flush ignores history config`, `MCP ready/blocked unfilterable`, `MCP introspection` and `MCP token-efficiency` into #15; `MCP no if_unchanged` into #1.
- GO-only enterprise integrations (#19, #20, and the auth half of #21) were **deliberately ranked last** per the stated priority order — not because they lack value.
- **Coverage caps:** 12 findings per domain and 8 from the critic. No domain hit its cap, so no finding was dropped for volume.

**Not covered — treat these as open questions, not clean gaps:**

- **Full storage-engine diff.** DWS's `franken_sync` is a whole alternate backend. Gaps #2 and #5 are the scoped slice; a complete engine comparison was **not** performed.
- **Performance and benchmark parity** were out of scope. The LOC ratio measures volume, not quality — a larger LOC count is not evidence of a better tool.
- **Runtime verification.** The entire audit is static source + grep. **No binary was executed.** Claims such as "every mutating command fails with database is busy" (#2) and "SIGABRT on a closed pipe" (#4) are read from code paths and have **not** been reproduced under load or under an actual closed pipe.

**Corrections and caveats:**

1. **Scale framing** — the initial 148-vs-327 comparison mixed `src/`-only against whole-repo. Corrected to ~73% (§2).
2. **Action-pin count** — the audit agent reported "0/27 pinned"; direct enumeration gives **3/36** (§18).
3. **SIGPIPE scope** — the audit's "no SIGPIPE handling" is imprecise; JSON output *is* guarded, text output is not (§4).
4. **One agent failure** — 1 of 272 agents (a corroborator) returned without a structured result, losing severity calibration for exactly one finding. It did not affect that finding's survival (the refuter gate is independent).
5. **`br web` stub ratio** (#21) — the "~40% of routes are hardcoded stubs" figure is an estimate carried at low confidence.

---

## Appendix — file size evidence

Recorded because several findings are size claims, and because the LOCAL/DWS size relationship was initially mis-scoped.

All figures below are direct measurements (byte sizes of individual files, or `ls`/`find` counts).

| Path | LOCAL | DWS | GO |
|---|---|---|---|
| `src/cli/mod.rs` (dispatch) | 133 KB | 137 KB | — |
| `src/cli/commands/*.rs` (count) | **64** files | **46** files | `cmd/bd/` (hundreds) |
| `src/cli/commands/doctor.rs` | 768 KB | 1002 KB | `internal/` split |
| `src/cli/commands/sync.rs` | 152 KB | 280 KB | — |
| `src/cli/commands/update.rs` | 72 KB | 144 KB | — |
| `src/cli/commands/dep.rs` | 87 KB | 118 KB | — |
| `src/cli/commands/close.rs` | 88 KB | 111 KB | — |
| `src/cli/commands/schema.rs` | 23 KB | 31 KB | — |
| `src/mcp/tools.rs` | 188 KB | 226 KB | `integrations/beads-mcp/` |
| `src/cli/commands/vcs.rs` | **absent** | 67 KB | `internal/git/` |
| `src/cli/commands/upgrade.rs` | **absent** | 29 KB | — |
| `src/franken_sync/` | **absent** | 4 files | `internal/atomicfile`, `lockfile`, `compact` |
| `tests/*.rs` | 155 files / 121,369 LOC | — | `tests/` + `test/` |
| `CHANGELOG.md` | 68 KB | 179 KB | 546 KB |
| `UPGRADE_LOG.md` | 6.2 KB | 100 KB | — |
| `.github/workflows/` | 3 files | more | more |

LOCAL has **64** command modules vs DWS's **46** — LOCAL's CLI *surface* is broader. But DWS's modules are individually larger in the core write paths (`sync.rs` +84%, `update.rs` +100%, `dep.rs` +36%, `close.rs` +26%, `doctor.rs` +30%), and DWS additionally carries `capacity`, `list_fields`, `vcs`, `upgrade`, and a modularized `search/` directory that LOCAL lacks.

Note on `src/mcp/tools.rs`: LOCAL's is **188 KB**, not absent — only ~17% smaller than DWS's 226 KB. The MCP gap (§4 #8, #15) is therefore **not** a size gap; it is a gap in *which* tools are exposed and in error semantics (JSON-RPC errors instead of `isError` results). All 12 `mcp-agent-surface` findings survived refutation, but file size is not the evidence for them.

> **Two more corrections made while assembling this appendix.** The upstream audit reported "155 test files / 105,934 LOC"; direct measurement gives **155 files / 121,369 LOC**. And an initial draft of this table listed `src/mcp/tools.rs` as absent in LOCAL, which is wrong — it is present at 188 KB.
