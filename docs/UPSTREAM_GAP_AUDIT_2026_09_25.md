# Upstream Gap Audit — `beads_rust` vs `Dicklesworthstone/beads_rust` vs `gastownhall/beads`

**Date:** 2026-09-25
**Subject:** local `br` at `69f972d9` (Cargo version `0.1.3`)
**Compared against:**
- **DWS** — `github.com/Dicklesworthstone/beads_rust`, Rust, v`0.6.0`, commit `3a3ad36`
- **GO** — `github.com/gastownhall/beads`, Go, commit `c507f3b`

**Method:** 272-agent multi-agent audit. 12 capability domains scanned, every finding put through adversarial refutation and upstream corroboration, followed by a completeness-critic pass whose findings were verified the same way.

**Status:** read-only static analysis. No code was modified. No binary was executed.

**Structure of this document:**

| Part | Contents |
|---|---|
| §1–§3 | Verdict, measured scale, and the audit method |
| **§4** | **The 21 ranked gaps — the decision layer** |
| §5–§7 | Where LOCAL is ahead, recommended first moves, what was not covered |
| Appendix 0 | File-size evidence supporting §2 and the §7 corrections |
| **Appendix A** | **The 30 refuted claims — evidence that LOCAL already has these** |
| **Appendix B** | **Per-domain coverage notes, verbatim — how far each sweep actually looked** |
| **Appendix C** | **All 99 verified findings at full resolution — the evidence layer behind §4** |

§4 is the compressed decision layer. Appendices A–C are the unmined research layer: the 129 raw findings, their refutation reasoning, and each domain's own account of its coverage. Appendices A–C are ~590KB of the file; read §4 first and use the appendices to check whether a given gap is one problem or several.

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

### Recovering the finding ↔ verdict linkage

The journal records each agent's structured result under a `key` that is a hash of its prompt, and a `label` that identifies only the *domain* (e.g. `refute:cli-surface`) — not the individual finding. So the journal alone cannot say which verdict belongs to which claim.

Exact linkage was recovered from the 272 per-agent transcripts in the run's `subagents/workflows/wf_f2527e11-845/` directory. Each refute/corroborate transcript embeds its prompt verbatim, including a `CLAIMED GAP: <title>` line, so matching that marker against the 129 titles returned by the find-agents gives a 1:1 map from verdict to claim.

That map was validated by an independent check: it yields **129 linked verdicts, 0 unmatched transcripts, and a 99-survived / 30-refuted split** — which reproduces the audit's own reported `audit_stats` exactly. Had it not, the correlation would have been discarded as unreliable and the appendices reported without the linkage.

Appendices A and C are built from this map, which is why each refuted claim in Appendix A can be shown next to the refuter's reasoning and the corroborator's note.

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

**Nothing was discarded without being written down.** The 21 ranked gaps in §4 are a merge; the full set of 129 findings and their fates are preserved in the appendices:

- **[Appendix A](#appendix-a--the-30-refuted-claims-evidence-that-local-already-has-these)** — all 30 refuted claims with the refuter's reasoning. These are *not* losses: a refutation is a record of what LOCAL already implements.
- **[Appendix B](#appendix-b--per-domain-coverage-notes-verbatim)** — the 13 find-agents' own accounts of what they searched and how far they looked.
- **[Appendix C](#appendix-c--all-99-verified-findings-grouped-by-domain)** — all 99 surviving findings at full resolution, with per-finding description, impact, and both-sided evidence.

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

## Appendix 0 — File size evidence (supports §2 and the corrections in §7)

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

---

## Appendix A — The 30 refuted claims (evidence that LOCAL already has these)

Each claim below was reported by a find-agent, then **disproved** by a refuter whose instruction was to default to `refuted = true`. They are listed here because a refutation is *evidence in the opposite direction*: it records what LOCAL **does** already implement, and how the refuter established that. Anyone re-running this audit should not re-report these as gaps.

*Read `refuter verdict` as the claim's fate, and `what the refuter found` as the reason.*

### A1. No `batch` transaction command and no claim-lifecycle commands (assign, unclaim, reclaim, promote, supersede, duplicate, duplicates, find-duplicates)

- **Domain:** `cli-surface` · **Type:** `missing_feature` · **Claimed severity:** high · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: low

**Claimed upstream evidence**

> GO cmd/bd/batch.go:221 `rootCmd.AddCommand(batchCmd)` with cmd/bd/batch.go:14 `Short: "Run multiple write operations in a single database transaction"`; cmd/bd/assign.go:121 `rootCmd.AddCommand(assignCmd)`, cmd/bd/assign.go:16-22 documents the claim-fence behavior ("Refuses to overwrite another actor's live in_progress claim without --force ...; for a holder-aware transfer prefer 'bd update <id> --if-assignee <holder> -a <new>'") and cmd/bd/assign.go:119 registers `--force`; cmd/bd/unclaim.go:151; cmd/bd/reclaim.go:230; cmd/bd/duplicate.go:52 (registers `duplicateCmd` and `supersedeCmd`); cmd/bd/duplicates.go and cmd/bd/find_duplicates.go register their respective commands.

**Why the refuter killed it**

> Ran: (1) python3 brace-extraction of `pub enum Commands` in /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs -> 68 variants, none of batch/assign/unclaim/reclaim/promote/supersede/duplicate/duplicates/find-duplicates; (2) rg for all 8 names as string literals across src/ -> no command definitions; (3) no `#[command(alias=...)]`/`visible_alias` in mod.rs maps to any of them; (4) Cargo.toml [features] = default=["web"], mcp, web -> nothing feature-gates them; (5) src/mcp/ exposes only list_issues (with a read-only `items` batch), create/update/close, manage_dependencies, project_overview -> no write batch; (6) src/web/api.rs, cli/commands/{query,formula,recipes,sql}.rs -> no batch/atomic primitive (sql is explicitly "read-only: wrapped in a transaction that rolls back"); multi-id update is a per-issue loop (update.rs:483), not one transaction.
> 
> The claim's central rationale is factually false. LOCAL *has* a claim fence: update.rs:1166-1168 build_update sets expect_unassigned/claim_actor/claim_exclusive; update.rs:805-828 validate_route_runtime_guards rejects "issue {id} already assigned to {current}"; sqlite.rs:2732-2762 reads assignee INSIDE the IMMEDIATE transaction (explicit TOCTOU guard) and sqlite.rs:3105-3121 adds a SQL compare-and-set predicate `WHERE id = ? AND (assignee IS NULL OR TRIM(assignee)='' [OR assignee = ?])`. config/mod.rs:4421 `claim_exclusive_from_layer` (keys `claim.exclusive`/`claim-exclusive`) is the documented override == GO's `--force`; CHANGELOG.md:719 records "Atomic claim guard with `claim.exclusive` config and IMMEDIATE transaction". This is TOCTOU-proof, i.e. stronger than GO's pre-check.
> 
> `reclaim` is a documented deliberate omission: docs/CLI_REFERENCE.md:789 states verbatim "There is not a separate reclaim command; the audit comment plus `update --claim` is the documented recovery workflow", and src/coordination.rs:651-672 `reclaim_suggested_commands` emits exactly `br comments add ...` + `br update <id> --claim --json`. The claim mischaracterizes AGENTS.md, whose stale-claim policy prescribes comment+`--claim`, not `bd reclaim`.
> 
> `batch` contracts are implemented but deliberately unwired: src/write_combining.rs (96KB) provides plan_batch, execute_batch_with, BatchLimits, BatchPlan, assemble_batch_report, classify_command, exercised by tests/bench_contention_replay.rs; module docstring says "Pure contracts for future `.write.lock` write combining. This module deliberately does not route commands through a combiner." Only referenced from src/lib.rs:50 and that test.
> 
> `duplicate`/`supersede` underlying capability exists: src/model/mod.rs:331-334 DependencyType::{Duplicates,Supersedes}, settable via `br dep add --type duplicates|supersedes` and exposed through MCP manage_dependencies with aliases duplicate/dupe/dup/supersede/replaces (src/mcp/tools.rs:569-570). `unclaim` is reachable as `br update <id> --assignee ""` (docs/CLI_REFERENCE.md:389 "Assign (empty string clears)"). `assign` was already conceded by the claim.
> 
> Genuinely absent (the surviving gap): `find-duplicates` (no similarity engine — only exact SHA-256 dedup in `br sync`; doctor "duplicate" hits are schema/config row dup detection, e.g. health.rs:62-70, not issue similarity), `promote` (LOCAL `Wisp` at cli/mod.rs:1031 has only list/create/close/gc), and `batch` as a user-facing command.
> 
> present_in=go-only verified: DWS /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/mod.rs yields 47 variants, also none of the 8. Upstream evidence is substantively correct but has one citation error: `Short: "Run multiple write operations in a single database transaction"` is at cmd/bd/batch.go:36, not :14 (line 14 is an import). Other citations check out: batch.go:221 AddCommand, assign.go:121 AddCommand, assign.go:14-22 fence doc, assign.go:119 --force, unclaim.go:151, reclaim.go:230, duplicate.go:52+57, duplicates.go:191, find_duplicates.go:70, promote.go:44.

**Corroborator note**

> The command-name census is right (all 8 exist in GO, none in DWS), but the claim's central mechanism finding is false and its severity rests on that false premise. Three corrections: (1) LOCAL DOES have a claim fence — a TOCTOU-safe SQL compare-and-set at src/storage/sqlite.rs:2735 and :3106-3122, plus a `claim.exclusive` policy knob at src/config/mod.rs:4421 and a test at update.rs:1780 — and it is stronger than GO's `bd assign`, which assign.go:17 self-describes as a mere shorthand. "No fence flag" is technically true only in the narrow sense that it is a config key rather than a per-invocation flag, which is a different and weaker statement. (2) The claim inverts AGENTS.md's reclaim policy: the documented `br update --claim` procedure is made SAFE by the fence (a still-held claim fails loudly), so "unenforceable at the CLI" is backwards. (3) GO's `reclaim` is a lease-TTL reaper requiring a heartbeat subsystem LOCAL does not have, and GO's `promote` is a Dolt wisp→bead table copy — neither is a "claim-lifecycle" command, and neither is a portable one-command port. Additionally, `duplicate` and `supersede` are effectively present in LOCAL as the `duplicates` and `supersedes` dependency types (dep.rs:601), and the JSONL flush is per-command (update.rs:708-712), not per-issue. The genuinely missing items reduce to: no `batch` (low value on SQLite, since its stated purpose is Dolt write-amplification relief), no fence on plain `--assignee` and no `--if-assignee` holder-CAS, and no duplicate *discovery*. Corrected severity: low, not high.

### A2. `list` filter surface missing 35 GO flags and 8 DWS flags (no label exclusion/regex/glob, no --parent/--no-parent, no closed/due/defer windows, no --tree/--flat, no output-shaping flags)

- **Domain:** `cli-surface` · **Type:** `missing_flag` · **Claimed severity:** high · **Claimed present in:** both-upstreams
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: low

**Claimed upstream evidence**

> GO /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/list.go:413 `listCmd.Flags().StringSlice("exclude-label", ...)`; :414 `label-pattern`; :415 `label-regex`; :445 `no-labels`; :448 `skip-labels`; :443 `empty-description`; :456 `brief`; :470 `no-pinned`; :472 `include-templates`; :475 `include-gates`; :478 `include-infra`; :486 `include-ephemeral`; :489 `exclude-type`; :492 `parent`; :493 `filter-parent`; :495 `no-parent`; :501 `wisp-type`; :439-440 `closed-after`/`closed-before`; :505-508 `defer-after`/`defer-before`/`due-after`/`due-before`; :513-514 `tree`/`flat`. DWS /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/mod.rs:1842 `pub struct ListArgs` contains `dry_run`, `eligible_only`, `filters`, `ids`, `no_wrap`, `query`, `transition_comment`, `tree`.

**Why the refuter killed it**

> REFUTED on the claim's central thesis, which is demonstrably false, via a documented, fully-wired mechanism the finder never checked.
> 
> WHAT I RAN AND FOUND
> 
> 1) The decisive find — `--filter` is a Query DSL on `br list`. The last field of `ListArgs` (src/cli/mod.rs:2211) is `pub filter: Option<String>` with doc "Query DSL filter expression". I traced the complete wiring chain: src/cli/commands/list.rs:19 `use crate::query::parse_and_evaluate;` -> :615-617 parses args.filter and merges the returned filters -> :75 `build_filters` returns `(filters, predicate)` -> :156-160 actually executes it: `issues.retain(|issue| pred(issue))` under the comment "Apply in-memory predicate for complex query DSL expressions (OR/NOT)". This is live, not a stub.
> 
> 2) The evaluator (src/query/evaluator.rs, header "Ported from Go beads /internal/query/evaluator.go") implements NotEq across status, type, priority, assignee, owner, unassigned, pinned, title, id, labels, description, notes, closed_at, started_at, spec, ephemeral, template. `apply_str_op` (:488-494) returns `!a.eq_ignore_ascii_case(b)` for NotEq. So the claim's assertion — "LOCAL's whole filter vocabulary is additive-only (every list filter selects *for* a value, none can select *against* one)" — is false. An agent CAN select against a value.
> 
> 3) Specific claimed-missing flags that ARE expressible through `br list --filter`:
>    - `--exclude-label` -> `NOT label=x` (evaluator.rs:555-556, labels NotEq)
>    - `--exclude-type` -> `NOT type=x` (evaluator.rs:498, apply_str_op NotEq)
>    - `--no-pinned` -> `NOT pinned=true` (:519-528)
>    - `--closed-before/after` -> `closed_at`/`closed` with Less/LessEq/Greater/GreaterEq (:569-590); model field `closed_at` at src/model/mod.rs:916
>    - `--has-metadata-key` -> `has_metadata_key=k` (:637-643) — an EXACT identifier match to the claim's quoted upstream flag
>    - `--metadata-field` -> `metadata.k=v` (:644-656)
>    - `--include-templates` -> `template=true/false` or `is_template` (:627-635); model `is_template` at :1004
>    - `--include-ephemeral` -> `ephemeral=true/false` (:620-626); model `ephemeral` at :994
>    - `--no-assignee` -> already a native flag (`--unassigned`), plus DSL `unassigned=true` (:519)
>    Tests exist for these paths (evaluator.rs:925 `closed_at>7d`, :961 `has_metadata_key=component`, :955 `template=false`, :949 `ephemeral=true`).
> 
> 4) Documented deliberate equivalent: docs/BD_VS_BR.md:40 "| Query DSL | `bd query` | `br list --filter` | Parity; `br` adds `--filter` on `list` |" and :96 "`bd query "..."` -> `br list --filter "..."` (with caveats — `br` filters are applied as SQL + in-memory predicate)". This is exactly the "renamed/replaced by a different mechanism" case the task asks me to look for.
> 
> 5) The finder's local_evidence method was inadequate. It ran literal string greps (`exclude-label`=0, `exclude_type`=0, `closed-after`=0...) and counted zero hits, without ever checking the sibling `--filter` flag that exists in the very same struct it parsed. It also misread `--filter` as merely a boolean shortcut for the flags it already found.
> 
> 6) A citation in the claim looks fabricated: it says `wisp_type` hits are "only the `WispType` model enum in wisp.rs". `find src -name "wisp*"` returns only `src/cli/commands/wisp.rs` — there is no model/wisp.rs. The claim's parenthetical is inaccurate.
> 
> UPSTREAM EVIDENCE SANITY CHECK (plausible, both present)
>    GO: verified cmd/bd/list.go contains exclude-label, label-pattern, label-regex (:413-415 region), closed-after/closed-before (:439-440), empty-description, no-assignee, no-labels, skip-labels, include-templates (:472), include-gates (:475), include-infra (:478) — all real, matching the claim's line numbers.
>    DWS: verified `pub struct ListArgs` is at src/cli/mod.rs:1842, and the range contains `tree` ("Group children under their parents with tree connectors"), `query: String`, `filters: ListArgs` (recursive), `ids: Vec<String>`, `no_wrap`. However `dry_run`, `eligible_only`, and `transition_comment` do NOT appear in that range, so the claim's DWS citation is partly imprecise. Notably DWS's own ListArgs embeds a nested `filters: ListArgs` and a `query: String`, suggesting DWS also exposes a query/DSL mechanism — i.e. the two upstreams are closer in design than the claim implies, and LOCAL's `--filter` is squarely in that family.
> 
> RESIDUAL GENUINE GAPS (I am not claiming the claim is 100% wrong)
>    These survive my search and are real:
>    - `--parent` / `--filter-parent` / `--no-parent`: the DSL `parent` field is a STUB, and worse than absent — `evaluate_predicate_on_issue` returns hardcoded `false` (evaluator.rs:621-625) with the comment "for now, treat as unknown field (no match)". So `br list --filter 'parent=X'` silently matches nothing. That is a genuine correctness bug worth filing separately.
>    - `--tree` / `--flat` hierarchy layout: `--pretty` is NOT a substitute. `format_issue_pretty_with` (src/format/text.rs:406-425) draws a per-issue detail tree using `├──`/`└──` over a single issue's detail LINES, not a child-issue hierarchy.
>    - `--label-pattern` / `--label-regex`: the DSL has no glob/regex operator for labels.
>    - `--defer-before/after`, `--due-before/after`: no `due`/`defer` field in the evaluator (grep empty), though model fields `due_at` (:928) and `defer_until` (:932) exist and could back one.
>    - `--no-labels` emptiness: `label=none` errors in the SQL path (:256) and the predicate path treats it as a literal label named "none" (:555), so it is not an emptiness test.
>    - `--skip-labels`, `--brief`, `--no-pager`, `--no-wrap`, `--include-gates`, `--include-infra` (0 hits confirmed), and DWS's `--eligible-only` / `--transition-comment`.
> 
> NET ASSESSMENT
> The gap is real but drastically overstated and its headline framing is wrong. Roughly a third of the claimed "missing" surface (the entire negative/set-based/window/metadata family) is already reachable via `br list --filter`, and that mechanism is documented in docs/BD_VS_BR.md as the intended parity path. What remains is a set of niche filters, one layout flag, and some output-shaping conveniences — not the sweeping "whole filter vocabulary is additive-only" hole claimed at high severity. The `parent` DSL stub is the one item I would escalate, as a silent-wrong-results bug rather than a missing-flag gap.

**Corroborator note**

> Refuted as stated, but keep a smaller real finding.
> 
> The GO half is genuine: `bd list` registers 64 flags vs LOCAL's 36, and a subset really is absent from `br list`.
> 
> The DWS half does not hold up. The 8 "DWS-only list flags" were read out of the wrong structs: `--eligible-only` (src/cli/mod.rs:2029), `--dry-run` (:2037) and `--transition-comment` (:2041) belong to `br epic status` / `br epic close-eligible`; `--ids` (:1976) and `--no-wrap` (:1990) to `br show`; `--query` (:1965) to `br search`; `--filters` (:1968) is a `#[command(flatten)]`, not a flag. Only `--tree` (:1936) is really a DWS list flag, and GO has it too (list.go:513). Repo-wide, DWS has no `exclude-label`, `label-pattern`, `label-regex`, `exclude-type`, `no-parent`, `skip-labels`, `include-infra`, `include-ephemeral`, `wisp-type`, `has-metadata-key`, `metadata-field`, `closed-before` or `due-before` anywhere in code — the only mentions of a few are in docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md, a porting proposal doc, not shipped code. So the gap is GO-only, not both-upstreams, and the "8 DWS flags" line should be deleted, not re-filed.
> 
> The "additive-only" framing is also wrong. LOCAL ships a Query DSL (`-F/--filter`, src/cli/mod.rs:2214) with NotEq and NOT (src/query/ast.rs:9,30) that already covers closed_at windows, template/ephemeral/spec/metadata-key toggles and `NOT status=…` pushed down to SQL. And the claim's own list contains flags LOCAL already has under other names: `--no-assignee` = `--unassigned`, `--has-metadata-key` = `--metadata KEY=VALUE`, `--title` = `--title-contains`, `--state` = `--status`, `--ready` = the `br ready` command, `--no-pager` = N/A (br never pages).
> 
> What is actually worth filing, at LOW severity: hierarchy scoping (`--parent` / `--no-parent`) and true label exclusion (`--exclude-label`). The DSL does not cover these — `-F "label != X"` hard-errors (evaluator.rs:248-249; evaluate() only falls back on ComplexQuery, not InvalidOperator, at 680-683), and `parent` is an explicit `false` stub in the predicate path (evaluator.rs:621-624). Also absent: `--label-pattern`/`--label-regex`, `--no-labels`/`--empty-description`, `--tree`/`--flat`, `--due-*`/`--defer-*` windows, `--brief`/`--skip-labels`. Severity is low rather than high because docs/BD_VS_BR.md:34,40 already claims list parity and documents the DSL as the `bd query` equivalent, and everything but hierarchy scoping is reachable by post-filtering `--json`, which is LOCAL's own documented agent path. `--wisp-type`/`--deps`/`--watch` sit under the documented out-of-scope Go domains (BD_VS_BR.md:5-16) and should not be ported.
> 
> Separate, more interesting defect found while verifying: `-F "type != bug"` is a SILENT NO-OP. NotEq clears the operator guard at evaluator.rs:218-221, but the `if op == Eq` block is skipped so nothing is pushed to `filters.types`, and `can_use_filter_only` returned true for the node — so evaluate() (669-679) reports `requires_predicate: false` with no predicate and `br list` returns every row instead of erroring. A negated filter that quietly returns the unfiltered set is worth an issue on its own.

### A3. Entire Dolt/multi-backend maintenance command family absent (12 GO commands: dolt, backup, compact, gc, vc, branch, migrate, migrate-personal, recompute-blocked, purge, db-proxy-child, conflicts) — also absent from DWS

- **Domain:** `cli-surface` · **Type:** `architecture_gap` · **Claimed severity:** high · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> GO registration sites: cmd/bd/dolt.go:1876 `rootCmd.AddCommand(doltCmd)`; cmd/bd/backup.go:156 `rootCmd.AddCommand(backupCmd)` (with subcommands at cmd/bd/backup_dolt.go:476-478 backupInitCmd/backupSyncCmd/backupRemoveCmd); cmd/bd/compact_dolt.go:235 `compactDoltCmd`; cmd/bd/gc.go:300 `gcCmd`; cmd/bd/vc.go:306 `vcCmd`; cmd/bd/branch.go:86 `branchCmd`; cmd/bd/migrate_personal.go:62 `migratePersonalCmd`; cmd/bd/recompute_blocked.go:80 `recomputeBlockedCmd`; cmd/bd/purge.go:380 `purgeCmd`; cmd/bd/conflicts.go:779 `conflictsCmd` (subcommands at :776-778 conflictsListCmd/conflictsShowCmd/conflictsResolveCmd). GO's `db-proxy-child` is registered in cmd/bd/db_proxy_child.go.

**Why the refuter killed it**

> REFUTED — found real implementations under different names AND documented deliberate non-goals, plus factual errors in the claim.
> 
> WHAT I RAN: brace-counted `pub enum Commands` in LOCAL src/cli/mod.rs (68 variants — the claim says 65, minor hand-wave) and all 12 `pub enum *Commands` sub-enums; read admin.rs in full; grepped every keyword across src/ and docs/; parsed the DWS Commands enum (47 variants); enumerated all GO `rootCmd.AddCommand` sites; read the GO Long/Short help for each of the 12.
> 
> REAL IMPLEMENTATIONS FOUND (3 of 12 have LOCAL equivalents under different names):
> 1. `purge` -> `br wisp gc` EXISTS. src/cli/commands/wisp.rs:39 `Gc(WispGcArgs)`, args at :79-88 (`--max-age-hours` default 24, `--dry-run`), impl at :315-353 filters `i.ephemeral && i.updated_at < cutoff` then `store.delete_issue(...)`. Dispatched at src/main.rs:901. GO purge.go:45-88 is "Delete closed ephemeral beads to reclaim space" — same domain (wisps), same irreversible-delete outcome, plus dry-run. LOCAL is even broader (age-gated, not closed-gated).
> 2. `recompute-blocked` -> `br doctor --repair` EXISTS. GO recompute_blocked.go explicitly says "Works in every storage mode" (i.e. NOT Dolt-gated). LOCAL has a dedicated staleness finding (doctor.rs:290-292 `BLOCKED_CACHE_STALE_FINDING`, `BLOCKED_CACHE_CONTENT_MISMATCH_FINDING`) that calls `ensure_blocked_cache_fresh()` / `rebuild_blocked_cache(true)` at doctor.rs:2444-2448, reporting `blocked_cache_rebuilt` (:87, :357). `br sync` also force-rebuilds at sync.rs:2760 and :2359.
> 3. `migrate` (the `schema` subcommand, "Apply pending schema migrations (idempotent)") -> AUTOMATIC in LOCAL. schema.rs:582, :677, :706 call `run_migrations`; `CURRENT_SCHEMA_VERSION = 21` (schema.rs:12); `run_migrations_atomic` is invoked from open. No command needed.
> 
> DOCUMENTED DELIBERATE NON-GOALS (the Dolt/VC core of the family):
> - README.md:784, under "Limitations — br intentionally does not support": "| **Dolt/VC backend** | SQLite + JSONL only |"
> - docs/porting/PROPOSED_ARCHITECTURE_FOR_BR_USING_RUST_BEST_PRACTICES.md:3512, under "Appendix D: Explicit Non-Goals": "| Dolt backend | SQLite-only for simplicity |"
> - docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:208 "dolt/ # Dolt backend (DO NOT PORT)"; :4460 "`--backend` accepts `sqlite` (default) and `dolt` (legacy; not ported to `br`)"
> - EXISTING...md §15.91 (lines 7557-7594) "Non-Invasive Boundaries (Explicit Do-Not-Port List)": auto-commit/auto-merge, sync-branch creation/checkout, merge drivers — which is exactly what `vc` and `branch` do.
> This covers dolt, db-proxy-child, conflicts (conflicts.go:33-38 "resolve them without the raw dolt CLI"), compact, gc's Dolt phases, migrate-personal (imports internal/doltserver + internal/storage/dolt), and migrate's 4 server-mode-switch subcommands.
> 
> FACTUAL ERRORS IN THE CLAIM:
> - `compact` is described as "TTL compaction for ephemeral issues". FALSE. compact_dolt.go:22 Short is "Squash old Dolt commits to reduce history size" — Dolt commit squashing, not TTL. The claim conflated it with `bd admin compact` (admin.go:40). LOCAL's SQLite analogue is `br admin vacuum` + config/mod.rs:2064 `compact_database` via VACUUM INTO.
> - local_evidence asserts `heartbeat`=0. FALSE — src/model/mod.rs:562/585/601 (`WispType::Heartbeat`) and src/storage/sqlite.rs:11210. `heartbeat` and `unclaim` are not even among the 12 claimed commands, so this evidence is boilerplate.
> - The count of 12 is a cherry-picked undercount. GO also registers `prune` (prune.go:78), `flatten`, `restore`, `repo`, `reclaim`, `assign`, `duplicates`, `find-duplicates`, `diff`, `events`, `human`, `rules`, `swarm`, `todo`, `cook`, `setup`, `onboard`, `preflight`, `promote`, `provenance`, `kv`, `link`, `note`, `tag`, `types`, `state`, `heartbeat`, `unclaim`, `mail`, `github`, `gitlab`, `linear`, `jira`, `notion` — all absent from LOCAL.
> 
> RESIDUAL REAL GAP (why I did not set severity to info): there is no closed-NON-ephemeral decay. GO `bd prune` (prune.go:10-45, "Permanently delete closed non-ephemeral beads") and `bd gc` phase 1 DECAY ("Delete closed issues older than N days (default 90)") have no LOCAL analogue — `br history prune` (src/cli/commands/history.rs:635-687) only prunes old JSONL backup files, not issues. That is a genuine but minor, low-severity gap, and it is a real capability GO has that neither LOCAL nor DWS does.
> 
> DWS VERIFIED: 47-variant enum (Agents, Audit, Blocked, Capabilities, Capacity, Changelog, Close, Comments, Completions, Config, Coordination, Count, Create, Defer, Delete, Dep, Doctor, Epic, Gate, Graph, History, Info, Init, Label, Lint, List, Orphans, Q, Query, Ready, Reopen, RobotDocs, Scheduler, Schema, Search, Show, Stale, Stats, Status, Sync, Undefer, Update, VcsStatus, Serve, Upgrade, Version, Where). None of Dolt/Backup/Compact/Gc/Vc/Branch/Migrate/Recompute/Purge/DbProxy/Conflicts. Its `dolt` grep = 0 hits. So present_in = go-only is correct (note DWS also lacks Admin/Export/Import, so it is a different/older fork than LOCAL).
> 
> UPSTREAM EVIDENCE SANITY CHECK: all 12 cited GO registration sites are accurate and exist exactly as quoted (verified by grep of rootCmd.AddCommand, and by reading backup_dolt.go:476-478 and conflicts.go:776-778). The upstream half of the claim is well-evidenced; only the local half is wrong.

**Corroborator note**

> Refuted as stated. present_in=go-only and all 12 cited file:line registrations are accurate, but the "high severity, 12-command maintenance family" framing is not supportable. db-proxy-child is Hidden internal fork+exec plumbing with no user-visible behaviour and must be struck (12 -> 11). Of the rest, 7 are Dolt-native by their own documentation (dolt, backup, compact, vc, branch, conflicts, and gc's compact/GC phases) and are excluded by the brief's "Go-only enterprise/external-integration concern far from the Rust product direction" rule. `branch` is misdescribed: it lists/creates Dolt branches, not "branch-scoped issue views". Three impact claims are wrong: `br wisp gc` (wisp.rs:39/:313, --max-age-hours, --dry-run) already provides the ephemeral retention that `bd purge` targets; the blocked-cache recompute repair exists at `br doctor --repair` (doctor.rs:2448, reached at :11416) plus automatically on every mutation; and GO's own backup Long (backup.go:19-22) frames `bd export` — which LOCAL ships — as the JSONL migration/interoperability path. Both Rust forks document the omission as deliberate (LOCAL PROPOSED_ARCHITECTURE:3512 "Dolt backend | SQLite-only for simplicity"; DWS ARCHITECTURE.md:39 and BRIDGE_PLAN:957 "frozen; no Dolt"), which is the brief's explicit "set info" trigger. Corrected: a documented backend-driven fork divergence at info severity, retaining only a low-severity residual — no retention/decay policy for closed regular (non-ephemeral) issues, where GO has `bd prune`/`gc --older-than`, and no standalone `recompute-blocked` alias. Nothing here blocks core agent workflows, data integrity, or recovery, so high is not defensible.

### A4. `create` lacks GO's --graph bulk-plan primitive plus 13 other create flags (--spec-id, --id, --repo, --skills, --context, --no-inherit-labels, --validate, --metadata, --storage-class, event-* quartet)

- **Domain:** `cli-surface` · **Type:** `missing_flag` · **Claimed severity:** high · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: go-only · corrected severity: low

**Claimed upstream evidence**

> /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/create.go:919 `createCmd.Flags().String("graph", "", "Create a graph of issues with dependencies from JSON plan file")`; :927 `spec-id`; :933 `id` ("Explicit issue ID (e.g., 'bd-42' for partitioning)"); :940 `repo`; :929 `skills`; :930 `context`; :935 `no-inherit-labels`; :947 `validate`; :948 `allow-empty-description`; :964 `metadata`; :944 `storage-class`; :943 `no-history`; :937-938 `waits-for`/`waits-for-gate`; :950-953 `event-category`/`event-actor`/`event-target`/`event-payload`.

**Why the refuter killed it**

> The claim's upstream citation is accurate but its central rationale is factually false, and I proved it by reading the code and its tests.
> 
> UPSTREAM EVIDENCE CHECK (plausible, accurate): `sed -n '919p' /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/create.go` returns verbatim `createCmd.Flags().String("graph", "", "Create a graph of issues with dependencies from JSON plan file")`. Lines 927 (`spec-id`) and 933 (`id`, "Explicit issue ID (e.g., 'bd-42' for partitioning)") also match exactly. So GO really does have these flags. present_in=go-only is correct: DWS's `pub struct CreateArgs` (28 fields: title, title_flag, type_, slug, priority, description, description_file, assignee, owner, labels, parent, deps, estimate, due, defer, external_ref, ephemeral, status, acceptance_criteria, prerequisites, agent_context, dry_run, silent, file, agent_name, harness, model) has NO `graph`, `spec_id`, `id`, `repo`, `skills`, `context`, or `metadata`. DWS also has `DepCommands::Import` (a bulk `dep import`) that LOCAL lacks — a separate, unclaimed gap.
> 
> WHY THE GAP IS REFUTED: The claim asserts "LOCAL can only create issues one at a time (or via `--file` for one-issue-per-heading markdown) and wire dependencies afterward through separate `br dep add` invocations — a non-atomic multi-step sequence with a visible partially-built state." This is wrong. LOCAL's `br create --file` creates a full issue graph WITH inter-issue dependencies in a SINGLE invocation:
> 
> - `src/util/markdown_import.rs` module docs (lines 15-24) document an "Intra-file Dependency References" feature: "Dependencies can reference other issues in the same import file by: Title (exact H2 text) or Stand-in ID (assign `### ID`, then reference it from another issue's `### Dependencies` section)." `ParsedIssue` carries `stand_in_id`, `parent`, and `dependencies: Vec<String>`; `Section::from_header` recognizes `ID`, `Parent`, `Dependencies`/`Deps`.
> - `src/cli/commands/create.rs:885-887` builds `standin_to_ids` / `title_to_ids` / `deferred_deps` maps and comments "Phase 1: Create all issues, deferring intra-file dependency resolution." The resolution loop at create.rs:1230-1330 calls `storage.add_dependency(issue_id, &resolved_dep_id, &type_str, &actor)` at line 1314 (and `parent-child` at 1221) in the same process/transaction.
> - `tests/markdown_import.rs:798 test_markdown_import_standin_id_dependency_resolution` proves it end-to-end: one `br create --file issues.md --json` invocation creates 2 issues AND asserts `issues[1]["dependencies"][0]["depends_on_id"] == issues[0]["id"]`. Corroborating tests: `:857` title-based resolution, `:739` whitespace-separated typed deps, `:424` parent-as-global-default, `:601`/`:665` dep-parse guards.
> 
> So GO's `--graph` (JSON plan) and LOCAL's `create --file` (markdown plan) achieve the SAME user-visible outcome — one call creates an issue graph with inter-issue dependencies. The delta is input format (JSON vs markdown), a surface difference, not a missing capability. LOCAL's own `capabilities.rs:518` documents `--file` as "bulk import".
> 
> ON THE `spec_id` SUB-CLAIM: accurate that no CLI flag sets it (rg `spec_id` over `src/cli/commands/update.rs` returns 0 hits; the 4 hardcoded `None` sites cited are real), but overstated as evidence of a gap — the field is fully plumbed elsewhere: `model/mod.rs:1020`, a `spec_id TEXT` column (`storage/schema.rs:75,822,1010`), a working query predicate `spec_id=` (`query/evaluator.rs:387,617-618`, tested at `:998`), and inclusion in the content hash (`util/hash.rs:84`). It is a queryable, persisted, hash-stable field, reachable through JSONL import; only the create/update flag is missing.
> 
> WHAT SURVIVES: several genuinely absent create flags (`--repo`, `--skills`, `--context`, `--validate`, `--allow-empty-description`), and `spec_id`/`--id` have no flag setter (DWS lacks them too, so not a LOCAL-specific regression). LOCAL's `DepCommands` (Add/Remove/List/Tree/Cycles) does lack DWS's `dep import`. These are real but low-impact surface-parity items, not the claimed high-severity missing bulk-graph primitive.

**Corroborator note**

> Gap is real (flags are genuinely go-only; all 19 citations verified exact) but severity is overhyped from high to low, and two impact claims are factually wrong.
> 
> (1) WRONG: "LOCAL can only create issues one at a time (or via --file for one-issue-per-heading markdown) and wire dependencies afterward through separate br dep add invocations." LOCAL's br create --file ALREADY does single-call bulk graph creation WITH intra-file dependency wiring and symbolic resolution — src/util/markdown_import.rs:16-23 (references by H2 title or "### ID" stand-in, "resolved to real generated IDs during import"), wired in the same invocation at src/cli/commands/create.rs:1234-1240 via lookup_import_reference + storage.add_dependency, and regression-tested at tests/markdown_import.rs:798 and :857. The real difference is the serialization format (JSON plan vs markdown headings), not the capability. The "N create + M dep calls" cost model does not apply.
> 
> (2) WRONG: "The unpopulated spec_id model field also means schema/JSONL round-trips carry a permanently-null column." spec_id is #[serde(default)] at src/model/mod.rs:1019-1020, sync deserializes Issue directly at src/sync/mod.rs:819, and it is read on DB load at sqlite.rs:10261 / schema.rs:2169 and folded into content hashing at src/util/hash.rs:84,205. A spec_id set by GO round-trips through JSONL correctly. The only accurate part is that no br CLI flag sets it (construction sites create.rs:475, :1048, q.rs:177, wisp.rs:256 do hardcode None) — a minor ergonomic gap, not data corruption.
> 
> (3) MOST REMAINING FLAGS ARE OUT OF SCOPE FOR br. --repo (multi-repo auto-routing), --skills, --context, --storage-class (versioned/unversioned/ephemeral), --no-history (Dolt commit history), --waits-for/--waits-for-gate (swarm fanout gates), and the event-* quartet are tied to GO's Dolt-backed Gastown deployment/swarm model, which br deliberately does not replicate (br is non-invasive SQLite+JSONL, no Dolt, no swarm daemon). Per the calibration rule, Go-only enterprise/orchestration concerns far from the Rust product direction should downgrade.
> 
> (4) MINOR COUNT DRIFT: LOCAL CreateArgs is 22 fields (src/cli/mod.rs:1203-1298), not 20; GO create.go registers 36 create flags, not 33.
> 
> (5) THE ONE LEGITIMATE RESIDUAL: LOCAL's bulk import has no transaction wrapper — src/cli/commands/create.rs execute_import (:802+) has no begin/commit; per-issue failures `continue` instead of rolling back, so a mid-import error can leave a partially-built graph. This is a fair but narrow atomicity concern about the EXISTING markdown path, not a case where --graph uniquely rescues the user. Low severity: agents can use the beads-workflow skill (markdown-plan-to-beads, which already documents ### Dependencies at SKILL.md:113) as the reasonable alternative.

### A5. 20+ long-tail GO commands absent (edit, children, tag, link, note, kv, types, statuses, state/set-state, flatten, create-form, context, rules, provenance, heartbeat, human, ship, cook, repo, preflight, setup, init-safety, cursor-hook, swarm, metrics, diff, restore, prune, events)

- **Domain:** `cli-surface` · **Type:** `missing_feature` · **Claimed severity:** low · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> GO registration sites (cmd/bd/): edit.go:193, children.go:56, tag.go:123, link.go:112, note.go:204, kv.go:340, types.go:166, statuses.go:128, state.go:352, flatten.go:185, create_form.go:406, context_cmd.go:213, rules.go:667, provenance.go:208, heartbeat.go:106, ping.go:102, human.go:502, ship.go:156, cook.go:1149, repo.go:485, preflight.go:75, init_safety_help.go:104, setup.go:434, cursor_hook.go:67, swarm.go:1255, diff.go:155, restore.go:181, prune.go:78, events.go:209. Telemetry flush is cmd/bd/send_metrics.go:10 (`sendMetricsCmd`, `Hidden: true`, `Short: "Internal: flush queued telemetry events (spawned by bd)"`).

**Why the refuter killed it**

> I ran an exhaustive search of /Users/tranquangdang21/Projects/beads_rust (LOCAL) and both upstreams on disk. The claim fails on its own evidence and on substance.
> 
> EVIDENCE ERROR 1 — variant count. The claim's local_evidence says "python3 brace-count of `pub enum Commands` in src/cli/mod.rs (65 variants)". I reproduced the brace-count and got 68: ['Agents','Audit','Blocked','CodexHook','Capabilities','Changelog','Close','Formula','Comments','Completions','Config','Coordination','Count','Create','Defer','Delete','Dep','Doctor','Admin','Epic','Mol','Gate','Export','Graph','History','Hooks','Import','Info','Init','Label','Lint','List','Orphans','Q','Quickstart','Query','Ready','Recipes','Rename','RenamePrefix','Reopen','RobotDocs','Scheduler','Schema','Search','Show','Stale','Stats','Status','Sync','Undefer','Memory','Wisp','CustomStatus','CustomType','Prime','Reflect','Template','Update','Serve','Web','Upgrade','Version','Worktree','Federation','MergeSlot','Where','Sql']. The claim's list of 65 omits Formula, Admin, Epic, Mol, Gate, CustomStatus, CustomType, Memory, Wisp, Init, Sql, Hooks — precisely the variants that would have refuted `types`, `statuses`, and `swarm`. A hand-waved count that misses the refuting entries is not a verified enumeration.
> 
> REAL IMPLEMENTATIONS FOUND (each read in source, not inferred):
> - types/statuses: `src/cli/commands/custom_status.rs` defines `StatusCommands{List,Add,Remove}` (custom_status.rs:29) and `TypeCommands{List,Add,Remove}` (custom_status.rs:47), backed by store.list_custom_statuses/add_custom_type. Registered at `src/cli/mod.rs:1037-1046` and documented at `docs/CLI_REFERENCE.md:972-1008` ("br custom-status list/add/remove", "br custom-type list/add/remove"). Directly refutes "types/statuses absent".
> - swarm: `src/cli/commands/mol.rs:92-99` `MolSwarmCommands{Validate,Create}` with MolSwarmValidateArgs/MolSwarmCreateArgs (mol.rs:56-70), registered at cli/mod.rs:837-840. Directly refutes "swarm absent".
> - repo: `src/cli/commands/federation.rs:25` FederationCommand{Add,Remove,Sync,Info}.
> - setup: `src/cli/commands/recipes.rs` `br recipes list/install` over `src/recipes/mod.rs:71-266` — 20+ AI-tool recipes (Cursor IDE, Windsurf, Cody, Claude Code, Gemini CLI, Copilot CLI+VS Code, Codex CLI, Aider, Goose, Crush, Junie, Factory, OpenCode, Mux). GO's `setup` is "Setup integration with AI editors" (setup.go:30-32) — a direct functional match.
> - cook: `src/cli/commands/formula.rs:27` FormulaCommands{Validate,Expand,Apply,List,Show,Convert}.
> - kv: `ConfigCommands{List,Get,Set,Delete,Edit}` at cli/mod.rs:3461-3500 operates the config table; `src/cli/commands/memory.rs:13` `const MEMORY_KEY_PREFIX: &str = "kv.memory."` and config.rs:824-829 reserves that same `kv.` namespace — i.e. LOCAL has GO's kv store, addressed through `br config`/`br memory` rather than a `kv` verb.
> - tag/link/note: `br label add/remove` (label.rs:38-39), `DepCommands::Add` (cli/mod.rs:2448), `CommentCommands::Add` (comments.rs:37).
> - children: `br list --parent <id>` with `-r/--recursive` (cli/mod.rs:3084-3088) — GO's `children <parent-id>` is the same query.
> - edit: UpdateArgs carries title/description/design/acceptance_criteria/notes (cli/mod.rs:1446-1463) — the exact field set GO's `edit` opens in $EDITOR (edit.go:89-97); and `br config edit` does spawn $EDITOR via an allowlisted-editor enum (config.rs:516-556, AllowedEditor at :574).
> - state/set-state: GO's state.go:54-128 documents the model as `<dimension>:<value>` labels. LOCAL's LabelValidator accepts namespaced labels — `assert!(LabelValidator::validate("team:backend").is_ok())` at src/validation/mod.rs:751 — and MERGE_SLOT_LABEL = "gt:slot" (src/merge_slot/mod.rs:22) is a real colon label. So the state-dimension store exists; `br label add/remove` is the setter.
> - context: `br where` emits path/redirected_from/prefix/database_path/jsonl_path (where.rs:19-30); plus `br info`, `br capabilities`.
> - provenance: `src/storage/events.rs` is an append-only events table; `AuditCommands{Record,Coordination,Label,Log,Summary}` (cli/mod.rs:2788-2799).
> - heartbeat: `WispType::Heartbeat` (model/mod.rs:562, :585) plus `br wisp create/list/gc` (wisp.rs:30-39) and `br coordination` lease/expiry (coordination.rs:758).
> - preflight: `br doctor` is the pre-PR checklist/repair surface (768KB, DoctorSubcommand{Capabilities,RobotDocs,Health,Ls,Undo,Explain} at cli/mod.rs:3709-3725) plus `br lint`.
> - cursor-hook: `br codex-hook` implements SessionStart/PreCompact/PostCompact/UserPromptSubmit (codex_hook.rs:1-8, :147, :164) and `br hooks run` (HooksCommand at cli/mod.rs:1653-1662); Cursor is also a first-class recipe target.
> - ship: `br federation sync` + `br merge-slot` (merge_slot.rs).
> 
> EVIDENCE ERROR 2 — the claim's own sub-group (c) equivalence is false. It says diff/restore/prune/events are "naming/nesting differences rather than true absences." Reading both sides: GO `prune` deletes *closed beads* with --older-than/--pattern/--force (prune.go:11-77) whereas LOCAL `history prune` prunes *backup files* with --keep/--older-than (cli/mod.rs:3575-3580, dispatched at history.rs:351-352). GO `restore` recovers a *compacted issue's pre-compaction content* from a compaction snapshot (restore.go:18-28) whereas LOCAL `history restore` restores a *backup file* (cli/mod.rs:3566-3572). GO `diff <from-ref> <to-ref>` diffs *two Dolt git refs* (diff.go:13-28, calls store.Diff) whereas LOCAL `history diff` diffs *a backup JSONL against the current JSONL* (history.rs:471, uses similar::TextDiff). Different capabilities, not renames.
> 
> EVIDENCE ERROR 3 — not-portable. GO `flatten` is "Squash all Dolt history into a single commit" (flatten.go:18-20); LOCAL is SQLite+JSONL with no Dolt history to squash. GO `sendMetricsCmd` is `Short: "Internal: flush queued telemetry events (spawned by bd)"` with `Hidden: true` (send_metrics.go:12-13) — an internal child-process helper, not user-facing surface.
> 
> EVIDENCE ERROR 4 — a documented deliberate-scope decision exists and was ignored. `docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:3845-3858` §15.19 "Classic-Only Command Subset" states the Rust port "intentionally omits Gastown/daemon/hook automation," and §15.82.4 (:6943-6960) records per-command dispositions — `swarm validate`/`swarm create`/`ship`/`cook`/`mol bond`/`restore` all marked "Excluded." LOCAL subsequently *implemented* most of them anyway (br mol swarm, br formula, br federation, br gate). These were deliberate, documented omissions that were then closed — not undiscovered gaps.
> 
> UPSTREAM SANITY CHECK — the GO citations are plausible (every file exists under cmd/bd/ with matching Use/Short: edit.go:16-18, children.go:10-12, kv.go:141-143, create_form.go:215-217, context_cmd.go:36-38, rules.go:632-647, provenance.go:16-142, heartbeat.go:13-16, human.go:20-327, ship.go:14-15, cook.go:35-36, repo.go:19-187, preflight.go:39-41, setup.go:30-32, cursor_hook.go:57-59, swarm.go:20-932, diff.go:13, restore.go:18, prune.go:11, events.go:33-158). The GO side is real. The LOCAL side is where the claim fails.
> 
> I also confirmed LOCAL's alias citations are accurate (comment :784, completion :788, coord :798, onboard :902) and memory.rs MemoryCommands Remember/Memories/Recall/Forget (memory.rs:46-90) is real — so the claim's exclusions are fine. The failure is entirely in the "absent" list.

**Corroborator note**

> Refuted as characterized; downgrade low -> info. The 29 upstream citations are all real, and the commands are genuinely GO-only (DWS has none). But: (a) `flatten` is fabricated — it is Dolt history squashing ("Nuclear option: squash ALL Dolt commit history into a single commit"), not hierarchy flattening; (b) the four commands the finding calls load-bearing are all already covered — `link` is verbatim "Shorthand for 'bd dep add'" (covered by `br dep add --type`), `kv` is a `store.SetConfig` config store (covered by `br config get/set`), and `types`/`statuses` have full CRUD locally (`br custom-type`/`br custom-status` list/add/remove) versus GO's list-only; (c) the claim that diff/restore/prune/events are mere "naming/nesting differences" is wrong for 3 of 4 — GO `restore` recovers a compacted issue from a Dolt snapshot, GO `prune` permanently deletes closed beads (destructive), GO `diff` takes Dolt commit/branch refs, all unlike LOCAL's JSONL-backup-based `history` subcommands; (d) the AuditCommands citation is wrong (src/cli/mod.rs:2788-2799, not :2040-2048); (e) most of the list is a deliberate, documented fork divergence — docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:3845-3857 states br targets the classic SQLite+JSONL tracker and intentionally omits Gastown/daemon/hook automation, which covers cursor-hook, swarm, cook, ship, repo, context, heartbeat, provenance, and the Dolt-only flatten/restore/diff. Residual genuine (but minor) absences worth a one-line parity note: cursor-hook (LOCAL ships only the hidden codex-hook), destructive prune, lease/heartbeat, and state/set-state.

### A6. DWS `vcs-status` (67KB bounded VCS diagnostics) has no LOCAL equivalent

- **Domain:** `cli-surface` · **Type:** `missing_feature` · **Claimed severity:** medium · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: dws-only · corrected severity: low

**Claimed upstream evidence**

> DWS /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/mod.rs:989 `VcsStatus(VcsStatusArgs)`; args struct at :1477 with `jsonl` (:1480), `allow_external_jsonl` (:1484), `timeout_ms` (:1492, default 2000), `robot` (:1496). Implementation /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/commands/vcs.rs (67KB), module doc lines 1-8: "Explicit, bounded VCS diagnostics. This module is intentionally isolated from every sync path. `br sync` is VCS-agnostic by contract; only a direct `br vcs-status` invocation reaches the process capability below."

**Why the refuter killed it**

> REFUTED. The finder's local_evidence is materially wrong on its decisive conclusion. It ran `rg -in '\bvcs\b' src` (a name-based search that can only miss a feature that never uses the token "vcs"), concluded "LOCAL's design is 'never touches VCS'", and stopped there. LOCAL does exactly the opposite: it runs read-only git probes and reports JSONL working-tree status — it just calls the feature `git_export` on `br sync --status` instead of `vcs-status`.
> 
> What I actually ran and found in LOCAL (/Users/tranquangdang21/Projects/beads_rust):
> 
> 1. `rg -c` over `src/` for DWS's own field names returned non-zero for `index_clean` (6) and `worktree_clean` (6), plus `hash-object` (3), `ls-files` (3), `check-ignore` (1). `worktree_state`/`WorktreeState`/`unmerged`/`object_format` were 0 — so I followed the non-zero leads rather than stopping at the empty `vcs` grep.
> 
> 2. The implementation is `src/cli/commands/sync.rs`:
>    - :89-91 — the sync-status output struct carries `pub git_export: GitExportStatus` with the doc "Read-only git visibility for the canonical JSONL export (beads_rust#338)".
>    - :94-140 — `pub struct GitExportStatus { available, tracked, worktree_clean, index_clean, head_hash, worktree_hash }`. Four of these field names are byte-identical to DWS's `VcsExportStatus` fields `available`, `tracked`, `worktree_clean`, `index_clean`.
>    - :876-895 — `fn porcelain_cleanliness(porcelain: &str) -> (bool, bool)` returning exactly `(index_clean, worktree_clean)`, decoding the `??` untracked case and the X/Y porcelain columns — i.e. LOCAL implements the same porcelain→cleanliness mapping DWS's `WorktreeState` enum provides.
>    - :863-874 — `run_git_capture()`: read-only, degrades to `None` on any failure.
>    - :898-937 — `fn git_export_status(jsonl_path) -> GitExportStatus` calling `git rev-parse --is-inside-work-tree`, `git status --porcelain -- <file>`, `git ls-files --error-unmatch`, `git rev-parse --verify --quiet HEAD:./<file>`, and `git hash-object`. DWS's module doc at src/cli/commands/vcs.rs:1-8 describes the same probe set.
>    - :990 — wired into output: `git_export: git_export_status(jsonl_path)`.
>    - :100-104 — the same "never fails the command, `{available:false}` on any git error" degradation contract as DWS's `VcsExportStatus::unavailable()`.
> 
> 3. Reachability is a documented flag, not a hidden path: `rg 'status'` on src/cli/mod.rs found `--status  Show sync status (read-only)` at :983, `br sync --status  Show current sync status` in the help block at :1017, and `SyncArgs` at :3332/:3355. The help text is printed by the binary itself.
> 
> 4. It is tested, not aspirational: `tests/e2e_sync_status_health.rs:71` `e2e_sync_status_git_export_committed_vs_dirty_jsonl` runs real `git init/add/commit` and asserts `available`, `tracked`, `worktree_clean`, `index_clean`, `head_hash` (cross-checked against `git hash-object` for 40-hex agreement) and `worktree_hash` across untracked → committed → dirty states; :179 `e2e_sync_status_git_export_unavailable_outside_repo` asserts the `{available:false}` degradation. Inline unit tests at sync.rs:3361-3390 cover `porcelain_cleanliness` and the non-repo path.
> 
> 5. It is not feature-gated: `rg '#\[cfg\(feature' src/cli/commands/sync.rs` returned nothing, and Cargo.toml's only gates are `self_update` (default) and `mcp` — neither wraps the status path.
> 
> So the claimed gap's own description — "reports the working-tree status of the `.beads/` state files (the JSONL export) against the current VCS" — is delivered by LOCAL today via a different command name. The rule I was given is explicit: any real implementation or documented deliberate equivalent sets refuted=true.
> 
> Residual real-but-smaller differences (so this is low-severity partial parity, not a dead claim): LOCAL has no standalone `br vcs-status`; it lacks DWS's `--jsonl <PATH>` override (LOCAL's `git_export_status` is hardwired to `path_policy.jsonl_path`, though sync does accept `--allow-external-jsonl` at mod.rs:3425 for the write path), the `--timeout-ms` shared budget (LOCAL uses blocking `Command::output()` with no deadline), object-format detection, unmerged index stages, the richer `WorktreeState` enum, in-process raw SHA-256, and the hook/filter/prompt/fsmonitor/untracked-cache/GIT_*-redirection neutralization. Notably LOCAL's `run_git_capture` (sync.rs:863) does not even apply the `-c core.hooksPath=` hardening that its own `worktree/mod.rs:514` does.
> 
> Upstream cross-check (DWS citations are all accurate — I verified them rather than assuming): `rg 'VcsStatus' /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/mod.rs` returned :989 `VcsStatus(VcsStatusArgs)`, :1477 `pub struct VcsStatusArgs`, :1604 `VcsStatus`, :1744 dispatch; and :1477-1496 confirms `jsonl` :1480, `allow_external_jsonl` :1484, `timeout_ms` :1492 (`default_value_t = 2_000`), `robot` :1496. `wc -c` on its vcs.rs = 68151 bytes, matching the "67KB" claim. GO: `rg -i 'vcs.status' /tmp/beads_gap_audit/gastownhall_beads` returned nothing; its `cmd/bd/vc.go:251` `vcStatusCmd` ("Show current branch and uncommitted changes") actually only prints `branch` and `commit` pulled from `store.CurrentBranch`/`store.GetCurrentCommit` and never inspects the `.beads/` JSONL, so GO does not have this capability either. present_in is therefore dws-only among the upstreams, while LOCAL holds a real partial equivalent.

**Corroborator note**

> The command NAME `br vcs-status` and its hardening are DWS-only, but the CAPABILITY the claim says LOCAL lacks is already shipped in LOCAL as `br sync --status` -> `git_export` block (src/cli/commands/sync.rs:91, :105-141, :901-937, :1005-1006; bead beads_rust#338), which reports tracked/index_clean/worktree_clean/head_hash/worktree_hash for the JSONL export and is JSON/robot-readable. Reclassify from "missing_feature (medium)" to "hardening delta (low)": the genuine DWS-only residue is the Git-env neutralization (vcs.rs:1322-1366, :1844), bounded 32KB capture (:32), 25-30000ms shared deadline (:33-34), external-JSONL path allowlist, and no-follow in-process raw hashing. LOCAL's `run_git_capture` (sync.rs:864) inherits the process env and has no timeout, so that hardening is a defensible future improvement — but it is defense-in-depth, not a missing product capability, and the "no first-class way to ask why my JSONL is dirty" impact statement should be struck as factually incorrect.

### A7. No byte-pinned golden JSON wire corpus or protocol contract suite

- **Domain:** `cli-ux-agent-contract` · **Type:** `missing_test` · **Claimed severity:** high · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: both-upstreams · corrected severity: low

**Claimed upstream evidence**

> cmd/bd/protocol/CATALOG.md (full producer-corpus contract: byte-compare, `make corpus-regen`, byte-identity backstop); cmd/bd/protocol/corpus.go (17KB deterministic plan + canonicalizer + manifest); 35 committed goldens under cmd/bd/protocol/testdata/corpus/{flat,envelope}/ plus manifest.json; cmd/bd/protocol/canonicalize_test.go (`TestCorpusDoubleRunByteIdentical`); cmd/bd/protocol/versioning_contract_test.go (7.1KB), exit_codes_test.go + exit_codes_init_test.go (9.9KB), errors_contract_test.go (9.9KB), interchange_test.go (17KB), roundtrip_test.go, json_contract_test.go (14KB).

**Why the refuter killed it**

> REFUTED. LOCAL has three independent, committed, byte-pinned golden JSON wire corpora plus a regen workflow. The claim's local_evidence is materially false.
> 
> WHAT I RAN AND FOUND
> 
> 1) `find tests -type d \( -name '*golden*' -o -name '*corpus*' -o -name '*protocol*' -o -name '*contract*' -o -name '*fixture*' -o -name '*snapshot*' \)` returned `./tests/snapshots`, `./tests/fixtures`, `./tests/doctor_fixtures` in addition to the perf dirs the claim cited. The claim's `find` patterns were `*golden*` / `*corpus*` only — neither matches `snapshots`, `fixtures/json_baseline`, or `agent_baseline`. That is why it reported "only perf baselines."
> 
> 2) tests/snapshots/ — 96 committed goldens, ALL git-tracked (`git ls-files 'tests/snapshots/**' | wc -l` -> 96). Wired as a real test target: `tests/snapshots.rs` (2 lines, `#[path = "snapshots/mod.rs"] mod snapshots;`) -> `snapshots/mod.rs:679-686` declares 8 submodules (cli_output, error_messages, history_diff_output, json_output, jsonl_format, robot_output, schema_output, toon_output) carrying 67 `#[test]` fns. Uses `insta` (Cargo.toml:138, features json+yaml) plus a hand-rolled canonicalizer (`normalize_json`/`normalize_output`, regex normalizers for ANSI, IDs, timestamps, paths, build profile at mod.rs:45-95).
> 
> 3) The single most damaging artifact — a RAW unmasked byte golden. tests/snapshots/json_output.rs:8-15 states the intent verbatim: "The fixture uses fixed IDs, actors, and timestamps, so these snapshots should not require masking; they intentionally lock down JSON field order and optional/null omission behavior from the CLI serializer." It commits a 4-line JSONL fixture (bd-golden-parent/child/closed/deleted, 2026-01-01T00:00:00Z) and asserts `assert_snapshot!("representative_list_json_output", output.stdout.trim_end())`. The golden `snapshots__snapshots__json_output__representative_list_json_output.snap` is the literal one-line wire payload with real ids/timestamps — byte-compared via insta. This is precisely "a byte-pinned golden JSON wire corpus."
> 
> 4) tests/fixtures/json_baseline/ — 16 committed JSON goldens (all git-tracked: version, list, list_all, show_single, show_multiple, ready, blocked, search, stats, count, dep_list, doctor, label_list, label_list_all, list_priority_0_1, comments_list). Generator `scripts/generate_json_baseline.sh` ("Capture JSON output baselines for backward compatibility testing ... ensure that JSON output remains byte-identical"). Loader tests/common/json_baseline.rs:51-62 exposes `load_baseline_raw` documented as "Useful for byte-level comparison of JSON output."
> 
> 5) agent_baseline/ — 18 committed files at REPO ROOT (so even a widened `find tests/` would miss it): schemas/{schema_all,schema_error,cli_schema,schema_issue_details}.json, examples/{show_one,ready,list_limit3}.json + matching .toon + robot_mode_examples.jsonl, errors/show_not_found.json, help/{br_help,br_list_help,br_schema_help}.txt. All git-tracked. Consumed by tests/e2e_schema.rs:867 `agent_baseline_snapshots_match_current_binary`, which runs `compare_text_baseline` / `compare_json_baseline` (defined :1074, assert_eq on normalized values) / `compare_toon_baseline` against the live binary, with an `UPDATE_AGENT_BASELINE=1` regen env (:13, :1129) — the direct structural analogue of GO's `make corpus-regen`.
> 
> WHY THE CORE CLAIM IS FALSE
> The claim's decisive sentence — "a br wire change with no matching bd change regresses nothing" — is directly contradicted. Any br wire change to list/show/schema/version/error output hard-fails `representative_list_json_output`, `schema_document_golden_json_all`, and `agent_baseline_snapshots_match_current_binary`, none of which consult a bd binary. The self-skip-on-missing-bd critique is simply aimed at the wrong files.
> 
> UPSTREAM_EVIDENCE SANITY CHECK (plausible, and understated vs LOCAL coverage)
> GO's CATALOG.md is genuine and accurately quoted. I confirmed cmd/bd/protocol/CATALOG.md, corpus.go (17KB), canonicalize_test.go (12KB), versioning_contract_test.go (7.1KB), errors_contract_test.go (9.3KB), exit_codes_test.go + exit_codes_init_test.go (2.8+9.9KB), interchange_test.go (17KB), roundtrip_test.go (8.6KB), json_contract_test.go (14KB), and testdata/corpus/{flat,envelope}/ with 17 files each = 34 goldens + manifest.json = 35. The claim's "35" counts the manifest; the double-run test name is real.
> 
> HONEST RESIDUAL (refinement, not a missing capability)
> GO retains three things LOCAL does not: the flat-vs-`BD_JSON_ENVELOPE=1` dual byte-shapes (br has no envelope mode, so this is inapplicable rather than absent), a dedicated `TestCorpusDoubleRunByteIdentical` determinism backstop, and the downstream-consumer-vendors-the-corpus interchange model (GO positions the corpus as a consumer-pinned decoder artifact). These are refinements of a capability LOCAL demonstrably has, and none touches the claimed "no byte-pinned golden corpus" assertion.
> 
> CONCLUSION
> A real, wired, git-tracked, byte-pinned golden JSON wire corpus and protocol contract suite exists in LOCAL. The gap does not exist.

**Corroborator note**

> The gap is mischaracterised: the byte-pinned golden JSON wire corpus is NOT go-only — it exists in both upstreams, and LOCAL already pins br's own wire surface byte-for-byte, independently of bd.
> 
> Concretely, /Users/tranquangdang21/Projects/beads_rust/tests/snapshots/ holds 87 committed .snap files, and tests/snapshots/snapshots/snapshots__snapshots__json_output__representative_list_json_output.snap is an unmasked byte-exact golden of `br list --json` stdout (real field names, real order, real types). tests/snapshots/json_output.rs:13-15 says outright that it "intentionally lock[s] down JSON field order and optional/null omission behavior from the CLI serializer" — precisely the src/output/context.rs / src/format/output.rs / command-JSON-payload surface the claim asserts is unpinned. tests/snapshots/mod.rs:547 normalize_json masks only ids, timestamps, hashes, and actor names (the exact analogue of Go's `<TS>` canonicalizer) and never masks field names or structure. tests/snapshots/schema_output.rs additionally freezes 12 full JSON Schema documents in both JSON and TOON. DWS carries the same suite (89 .snap files, same representative_json_golden tests at json_output.rs:219/243), so present_in is both-upstreams. LOCAL also has a documented regen-and-review workflow (INSTA_UPDATE=always cargo test --test snapshots representative_json_golden), the functional equivalent of Go's `make corpus-regen`.
> 
> The claim is right that conformance self-skips without bd (tests/conformance.rs:34,60), but that is a secondary observation, since the snapshot goldens provide bd-independent coverage.
> 
> The one genuine, adjacent finding — and the correct replacement gap — is narrower and cheaper: .github/workflows/ci.yml:35 runs only `cargo test --lib --all-features`, and tests/snapshots.rs is a separate integration target, so the snapshot goldens, schema goldens, and conformance suite never actually execute in this repo's GitHub CI. That is a CI test-scope selection oversight affecting all of tests/, not a missing corpus, and it is a one-line fix. Minor citation nit: the Go corpus is 34 goldens + 1 manifest.json = 35 files, not "35 golden files plus manifest.json".

### A8. No published JSON contract stability policy or migration guide

- **Domain:** `cli-ux-agent-contract` · **Type:** `missing_docs` · **Claimed severity:** low · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)

**Claimed upstream evidence**

> docs/reference/json-schema.md:3 frontmatter `description: The stable JSON output contract for bd --json commands, covering the schema_version envelope, per-command fields, and consumer guidelines`; :28 and :38 flat and envelope example shapes; :56 `bd create "Example" --json | jq '.schema_version'`; :70-77 "Current version: **1**" plus the bump policy; :113-156 per-command and error-envelope shapes; :195-196 "this shape is pinned by a contract test — a change here is a breaking wire change"; :251-261 the consumer migration checklist including "Check `schema_version` on object output".

**Why the refuter killed it**

> I searched LOCAL hard and could not sustain the claim as written, because its local_evidence is materially incomplete and its central assertion is factually false.
> 
> WHAT I RAN (LOCAL = /Users/tranquangdang21/Projects/beads_rust): repo-wide `rg -ril` for schema_version|json.schema|contract|breaking change|stability|stable; targeted greps for 'stable contract|wire change|breaking wire|guarantee(to be) stable|is a stable'; greps for `contract_version` and `schema_version` across src/ docs/ tests/ scripts/ agent_baseline/; a 174-line sweep of README/AGENTS.md/CHANGELOG.md/UPGRADE_LOG.md/docs/scripts/agent_baseline for stab(le|ility)|breaking change|semver|bump polic|migration|additive|backward compat|wire change; full heading dump of docs/AGENT_INTEGRATION.md; read of src/cli/commands/capabilities.rs (1096L) and robot_docs.rs (158L); read of docs/agent/{SCHEMA,ROBOT_MODE,ERRORS}.md, docs/reliability/HEALTH_CONTRACT.md:140-175, AGENTS.md:425-465, docs/JSONL_COMPATIBILITY.md, tests/e2e_schema.rs:860-900. I also checked src/mcp/{mod,tools,resources,prompts}.rs, src/coordination.rs, src/policy.rs, src/write_combining.rs, src/sync/witness.rs, docs/porting/, and the agent_baseline/schema pin mechanism. Nothing was written, edited, or built.
> 
> THE FINDER MISSED SEVERAL REAL MECHANISMS:
> 
> 1. `src/cli/commands/doctor_subsystems/capabilities_doctor.rs:14-15` states an explicit additive-stability policy verbatim: "Stability: the JSON shape is stable contract. New fields are purely additive; agents must tolerate unknown keys." — on a versioned surface `br.doctor.capabilities.v1`.
> 
> 2. `src/cli/commands/doctor_subsystems/exit_codes.rs:3,46`: "These exit codes are stable contract surface — CI, agent scripts, and …" / "Numeric values are stable contract; do **not** change them."
> 
> 3. `docs/reliability/HEALTH_CONTRACT.md:152-156` is a literal `## Contract Versioning` section containing BOTH halves the claim says are absent: a bump rule ("Adding a new variant is backwards-compatible") and a migration requirement ("Changing severity of an existing variant requires a migration note in the changelog").
> 
> 4. `src/cli/commands/capabilities.rs:11` — `br capabilities --format json` is a versioned (`contract_version: "br.capabilities.v1"`) machine-readable publisher of the per-command contract: operation, workspace, machine_output formats, examples, global flags, output formats, exit codes 0-8 with category+description, env vars, safety guarantees — and via `command_detail_for_path` (`:335`) the full per-command ARGUMENT SHAPE (id/kind/long/short/aliases/help/required/action/value_names/default_values/possible_values), subcommands and safety notes. This is a working equivalent of GO's "enumerates per-command field shapes" for the `--command <path>` form. `src/cli/commands/robot_docs.rs:11` adds `br.robot_docs.v1`.
> 
> 5. `docs/agent/ERRORS.md` publishes the flat error-envelope shape plus a runnable regression check (`.error.code == "ISSUE_NOT_FOUND"`, exit 3).
> 
> 6. `docs/JSONL_COMPATIBILITY.md` is a real published wire-format compatibility reference (field map + per-field ✅/⚠️/❌ verdicts) for the interchange surface.
> 
> 7. Enforcement exists: `tests/e2e_schema.rs:868 agent_baseline_snapshots_match_current_binary` pins `agent_baseline/schemas/*.json` and fails with "is stale; rerun with UPDATE_AGENT_BASELINE=1"; `docs/agent/SCHEMA.md:33-43` documents that regeneration path for "intentional schema changes"; `scripts/generate_json_baseline.sh` is headed "Capture JSON output baselines for backward compatibility testing".
> 
> The claim's local_evidence did `ls *.md` (ROOT ONLY — ignoring docs/, docs/agent/, docs/reliability/) plus two narrow greps, and never looked at the `capabilities`, `robot-docs`, or `doctor capabilities` command surfaces. Its statement "nothing in the repo implements the versioning half" is therefore false.
> 
> WHAT SURVIVES (the honest residual): there is no single general document for `br --json` that emits a runtime INTEGER `schema_version` on general command output. LOCAL's `schema_version` fields exist only on artifact/coordination/doctor envelopes (`br.coordination.v1`, `br.doctor.db_snapshot.v1`, `br.reflect.v1`); `br list`/`ready`/`show` output carries no version field, so GO's checklist item #1 ("Check `schema_version`") has no LOCAL equivalent. Nor is there a consumer migration checklist. Note the only prose statement about the schema surface is actively negative: `agent_baseline/help/br_schema_help.txt:3` — "IMPORTANT: br schema is not a stable API and is subject to change. Use at your own risk." AGENTS.md:448's "Stable schema (changes are versioned and documented)" does remain only partly backed.
> 
> UPSTREAM SANITY CHECK: GO's `docs/reference/json-schema.md` is exactly as cited — :3 "The stable JSON output contract for bd --json commands", :15 "## Migration Guide", :72 "The `schema_version` field is an integer that increments when:", :195-196 "pinned by a contract test — a change here is a breaking wire change", :251 "**Check `schema_version`**". DWS has NO equivalent: `rg -ril 'json output contract|schema_version.*envelope|Migration Guide' --glob '*.md'` returned nothing, and no *json*.md contract doc exists. DWS does carry a `docs/AGENT_INTEGRATION.md:177` "Stable contract" column table and the same `capabilities_doctor.rs:14` stability line (it descends from LOCAL), but neither is a versioned contract reference. So present_in = go-only is correct.
> 
> VERDICT: the finding should be DISCARDED as stated and, at most, re-filed as info against a much narrower claim ("br --json object output carries no runtime schema_version and has no single consumer migration checklist"), given the versioned contract publishers, additive-stability policies, contract-versioning section, error-shape doc, compatibility reference, and drift-pin tests that already exist.

### A9. No distinct exit codes for destructive-operation refusals (GO uses 10-14; LOCAL folds them into category 2)

- **Domain:** `cli-ux-agent-contract` · **Type:** `missing_output_mode` · **Claimed severity:** medium · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> cmd/bd/init_safety.go:25-45 `ExitRemoteDivergenceRefused = 10`, `ExitLocalExistsRefused = 11`, `ExitDestroyTokenMissing = 12`, with the comment "Stable values so CI scripts can branch on them without grep'ing stderr"; cmd/bd/errors.go:183 `ExitMigrationFrozen = 14` ("Stable value so scripts can branch on 'someone is migrating this workspace, come back later' without grep'ing stderr, instead of reading it as a generic failure (1) worth retrying immediately"); cmd/bd/update.go:824 `ExitGuardMismatch = 13`; cmd/bd/sync.go:42-44 `ExitSyncConflict = 2`, `ExitSyncRetriesExhausted = 3`, `ExitSyncDirtyStuck = 4`; cmd/bd/errors.go:196-215 `CheckReadonly` is the single enforcement chokepoint that dispatches between them.

**Why the refuter killed it**

> REFUTED on the mechanism, while the claim's literal numbers check out. What I ran and found: (1) rg -n '= 10|= 11|= 12|= 13|= 14' src/error/structured.rs -> 0 hits; rg -n 'Exit[A-Z][a-zA-Z]*\s*(u8|i32|usize|u32)?\s*=\s*[0-9]' src/ -> 0 hits. Read the full ErrorCode::exit_code() match at src/error/structured.rs:224-266: it returns only 1-8, and AlreadyInitialized is folded into 2 with DatabaseNotFound|DatabaseLocked|SchemaMismatch|DatabaseError|SchemaSkew|NotInitialized (lines 229-233); the test at structured.rs:1049-1057 pins exactly {1..8}. So "tops out at 8" and "AlreadyInitialized -> 2" are accurate.
> 
> (2) But the claim's load-bearing sentence is FALSE: "A script cannot distinguish 'this workspace is mid-migration, retry later' (GO 14) from 'the database is locked' (LOCAL 2)." LOCAL ships a stable machine-readable `code` string orthogonal to the numeric bucket in every --json error envelope: structured.rs:42 says "These codes are stable and can be used for programmatic error handling", and to_json (structured.rs:498-508) emits {error:{code,message,hint,retryable,context}}. DB locked -> code DATABASE_LOCKED, retryable true (is_retryable, structured.rs:199). Schema behind/ahead of the binary (LOCAL's nearest analogue to "someone is migrating, come back later") -> code SCHEMA_SKEW, retryable false, context {direction, db_version, binary_version} (structured.rs:677-697). A CI script branches on .error.code with NO stderr grepping -- the exact property GO's numbers were introduced for (init_safety.go:25: "Stable values so CI scripts can branch on them without grep'ing stderr"). The claim's own ALREADY_INITIALIZED case is likewise distinct from NOT_INITIALIZED and DATABASE_LOCKED despite sharing exit 2 (structured.rs:556-557), which is exactly GO's ExitLocalExistsRefused(11) situation.
> 
> (3) This is a documented, supported contract, not incidental: br capabilities publishes the whole taxonomy as contract "br.capabilities.v1" (src/cli/commands/capabilities.rs:11, 165-206, 293), and docs/AGENT_INTEGRATION.md:442-492 ships a reference BrError wrapper whose branching fields are literally error.code / error.hint. docs/CLI_REFERENCE.md:2032, docs/ARCHITECTURE.md:429 and the structured.rs:211-220 doc comment ("Exit codes are grouped by error category") document the 0-8 numeric table as deliberately category-level. GO reaches "CI can branch" via numeric codes; LOCAL reaches it via stable string codes -- an equivalent capability by a different mechanism.
> 
> (4) Four of GO's five cited codes are codes for conditions LOCAL never implemented: rg -c -i 'destroy.?token' src/ -> 0 files; rg -i 'dolt|discard-remote|reinit-local' src/ -> no refusal logic (only an ID-format comment and a BEADS_REMOTE_SYNC_INTERVAL env NAME); rg -i 'migration freeze|freeze marker' src/ docs/ -> 0 hits; rg 'if-assignee|if_assignee|if-status|if_status' src/ tests/ -> 0 hits. No Dolt backend, no destroy token, no freeze marker, no --if-* conditional-update guard. The claim concedes this for the destroy token yet keeps it in scope. Closest real LOCAL analogue: the atomic claim guard at src/storage/sqlite.rs:2733-2761 raises BeadsError::validation("claim", ...) -> code VALIDATION_FAILED, exit 4, retryable true, context.field="claim" (distinguishable via context, not a dedicated band).
> 
> (5) Reachability checks for alternative homes all came up empty: process::exit outside doctor is only src/main.rs:692 (shutdown 128+signo, src/shutdown.rs:72), :748 and :1289 (handle_error -> err.exit_code()), src/cli/commands/lint.rs:199,232 (0/1), src/cli/commands/version.rs:238,271. The main.rs:104-131 ConcurrencyLost path is doctor --repair only. Doctor has its own separate richer dictionary (src/cli/commands/doctor_subsystems/exit_codes.rs:51-74: 0-6 plus sysexits 64/66/73/74, with refuse_gates.rs) which the claim already notes and correctly scopes out. No feature-gated variant (no cfg(feature) exit-code path), no MCP error-code surface, no env-var/flag override (rg 'BR_EXIT|EXIT_OVERRIDE' -> nothing), no destroy-token docs.
> 
> (6) DWS cross-check: /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/error/structured.rs:238-284 has the same 1-8 map plus UpdatePreconditionFailed->6, WorkflowCapacityExceeded->4, CloseIncomplete->3, ShuttingDown->130 -- and no 10-14. So present_in=go-only is correct and no Rust port adopted the band.
> 
> (7) GO upstream citations verified on disk: cmd/bd/init_safety.go:25-45 (10/11/12), cmd/bd/errors.go:183 (ExitMigrationFrozen=14) and :196-215 (CheckReadonly chokepoint), cmd/bd/update.go:824 (ExitGuardMismatch=13), cmd/bd/sync.go:42-44 (2/3/4). All plausible.
> 
> Net: a real numeric-band difference from GO, but the user-visible capability it delivers is already delivered in LOCAL by a stable, documented, published code enum plus retryable flag plus context discriminator. Four of the five cited refusal conditions do not exist in LOCAL. This is an ergonomics/docs preference (numeric vs string branching surface), not a missing capability. Residual, narrower than claimed: LOCAL lacks DWS's UpdatePreconditionFailed variant, so a lost claim race surfaces as generic VALIDATION_FAILED -- a missing ErrorCode variant, not a missing exit band, and it cannot rescue a claim built on destroy-token/migration-freeze/remote-divergence conditions LOCAL never had.

**Corroborator note**

> GO citations all verify and present_in=go-only is correct, but the claim's impact is mischaracterised and should be downgraded medium -> info. The five cited conditions (destroy-token, guard-mismatch, migration-freeze, remote-divergence) do not exist in LOCAL at all — zero hits for all five concepts across src/ and docs/ — so nothing is being "folded into category 2"; they are consequences of a Dolt-remote/multi-writer architecture LOCAL deliberately does not have (no Dolt backend, no remote integration). LOCAL also already emits a machine-readable `code` string plus a `retryable: bool` (src/error/structured.rs:498-508, is_retryable at :199-210) documented in docs/agent/ERRORS.md, which is exactly the retry-or-not signal the claim says scripts lack. The only LOCAL-applicable residue — the 2/3 code-space collision — is partly GO's own fault (GO overloads exit 2 for max-rows at max_rows.go:227 and flag conflicts at list_input.go:99) and LOCAL's SyncConflict=6 is actually more granular than GO's 2. Refuted as a deliberate fork divergence; keep as an informational note about cross-tool exit-code numbering only.

### A10. No `help --all` / `--list` / `--doc` / `--docs-root`, no help supplements, no golden help fixture

- **Domain:** `cli-ux-agent-contract` · **Type:** `missing_feature` · **Claimed severity:** medium · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> cmd/bd/help_all.go:18-22 declares helpAllFlag/helpDocFlag/helpListFlag/helpDocsRootFlag; :39-42 registers `--all` ("Show help for all commands in a single document"), `--doc`, `--list`, `--docs-root`; :83 `writeAllHelp` emits "# bd — Complete Command Reference" grouped by `root.Groups()`; cmd/bd/help_supplements.go:14-21 `//go:embed help_supplements/*.md` mapped by cobra CommandPath and appended after a command's flags; cmd/bd/help_all_test.go:36-56 `TestHelpAllIncludesTopLevelAndNestedCommands` asserts "# bd — Complete Command Reference", "[bd mol](#bd-mol)", "### bd mol", "#### bd mol pour"; cmd/bd/testdata/help.txt (golden help); cmd/bd/help_all_test.go:157-161 asserts `--doc` is registered on the help command; docs/cli-docs.pin pins the bd tag the generated docs are built from.

**Why the refuter killed it**

> I searched LOCAL exhaustively and found multiple real implementations that achieve the claimed user-visible outcomes, and I found the claim's own local_evidence to be factually false.
> 
> WHAT LOCAL ACTUALLY HAS (all read/verified this session):
> 
> 1. `src/cli/commands/capabilities.rs` (1096 lines) is a machine-readable whole-tree command contract. `command_capabilities()` (line 314) enumerates every top-level subcommand straight off the live clap surface via `Cli::command().get_subcommands()` — so it is auto-complete and cannot drift, unlike a hand-maintained list. `command_detail_for_path()` (line 335) + `find_command_path()` (line 368) walk a dotted path ("comments add") through arbitrary nesting, and `command_detail()` (line 390) emits long_about, aliases, every non-hidden argument (id/kind/long/short/required/action/value_names/defaults/possible_values), nested subcommands, examples, and per-command safety notes. `command_contract()` (line 650) is a 51-entry table of real invocations. `render_text()` (line 1022) prints a full command listing plus the deep detail. This is `br capabilities [--command <PATH>] [--format json|toon|text]`. That maps onto all three of GO's flags: whole-tree single document (≈ `--all`), single-command deep doc (≈ `--doc`), sorted top-level listing (≈ `--list`). It is exercised by 10+ e2e tests I enumerated in tests/e2e_schema.rs (e2e_capabilities_json_no_workspace, e2e_capabilities_command_detail_create_json, ..._nested_alias_json, ..._group_contracts_json, ..._workflow_safety_notes_json).
> 
> 2. `br schema commands` / `br schema all` (SchemaTarget::Commands, src/cli/mod.rs:1790; src/cli/commands/schema.rs:253 `build_commands`) emits a per-command JSON envelope shape map for the whole tree.
> 
> 3. `src/cli/commands/robot_docs.rs` ships an embedded long-form agent handbook (GUIDE const, ~80 lines) plus CANONICAL_COMMANDS — the help-supplement equivalent, machine-readable via `br robot-docs guide --format json`.
> 
> 4. `docs/CLI_REFERENCE.md` is a 56KB checked-in single Markdown document covering the whole command tree, grouped (Core/Query/Organization/Workflow/Sync & Config/Agent Integration/Diagnostics/Utilities = 155 headings) with a TOC whose anchors include nested subcommands (e.g. `### comments add`). This is literally the `bd help --all` artifact, committed. Crucially it is drift-guarded: `test_cli_reference_documents_current_clap_surface` at src/cli/mod.rs:4191 include_str!'s the doc, walks the live `Cli::command()` surface, and fails if any top-level command lacks a `### name` heading, plus asserts 22 `CLAP_DRIFT_SENTINELS` flag strings. That is the `--docs-root` regeneration guarantee, enforced as a test.
> 
> WHY THE CLAIM IS REFUTED ON ITS OWN TERMS:
> The claim states "no golden help fixture" and supports it with `ls tests/ | grep -i 'complet\|help\|schema\|output'`. That command only scans top-level *.rs filenames in tests/ and misses the entire tests/snapshots/ subtree. Two golden help fixtures in fact exist and are actively tested: tests/snapshots/snapshots/snapshots__snapshots__cli_output__help_output.snap (3.3KB, full `br --help`) and ..._create_help.snap (2.7KB, `br create --help`), driven by `snapshot_help_output`/`create_help` in tests/snapshots/cli_output.rs and wired in via `mod cli_output;` at tests/snapshots/mod.rs:679. The claim "no long-form examples in help at all" is also wrong: examples ship via `br capabilities --command <path>` (51-entry command_contract table) and per-command in CLI_REFERENCE.md. The `after_long_help` grep did return 0, but the conclusion drawn from it (no long-form examples anywhere) does not follow.
> 
> UPSTREAM CHECK: GO evidence is plausible — /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/help_all.go:18-26 declares helpAllFlag/helpDocFlag/helpListFlag/helpDocsRootFlag and registers `--all`/`--doc`/`--list`/`--docs-root` on Cobra's help cmd, with help_supplements.go and cmd/bd/testdata/help.txt present. But DWS is NOT capability-free: it has src/cli/commands/capabilities.rs, robot_docs.rs, schema.rs, a 103KB docs/CLI_REFERENCE.md, agent_baseline/help/{br_help,br_list_help,br_schema_help}.txt, and its own help snapshots. So the capability class is present in both upstreams; only GO's specific flag spelling on the help command is go-only. The claim's "present in: go-only" understates DWS.
> 
> RESIDUAL (why severity drops to low, not zero): the only genuine delta is ergonomic naming/discoverability — there is no `br help --all` spelling (the user must know to type `br capabilities`), and long examples are not appended to `--help` output itself. That is a polish item, not a missing capability.

**Corroborator note**

> Downgrade medium -> info. Two corrections to the claim: (1) LOCAL already ships `br capabilities --format json`, a one-call machine-consumable inventory of the whole command tree (capabilities.rs:334 walks Cli::command().get_subcommands(); :395-440 walks nested paths with args/defaults/possible-values), plus `br schema commands --format json` and `br robot-docs guide` — so the stated agent impact does not exist. (2) "No golden help fixture" is false: LOCAL has insta goldens at tests/snapshots/snapshots/snapshots__snapshots__cli_output__help_output.snap and ...__create_help.snap, driven by tests/snapshots/cli_output.rs:6-32; the GO file cited (cmd/bd/testdata/help.txt) is itself a 120-byte txtar testscript of substring assertions, not a byte-exact golden. Minor citation drift: writeAllHelp is help_all.go:91, not :83. What genuinely survives is only `help --docs-root` plus the scripts/generate-cli-docs.sh --check drift gate, which exists to feed GO's committed Mintlify site (docs/cli-reference/, docs/docs.json); LOCAL has no docs site and no consumer for a generated tree, so it is a docs-pipeline nicety rather than an agent-UX gap. GO capability is real and correctly characterized otherwise (main.go:2392 registers it in the shipped path; present_in go-only confirmed — DWS has no after_long_help/help_all/docs_root and no CLI-reference generator).

### A11. `br graph` output/verification surface: no Graphviz DOT, no dependency-direction walk, no HTML/box/open renderers, no `graph check` integrity subcommand

- **Domain:** `graph-deps` · **Type:** `missing_flag` · **Claimed severity:** medium · **Claimed present in:** both-upstreams
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: both-upstreams · corrected severity: low

**Claimed upstream evidence**

> DWS src/cli/mod.rs:3773-3775 `/// Walk dependencies instead of dependents: what is blocking this issue` / `#[arg(long, conflicts_with = "all")] pub dependencies: bool` and :3781-3783 `/// Emit Graphviz DOT notation (pipe to dot -Tsvg); overrides text/JSON rendering` / `pub dot: bool`; renderers at DWS src/cli/commands/graph.rs:1173 `sanitize_dot_string`, :1196 `push_dot_node`, :1218 `push_dot_edge`, :1227 `format_single_graph_dot`, :1249 `format_all_graph_dot`. GO cmd/bd/graph.go:357-362 registers `--all --compact --box --dot --html --open`; GO cmd/bd/graph.go:270-272 `var graphCheckCmd = &cobra.Command{ Use: "check", Short: "Check dependency graph integrity" ...}` registered at cmd/bd/graph.go:367, rendering `{clean, cycles, summary.cycle_count}` at :303-338.

**Why the refuter killed it**

> REFUTED on the graph-integrity portion; the renderers portion survives but is much narrower than claimed.
> 
> WHAT I RAN AND FOUND
> 
> 1. The claim's own search is the tell. `rg -n 'pub dot|graph_check|graphHTML' src` returns 0 hits because it only searched the two UPSTREAM IDENTIFIER LITERAL SPELLINGS. It never searched for the capability. A capability search immediately hits:
> 
> - `src/cli/mod.rs:2457` — `Cycles(DepCyclesArgs)` inside `pub enum DepCommands` (doc line 2456: "Detect and report dependency cycles").
> - `src/cli/mod.rs:2671` — `pub struct DepCyclesArgs { blocking_only, include_closed }`.
> - `src/cli/commands/dep.rs:1578` — `fn dep_cycles(...)`, dispatched at dep.rs:42-45 and again at dep.rs:71-74.
> - `src/cli/commands/dep.rs:1584` — `storage.detect_dependency_cycle_report(args.blocking_only)?` (impl at `src/storage/sqlite.rs:12029`, which builds the graph, splits active vs archived-closed, and returns `DependencyCycleReport`).
> - `src/cli/commands/dep.rs:353-366` — `struct CyclesResult { cycles, count, active_count, archived_closed_count, total_count, blocking_only, include_closed, scope, active_cycles, archived_closed_cycles }`.
> - `src/cli/commands/dep.rs:1643-1656` — non-zero exit: in rich and plain modes it returns `Err(BeadsError::validation("dep_cycles", "... cycle(s) detected"))` (Validation -> ErrorCode::ValidationFailed -> exit 4, per `src/error/structured.rs:1052`).
> 
> So LOCAL ships exactly the described outcome — full-graph cycle detection, machine-readable JSON with the cycle list and a count, and a failing exit status when the graph is not clean — under a different command name. It is a first-class, documented surface, not a hidden one: `docs/CLI_REFERENCE.md:831`, `docs/TROUBLESHOOTING.md:362`, `docs/AGENT_INTEGRATION.md:854` ("Cycle detected" -> `br dep cycles --json`), `src/cli/commands/quickstart.rs:43`, a `CommandContract` in `src/cli/commands/capabilities.rs:706-713` with three example invocations, and a read-only-routing entry at `src/write_combining.rs:1000`. It is also parity-tested against bd: `tests/conformance.rs:8741` ("dep cycles tests (4)") with `conformance_dep_cycles_none` asserting `br_count == bd_count` from the `--json` payload (conformance.rs:8776-8792), plus `tests/e2e_global_flags.rs:1030` and `tests/e2e_schema.rs:684-729`. Refuting the finder's "LOCAL has no integrity subcommand" on the strength of a two-token `rg` would be wrong.
> 
> Honest caveat I found while verifying: `dep_cycles` returns `Ok(())` early in the JSON/TOON branch (dep.rs:1605-1624) BEFORE the error return, so `br dep cycles --json` exits 0 even with cycles present — the count must be read from the payload. GO's `graph check` instead calls `SilentExit()` when `!result.Clean` (`cmd/bd/graph.go`, renderGraphCheck). That is a real but small delta in agent/CI gating, not an absence of the capability.
> 
> 2. The renderer claims do hold. `rg -n -i 'digraph|graphviz|dot -T|pub dot|graph_check|graphHTML' src tests` over `/Users/tranquangdang21/Projects/beads_rust/src` returns only false positives (`src/util/id.rs:357` "contains a dot after the hash", `src/config/mod.rs:52` "disallow dot-directories", dot-notation child IDs in sqlite.rs/close.rs) plus a `conflicts_with_all = ["check"]` on the unrelated `agents` command. `src/cli/mod.rs:3978-3990` GraphArgs is confirmed to have exactly `issue` / `--all` / `--compact`. `src/cli/commands/graph.rs` renderers are `render_single_graph_plain` (:1009), `render_single_graph_rich` (:1045), `render_no_dependents_rich` (:1131), `render_all_graph_rich` (:1154), `render_no_issues_rich` (:1251) — no DOT, HTML, box, or open path. Not feature-gated: `rg '#\[cfg\(feature' graph.rs dep.rs src/mcp/*.rs src/web/*.rs` returns nothing, and `Cargo.toml:164-167` lists only `default = ["web"]`, `mcp`, `web`. Not reachable via MCP: `src/mcp/resources.rs:773-966` has `beads://graph/health` (density, `graph_has_cycle` at :803) but no renderer. Not via the web UI: `src/web/api.rs` route list has no graph route (only `/api/p/{id}/deps` for CRUD), `stub_insights` (:828) is a hardcoded stub, and the `d3` hits in `src/web/static/_next/static/chunks/0hokuz6-4p0-z.js` are minified local variable names (`d3=(e,t)=>t.map(...)`) and `\xd3` hex escapes inside German i18n strings — not the D3 library. No env var or config key: `rg -i 'BR_.*(DOT|GRAPH|HTML)|graph.*format' src docs` finds nothing. The `--dependencies` direction walk does exist but only as `br dep tree -d down|up|both` (`DepDirection` at mod.rs:2640-2648, implemented at dep.rs:640/660/1026-1060), which the claim already concedes as partial mitigation. No CHANGELOG/UPGRADE_LOG statement of deliberate removal — CHANGELOG.md:275/313/389/462/471 discuss graph rendering and cycle detection without any deprecation note.
> 
> 3. Upstream citations checked out. DWS `/tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/mod.rs:3764-3784` is exactly as quoted (`dependencies` at :3773-3775 with `conflicts_with = "all"`, `dot` at :3781-3783), and its graph.rs has `sanitize_dot_string` (:1173), `push_dot_node` (:1196), `push_dot_edge` (:1218), `format_single_graph_dot` (:1227), `format_all_graph_dot` (:1249). GO `/tmp/beads_gap_audit/gastownhall_beads/cmd/bd/graph.go:357-362` registers `--all --compact --box --dot --html --open`; `graphCheckCmd` is at :270 and registered at :367; `renderGraphCheck` emits `{clean, cycles, summary.cycle_count}`. Citations are accurate. The claim's "present in: both-upstreams" is only true for the `--dot` + `--dependencies` subset: DWS has no `graph check` and no `--html/--box/--open`, and DWS also carries `DepCommands::Cycles` (mod.rs:2010-2011) — the same `dep cycles` LOCAL already has.
> 
> NET: the DOT/HTML/box/open export surface is a genuine but cosmetic gap (LOCAL ships `br dep tree --format mermaid` at dep.rs:1363-1416 and a rich layered connected-component view in `render_all_graph_rich`, so users are not left without machine-readable graph output). The `graph check` subcommand portion of the claim is refuted by `br dep cycles`.

**Corroborator note**

> Gap is real and every cited file:line is accurate, but downgrade medium -> low, and two impact claims must be corrected. (1) "No meaningful exit code for a clean-graph probe" is WRONG: LOCAL `br dep cycles` returns Err(BeadsError::validation(...)) on cycles in rich/plain mode (dep.rs:1651, :1657) -> ErrorCode::ValidationFailed -> exit 4 (structured.rs:241-245), so it is already a working CI probe; only the JSON/TOON branch returns Ok(()) unconditionally (dep.rs:1605-1624), a narrow quirk. (2) "Cannot export the graph for external tooling" is OVERSTATED: `br graph --all --json` already emits the full graph as nodes+edges (graph.rs:40-60, emitted at :215-227 and :411-420), and DWS's DOT renderer just wraps those same edges in `digraph` syntax, so --dot is ~10 lines of jq. Also note the bundle is mixed, not uniformly "both-upstreams": --dot is in BOTH, --dependencies is DWS-only (with `br dep tree` as DWS's own documented substitute, mod.rs:3760-3762), and --html/--box/--open plus `graph check` are GO-only cosmetics/niceties that run against LOCAL's agent-first direction.

### A12. No HTTP REST API surface: `br serve` is MCP-stdio only and the HTTP serve path is dead code

- **Domain:** `integrations-external` · **Type:** `missing_feature` · **Claimed severity:** high · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> cmd/bd/serve.go:58-59 `Use: serveCmdName, Short: "Serve the beads HTTP API over loopback"`; cmd/bd/serve.go:168-176 flags `--addr` (default 127.0.0.1:0), `--allow-non-loopback`, `--auth-token-file`, `--insecure-no-auth`, `--allowed-host`; internal/httpapi/server.go (87 KB), internal/httpapi/routes.go (32 KB), internal/httpapi/problem.go (80 KB), internal/httpapi/apigen/ (generated OpenAPI), internal/httpapi/spec/embed.go.

**Why the refuter killed it**

> REFUTED — LOCAL ships a working HTTP REST/JSON API via `br web`, and the finder's own evidence contradicts its conclusion.
> 
> WHAT I RAN AND FOUND
> 
> 1. The command exists and is registered. `src/cli/mod.rs:1070-1073` declares `#[cfg(feature = "web")] #[command(alias = "ui")] Web(WebArgs)`, and `src/main.rs:523` dispatches `Commands::Web(args) => beads_rust::web::run_server(&args, &overrides)`. `WebArgs` (src/cli/mod.rs:3854-3874) carries `--port/-p`, `--host` (default 127.0.0.1), `--strict-port`, `--no-open`, `--db`. The finder's line quoted (main.rs:520) is the *adjacent* `Serve` arm and led it to stop one line short.
> 
> 2. It is a genuine HTTP server. `src/web/mod.rs` builds an `axum::Router` (line 75) with 24 `.route(` rows, binds a real `std::net::TcpListener` via `bind_first_free` (line 218), and serves it with `axum::serve(tokio_listener, app)` (line 206). Not a stub, not a placeholder.
> 
> 3. It is ON BY DEFAULT — the decisive fact. `Cargo.toml` has `[features] default = ["web"]` and `web = ["dep:axum", "dep:tokio", "dep:tower-http", "dep:rust-embed"]`. `Cargo.lock:325` confirms axum 0.8.9 resolves. A stock `cargo install beads_rust` gets the HTTP surface; this is not an opt-in feature. `src/lib.rs:55-56` gates `pub mod web;` on it.
> 
> 4. The REST handlers are real, not stubs. `src/web/api.rs` (869 lines) has 14 storage-backed handlers — `list_beads` (154) → `storage.list_issues`, `get_bead` (190) → `get_issue`, `create_bead` (223) → `create_issue` (builds an `Issue`, calls `generate_id`, persists), plus `update_bead`, `delete_bead`, `set_status`, `add_comment`, `add_dep`, `remove_dep`, `archive_bead`, `list_projects`, `doctor`, `get_config`, `update_config` — each opening storage via `config::open_storage_with_cli` inside `tokio::task::spawn_blocking` (11 call sites). CRUD over JSON with proper status codes (200/201/400/404/500). Only 9 routes are static stubs (gate, assist, human, insights, activity, gamification, attachments, publish, projects PATCH, fs, update_check).
> 
> 5. Documented as a shipped feature. `docs/CLI_REFERENCE.md:1394-1432` documents `br web` and its `/api/*` JSON API; `CHANGELOG.md:25-35` records "`br web` — new subcommand that serves a static web UI and REST API over HTTP using an embedded axum server", commit 4388299d "feat: add br web subcommand with embedded axum HTTP server".
> 
> WHY THE FINDER MISSED IT
> 
> The claim states axum/tokio hits occur "ONLY under src/web/ (the web feature)" and then concludes "there is no REST/JSON daemon". That is a self-contradiction: `src/web` IS the REST/JSON daemon, and it is the default feature. The finder noticed the right directory and drew the inverse conclusion, apparently discounting it as "the web feature" while never checking `[features] default`.
> 
> SUB-CLAIMS THAT DO CHECK OUT (but do not rescue the gap)
> 
> `br serve` is indeed MCP-stdio-only (main.rs:520 → `mcp::run_serve`). `src/cli/commands/serve.rs` is indeed unreferenced — my `rg 'commands::serve|serve::run|mod serve' -g '*.rs' src` returned zero hits, so it is not even declared as a module (the claim's "compiled but referenced by nothing" is itself inaccurate; it is uncompiled dead file), and its `cmd_start` does print the `{"status":"not_implemented"}` JSON. All true — and irrelevant, because the HTTP API ships via `br web`.
> 
> UPSTREAM SANITY CHECK
> 
> GO evidence is plausible and verified: `cmd/bd/serve.go` is 43 KB with `Use: serveCmdName` / `Short: "Serve the beads HTTP API over loopback"` at lines 58-59, and `internal/httpapi/` is a large real package (auth.go, batch_apply.go, close.go, claim.go, …) plus `apigen/`. DWS genuinely lacks any HTTP surface: no `src/web`, `default = ["self_update"]`, zero hits for axum/hyper/TcpListener/tokio::net/Router across the tree, and `Commands::Serve(_) => beads_rust::mcp::run_serve` at DWS src/main.rs:823. So among the two upstreams the capability is genuinely GO-only — but the audited LOCAL code has it too, which is exactly why this is not a gap.
> 
> RESIDUAL (the real, much smaller gap)
> 
> LOCAL's `br web` API is unversioned (`/api`, not `/v0`), has no embedded OpenAPI document, no `/healthz` probe, no `/v0/beads/context` capability report, no `--auth-token-file`/`--allowed-host`, and ships `CorsLayer::permissive()`. GO's is API-first, spec'd, versioned, and authenticated. That is a genuine API-hardening delta worth low severity — but categorically different from the claimed "no HTTP REST API surface / no REST/JSON daemon", which is false.

**Corroborator note**

> Claim over-hyped and partly factually wrong. CORRECTED: `br` DOES ship an HTTP REST API — `br web` (alias `br ui`), dispatched at src/main.rs:523, gated on the `web` feature which is in `default` (Cargo.toml `default = ["web"]`), serving an axum router with ~25 routes (src/web/mod.rs:75-175) backed by ~14 real CRUD/comment/dep/config/doctor handlers (src/web/api.rs) on 127.0.0.1:3000. So "No external automation can talk to br over HTTP: every client must fork a br subprocess per call" is false. The accurate residual gap is narrower: no VERSIONED/spec'd/hardened HTTP surface — LOCAL routes are unversioned (/api/p/{id}/... not /v0/...), no OpenAPI document, no /healthz liveness probe, no /v0/beads/context capability negotiation, no --auth-token-file / --allow-non-loopback / --allowed-host security model (unconditional CorsLayer::permissive()), browser auto-open positioning it as a human UI, and 9 of 23 handlers are stubs. Two secondary inaccuracies: (a) "70+ route rows" is inflated — routes.go has 36 pattern rows / ~40-45 distinct /v0 paths; (b) serve.rs is not "compiled but referenced by nothing" — `mod serve` is declared nowhere and `crate::cli::ServeCommand` does not exist outside serve.rs itself, so the file is an orphan that would not compile if wired in. Downgrading high -> info: this is a documented deliberate fork divergence (docs/BD_VS_BR.md:14 marks the network daemon out of scope; br's agent-integration path is MCP, which ships) with a working on-by-default LOCAL alternative, and it is an integration-surface nicety rather than a core agent-workflow, data-integrity, or data-loss blocker. Worth noting the claim's own parenthetical IS correct: GO has no `bd daemon` at all, so the LOCAL doc row referencing `bd daemon rpc`/`bd daemon start` is stale and should be refreshed to reference `bd serve` vs `br serve` (MCP) vs `br web` (HTTP UI).

### A13. LOCAL has a full web UI and federation surface that DWS entirely lacks — DWS is a regression, not a superset, in this domain

- **Domain:** `integrations-external` · **Type:** `architecture_gap` · **Claimed severity:** info · **Claimed present in:** both-upstreams
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> DWS: `rg -n 'axum|hyper|TcpListener|tokio|HttpServer|httpapi|web::' -g '*.rs' src` -> axum 0, TcpListener 0, tokio 0, httpapi 0, web:: 0 hits (only 1 incidental `hyper` string and 13 `listen` matches, none forming a server); `ls -d src/*/` shows no web/ or server/ dir; `rg -ni 'federation' -g '*.rs' src` -> 0 hits; DWS src/cli/commands/ has no serve.rs and no federation.rs. GO: no web UI either — `rg -ni 'webui|web ui|index.html' cmd/bd/serve.go` -> 0 hits (its embedded assets are only internal/httpapi/spec/embed.go and internal/templates/agent_skill.go).

**Why the refuter killed it**

> This is not a gap in LOCAL — LOCAL genuinely ships every capability the claim credits it with, so there is nothing in LOCAL to refute. I confirmed each local_evidence item directly and it is real, not hand-waved. REFUTED (discard from the LOCAL-gap list; at most it is a DWS-gap note).
> 
> What I actually ran and found:
> 
> LOCAL (/Users/tranquangdang21/Projects/beads_rust):
> - Cargo.toml features: `default = ["web"]`, `web = ["dep:axum","dep:tokio","dep:tower-http","dep:rust-embed"]`; deps axum 0.8, tokio, tower-http, rust-embed -> the web server is ON BY DEFAULT, not a hidden opt-in.
> - src/web/ present and real: mod.rs (10KB), api.rs (29KB), assets.rs (4KB), static/ (embedded Next.js SPA). `rg 'axum|Router|TcpListener|bind' src/web/` shows a real axum::Router with ~40 REST routes (list/create/get beads, set_status, comments, dep add/remove, archive, projects, doctor, config, update) and src/web/mod.rs:203 `tokio::net::TcpListener::from_std(...)` + fallback static asset service.
> - src/cli/mod.rs:1063-1088 wires three real subcommands: `Serve(crate::mcp::ServeArgs)` (stdio MCP), `Web(WebArgs)` (embedded web UI server), `Federation(FederationCommand)` ("Manage federation peers for P2P sync").
> - src/cli/commands/federation.rs (20KB): FederationCommand{Add,List,Remove,Sync,Info} with a real `run()` dispatch to cmd_add/cmd_list/cmd_remove/cmd_sync/cmd_info, backed by a real `federation_peers` table in src/storage/schema.rs (v17+, Issue #36). (Only a minor nuance: the Sync variant's doc comment says "stub", but Add/List/Remove/Info and the schema are fully implemented.)
> - src/mcp/ present and large: mod.rs 27KB, tools.rs 188KB, resources.rs 49KB, prompts.rs 33KB.
> 
> DWS (/tmp/beads_gap_audit/dicklesworthstone_beads_rust) — confirms the claim's negative:
> - `rg 'axum|hyper|TcpListener|tokio|web::' src` -> 0 real hits (one incidental `Markdown::...hyperlinks` in format/markdown.rs:315).
> - `rg 'axum|hyper|tokio|actix|warp|rocket|tiny_http|tower|poem' Cargo.toml` -> nothing; features are only `default=["self_update"]`, mcp, self_update. No web feature at all.
> - `ls -d src/*/` -> no web/ or server/ dir. `rg -ni federation src` -> 0; no federation_peers table; no serve.rs/web.rs/federation.rs in src/cli/commands/.
> - Broad `rg 'TcpListener|bind\(|std::net|serve\('` -> only SQLite `.bind(...)` calls, no server. DWS does have src/mcp/ + `br serve` (MCP stdio, same as LOCAL) and does have `br vcs-status` (src/cli/commands/vcs.rs; cli/mod.rs:988 `#[command(name="vcs-status")] VcsStatus(VcsStatusArgs)`) — so LOCAL has no vcs-status, consistent with the claim.
> 
> GO (/tmp/beads_gap_audit/gastownhall_beads) — the claim UNDERSTATES it: `rg -ni webui|web ui|index.html cmd/bd/serve.go` -> 0 (no web UI), but GO has a real network server (cmd/bd/serve.go 43KB + 100+ *_proxied_server.go files), a full internal/httpapi package (auth.go, batch_apply.go, SSE events, apigen), AND a real federation CLI: cmd/bd/federation.go with runFederationSync/Status/AddPeer/RemovePeer/ListPeers, plus storage.FederationStore/FederationPeer in backend and a `federation.remote` config key. So GO matches LOCAL on federation (CGO/Dolt-gated per federation_nocgo.go) and exceeds it on API breadth — it is not merely "between the two on listeners."
> 
> Net: the claim's factual core is correct — LOCAL is a superset of DWS in integrations-external, and DWS has strictly fewer surfaces. But the claim's own `present_in: both-upstreams` is wrong/self-contradictory (its description states DWS has NONE of these). Of the two upstreams, only GO has any of it (listener + federation; no web UI); the web UI is LOCAL-only. Severity stays info — nothing to fix in LOCAL.

**Corroborator note**

> The DWS side of this claim is accurate but should be restated, and the LOCAL side is overstated. Correct framing: DWS (Dicklesworthstone) ships no network listener, HTTP API, web UI, or federation command — verified by 0 hits for axum/tokio/TcpListener/httpapi (the single "hyper" match is src/format/markdown.rs:315 `.hyperlinks(true)`) and 0 federation hits. That is a deliberate fork divergence: DWS is a hardening fork (franken_sync/, doctor_subsystems/refuse_gates.rs, vcs-status) that dropped the network surface, not a regression.
> 
> But LOCAL does not have "a full web UI and federation surface" in the sense the claim implies. Two corrections: (1) the web UI assets are gitignored (.gitignore:237, src/web/.gitignore) and built only by CI (scripts/build-web.sh, "NOT run on the user's machine"), so a `cargo install --git` build embeds an empty src/web/static/ and every static route 404s — only official CI release binaries carry the UI; and a large part of the REST surface is stubs returning `{}` (src/web/api.rs:801-830, wired at src/web/mod.rs:109-149). (2) LOCAL's federation is not network P2P: federation.rs:374-380 rejects any `://` protocol and supports only `file://` or a bare path, and the crate has no HTTP client. On federation specifically the correct ranking is GO > LOCAL > DWS, since GO's cmd/bd/federation.go drives real Dolt remotes (dolthub/S3/GCS/SSH).
> 
> Also fix present_in: it is go-only, not both-upstreams — DWS has none of these capabilities and GO has no web UI. Also drop the claim that DWS has "no external trackers": DWS parses and validates external refs (src/util/id.rs:1380-1385, src/validation/mod.rs:220-232), though on that axis LOCAL is no better, so it is a wash rather than a LOCAL advantage. Net effect on the audit: keep this as an info-level ranking caveat (GO leads on trackers and on real federation, LOCAL leads only on a CI-only web UI, DWS leads on hardening subsystems), and do not record it as an actionable gap.

### A14. `br vcs-status` — explicit, bounded Git diagnostics (DWS-only, 67 KB) has no LOCAL equivalent

- **Domain:** `integrations-external` · **Type:** `missing_feature` · **Claimed severity:** medium · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: dws-only · corrected severity: low

**Claimed upstream evidence**

> DWS src/cli/commands/vcs.rs:1-9 module doc ("Explicit, bounded VCS diagnostics ... only a direct `br vcs-status` invocation reaches the process capability below"); vcs.rs:34 `const STATUS_SCHEMA: &str = "br.vcs-export-status.v2"`; vcs.rs:37-60 `VcsExportStatus` struct with `observation_atomic: false`, `object_format`, timeout fields; vcs.rs:37-39 byte/timeout caps; registered as a subcommand at DWS src/cli/mod.rs:988 `#[command(name = "vcs-status")] VcsStatus(VcsStatusArgs)` with args at cli/mod.rs:1475-1484 (`--jsonl`, `--allow-external-jsonl`); dispatched at DWS src/main.rs:815.

**Why the refuter killed it**

> The finder's local_evidence is factually wrong on its two load-bearing claims. I found a real, live, tested LOCAL implementation of the exact user-visible capability the gap describes.
> 
> WHAT I FOUND — `br sync --status` emits a read-only git-visibility diagnostic for the JSONL export:
> 
> - `src/cli/commands/sync.rs:88-141` — `SyncStatus.git_export: GitExportStatus` plus the `GitExportStatus` struct: `available`, `tracked` (via `git ls-files --error-unmatch`), `worktree_clean`, `index_clean`, `head_hash` (via `git rev-parse HEAD:./<file>`), `worktree_hash` (via `git hash-object`). Doc comment at :94-103 states the intent verbatim: "Read-only git visibility for the JSONL export (beads_rust#338) ... surfaces exactly what git knows about the export file."
> - `src/cli/commands/sync.rs:901-939` — `git_export_status()` is the live implementation, running 6 read-only git probes (`rev-parse --is-inside-work-tree`, `status --porcelain`, `ls-files --error-unmatch`, `rev-parse --verify --quiet`, `hash-object`).
> - `src/cli/commands/sync.rs:990` — populated in `execute_status`, i.e. it is real output, not dead code.
> - `src/cli/commands/sync.rs:885-896` — `porcelain_cleanliness()` decodes porcelain columns, with the comment "untracked files report false: their content is invisible to git" — the same diagnostic question the gap claims is absent.
> 
> REACHABILITY (checked, ruling out the "dead / feature-gated" refutation routes):
> - Dispatch: `sync_operation()` at :580 returns `SyncOperation::Status` on `--status`, dispatched at :512 to `execute_status`. Arg declared at `src/cli/mod.rs:3355-3363` (`pub status: bool`) with help text explicitly documenting "a read-only `git_export` block reporting whether the tracked JSONL is clean in the surrounding git repo ({"available": false} when git or a repo is absent)".
> - NOT feature-gated: no `#[cfg(...)]` or `cfg(feature = ...)` anywhere around `SyncStatus` (:67) or `git_export` (:990). Not behind an env var, config key, or hidden flag.
> - NOT only via MCP: it is on the primary CLI path, so the `src/mcp` question is moot.
> - Unit tests: `test_porcelain_cleanliness_maps_status_columns` (:3361), `test_git_export_status_unavailable_outside_git_repo` (:3377).
> - E2E test: `tests/e2e_sync_status_health.rs` drives real git repos and asserts `available`/`tracked`/`worktree_clean`/`index_clean`/`head_hash`/`worktree_hash` across the untracked, committed, and dirty cases. Behavior is contract-tested, not aspirational.
> 
> TWO CLAIMS FALSIFIED:
> 1. "LOCAL has no such command and no equivalent diagnostic" — false; `git_export` is an equivalent diagnostic answering exactly "is the configured JSONL export visible to Git".
> 2. "LOCAL git integration is limited to writing hook files (src/hooks/mod.rs) and never inspects Git state" — false on two counts. `br sync --status` shells out to git 6× per invocation, and `src/worktree/mod.rs:512-531` `capture_git()` is a fully hardened git wrapper (sets `core.hooksPath=`, `GIT_TEMPLATE_DIR=`, `env_remove("GIT_DIR")`, `env_remove("GIT_WORK_TREE")`) driving `br worktree` and a `check-ignore` probe at :476-482. The finder's `rg` only searched the literal names `vcs.status|vcs_status|VcsStatus` and then concluded absence by name, never by capability.
> 
> The residual, genuinely-absent delta (real but narrow — hardening/packaging, not capability):
> - DWS ships a dedicated standalone `br vcs-status`; LOCAL folds the diagnostic into `br sync --status`, so it is not isolated to an explicit opt-in invocation (DWS `src/cli/mod.rs:3022` states "sync never probes VCS"; LOCAL's probe lives inside the sync path, the opposite architectural choice).
> - DWS has a versioned schema id `br.vcs-export-status.v2` (:31) and an `observation_atomic` honesty field (:51); LOCAL's block carries no schema id.
> - DWS has a hard timeout, byte caps, and git invocation hardened against hooks/filters/fsmonitor/redirection. LOCAL's `run_git_capture` (`sync.rs:864-874`) neutralizes nothing — no `core.hooksPath=`, no `GIT_DIR`/`GIT_WORK_TREE` removal (note LOCAL already has the hardened pattern in `worktree/mod.rs`, so this is an inconsistency, not an unknown).
> - DWS has dedicated `--jsonl` / `--allow-external-jsonl` args; LOCAL derives the path from sync path policy.
> 
> UPSTREAM CITATION CHECK (all plausible and verified): DWS `src/cli/commands/vcs.rs` exists, 2102 lines / 67 KB as claimed; `STATUS_SCHEMA` at :31, `VcsExportStatus` at :44, `observation_atomic` at :51, subcommand registered at `cli/mod.rs:988-989`, `VcsStatusArgs` at :1477, dispatched at `main.rs:815`. GO confirmed to lack any `vcs-status` and any JSONL-export-visibility diagnostic; its `cmd/bd/doctor/tracked_runtime.go:83 CheckTrackedRuntimeFiles` serves a different purpose (flagging mistakenly-tracked lock/daemon/socket runtime artifacts), not export visibility. So `present_in = dws-only` is correct.
> 
> Per the verifier rule — any real implementation or documented equivalent found => refuted — this gap is discarded. What remains is a hardening/isolation polish item, so severity drops from medium to low.

**Corroborator note**

> The claim overstates the gap. LOCAL already ships the equivalent diagnostic: `br sync --status --json` emits a `git_export` block (src/cli/commands/sync.rs:105 struct, :901 probe fn, :990 wiring) whose contract is field-for-field identical to DWS's — available/tracked/worktree_clean/index_clean/head_hash/worktree_hash — and whose doc comment states the exact motivation the claim says LOCAL lacks (sync.rs:96-101). The claimed impact ("LOCAL users get no answer to whether the JSONL is committed/tracked/clean") is therefore false. DWS itself keeps the same struct in its sync path but hard-stubs it to `reason:"not_probed"` and redirects to the dedicated command (DWS cli/mod.rs:3019-3022), which shows LOCAL carries the earlier form of this same feature rather than lacking it. The genuine remaining delta is narrower and is a hardening story, not a missing-feature story: (a) no dedicated `vcs-status` command (LOCAL embeds it in `sync --status`); (b) LOCAL's `run_git_capture` (sync.rs:864-874) is a bare `Command::new("git").output()` with no timeout, no byte cap, and no env/hook/fsmonitor neutralization, versus DWS's validate_timeout 25..=30_000ms plus shared deadline and GIT_CONFIG_NOSYSTEM/GIT_CONFIG_GLOBAL=null_device/GIT_OPTIONAL_LOCKS=0/core.fsmonitor=false; (c) LOCAL uses `git hash-object` (sync.rs:928), which can invoke clean filters, whereas DWS computes the raw hash in-process to avoid filter execution; (d) no --jsonl/--allow-external-jsonl override and no object_format/unmerged_index_stages/stable schema id. Severity should drop medium → low: the diagnostic exists and answers the question; only its hardening and argument surface lag, on a read-only block off the write path, with no data-integrity or workflow-blocking impact. GO genuinely lacks the capability either way (its doctor gitignore.go / tracked_runtime.go check the inverse — runtime files that should NOT be tracked).

### A15. No lease-based claim lifecycle: no lease TTL, no `bd heartbeat`, no `bd reclaim`

- **Domain:** `model-lifecycle` · **Type:** `missing_feature` · **Claimed severity:** high · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: go-only · corrected severity: medium

**Claimed upstream evidence**

> GO cmd/bd/heartbeat.go:14 `var heartbeatCmd = &cobra.Command{` (Use "heartbeat <id>", alias "hb", GroupID "issues"); cmd/bd/reclaim.go:16-17 `var reclaimCmd = &cobra.Command{` / `Use: "reclaim"` with `--older-than` (cmd/bd/reclaim.go:227); internal/types/types.go:63 `LeaseExpiresAt *time.Time`, :64 `HeartbeatAt *time.Time`, :73 `LeaseGrantedNode string`; claim.go:1-88 (ParseClaimConflict shim over issueops.ClaimConflictError).

**Why the refuter killed it**

> REFUTED as framed — the literal GO primitive is genuinely absent, but the claim misclassifies a deliberate design decision (with a real implementation the finder under-credited) as a missing_feature.
> 
> WHAT I RAN AND FOUND (LOCAL = /Users/tranquangdang21/Projects/beads_rust):
> 
> 1) The claim's negative greps are ACCURATE. `rg -n 'lease_expires|LeaseGranted|heartbeat_at' src/ tests/` -> exit 1, 0 hits. `rg -ni 'lease' src/cli/ | grep -viE 'release'` -> 0 hits. No Heartbeat/Reclaim/Unclaim variant in `pub enum Commands` (src/cli/mod.rs). No lease columns in `src/storage/schema.rs`. So there is no lease TTL and no auto-reaper — that part checks out.
> 
> 2) BUT the claim understates LOCAL badly. `src/coordination.rs` is a 23KB policy engine, not "advisory strings". It defines an owner-class-aware age policy: `SWARM_STALE_CANDIDATE_AFTER_MINUTES = 2*60`, `SWARM_ABANDONED_LIKELY_AFTER_MINUTES = 8*60`, `HUMAN_STALE_CANDIDATE_AFTER_MINUTES = 24*60`, `HUMAN_ABANDONED_LIKELY_AFTER_MINUTES = 72*60` (src/coordination.rs:15-22); a `ClaimClassification` enum (Fresh/StaleCandidate/AbandonedLikely/NoMailSnapshot/Ambiguous, :161-179); and reservation-evidence gating where an active reservation *blocks* reclaim even for very old claims (test `active_reservation_blocks_reclaim_even_when_old`, :915).
> 
> 3) Real first-class CLI surface exists that the finder never mentioned:
>    - `Commands::Coordination { command: CoordinationCommands }` at src/cli/mod.rs:797-801 -> `br coordination status` (src/cli/commands/coordination.rs, 23KB) emitting a `br.coordination.v1` envelope. Confirmed read-only: I grepped its storage calls; every one is a read (`storage.list_issues`, `get_labels_for_issues`, `count_relation_counts_for_issues`, `get_latest_comments_for_issues`, `get_blocked_ids`, `count_issues_with_filters`) — no `update_issue`/assignee write.
>    - `Commands::Scheduler` (src/cli/mod.rs:943) -> `br scheduler` ranks ready work and attaches `evidence.stale_claim` (assignee, owner_kind, updated_age_minutes, stale/abandoned thresholds, classification, recommended_action, is_stale) to every candidate.
>    - `Commands::Stale(StaleArgs)` (src/cli/mod.rs:958) -> `br stale --days N --status in_progress` (src/cli/commands/stale.rs), a real age-based detector that prints assignee per row.
>    - `AuditCoordinationArgs` (src/cli/mod.rs:2842) -> `br audit coordination` records snapshots as bounded audit interactions.
>    - MCP resource `beads://coordination/status` (registered at src/cli/mod.rs:4216).
> 
> 4) DOCS CONTAIN AN EXPLICIT "THIS IS INTENTIONAL" STATEMENT — the decisive evidence. docs/CLI_REFERENCE.md:789: "There is not a separate reclaim command; the audit comment plus `update --claim` is the documented recovery workflow." docs/AGENT_INTEGRATION.md:320-323: "`br coordination status` never auto-reclaims, never runs git, and never creates or releases Agent Mail reservations. The output is advisory only." docs/SWARM_SCALE_TUNING.md:180: "Safe reclaim remains a two-step manual sequence." docs/COORDINATION_EVIDENCE.md:44-45 documents the classification table. This is a documented deliberate equivalent, not an accidental omission — and it is consistent with br's core non-invasive principle (AGENTS.md: "br NEVER executes git commands automatically", no daemons, no hooks): br deliberately refuses to auto-mutate agent state.
> 
> 5) I chased the strongest refutation candidates and they all failed. The `Wisp` system looked promising — `WispType::Heartbeat` with "TTL: 6h" (src/model/mod.rs:561-562) and `br wisp gc` — but `WispCreateArgs` (src/cli/commands/wisp.rs:55-67) has no link to a real issue claim, and `execute_gc` (:313-352) only filters `i.ephemeral && i.updated_at < cutoff` and calls `delete_issue`. It never reverts an in_progress non-ephemeral claim. So wisps are an ephemeral-issue tracker, not a claim lease. I also checked feature gates (`#[cfg(feature=...)]` list is only mcp/web/self_update — nothing gated away), and searched for mutating flags (`--reclaim|--unclaim|--expire|--reap`) -> none.
> 
> 6) "Permanently stuck" is overstated. A crashed claim is not invisible: it is discoverable via `br list --status in_progress`, `br stale --status in_progress`, `br coordination status`, and surfaced inline in `br scheduler` recommendations as classified evidence, with a documented 2-command recovery path.
> 
> UPSTREAM SANITY CHECK (GO = /tmp/beads_gap_audit/gastownhall_beads) — all citations verified exactly as quoted: cmd/bd/heartbeat.go:14 `var heartbeatCmd = &cobra.Command{` (Use "heartbeat <id>", alias "hb", GroupID "issues"); cmd/bd/reclaim.go:16-17 `var reclaimCmd = &cobra.Command{`; cmd/bd/reclaim.go:227 registers the `older-than` Duration flag defaulted to `2*issueops.DefaultLeaseTTL`; internal/types/types.go:63 `LeaseExpiresAt *time.Time`, :64 `HeartbeatAt *time.Time`, :73 `LeaseGrantedNode string`. GO's feature is real and exactly as described.
> 
> DWS = /tmp/beads_gap_audit/dicklesworthstone_beads_rust: has the same `Coordination` (cli/mod.rs:792), `Scheduler` (:893) and `Stale` (:908) surfaces and equally no lease/heartbeat/reclaim. Its "lease" hits are unrelated (sole-opener DB file lock in doctor_subsystems/engine.rs:37-133, plus "release" substrings). Note DWS coordination.rs differs from LOCAL's — LOCAL's is the more developed one.
> 
> CONCLUSION: the gap does not exist as a "missing_feature" in the sense the claim asserts. It is a deliberate, documented architectural divergence where LOCAL replaced GO's automatic lease reaper with a read-only, machine-readable detection surface (coordination status / scheduler / stale / MCP) plus a manual auditable recovery, on non-invasive-design grounds.

**Corroborator note**

> Gap stands; the "high / highest-severity" framing does not. Three specific overstatements, then the calibration.
> 
> 1. "reaps dead-worker claims automatically" — overstated. GO does not auto-reap. cmd/bd/reclaim.go:30-32 instructs: "Run it from a supervisor on a timer with a window of roughly 2x the claim TTL." It is an explicit command an external supervisor schedules, with a 2x-TTL grace window (reclaim.go:227) so a GC pause or clock skew does not rob live work. The lease is also deliberately weak-blast-radius: heartbeat.go:24-25 "Leases live in an ephemeral, node-local table: heartbeats write no Dolt commit and no history"; heartbeat.go:27 "Leases are only enforceable on the node that granted them". So even upstream, cross-machine recovery is advisory, not automatic. LOCAL is behind a supervisor-run command, not behind a built-in reaper.
> 
> 2. "no fencing token that a supervisor can check" — factually wrong about GO. GO has an optimistic-concurrency token: internal/types/types.go:99 `RowVersion int64` with `json:"-"`, documented at types.go:76-88 as an EQUALITY-ONLY token "a change signals the row was mutated since you read it", projected to the wire as `revision` for guarded clients. GO also has a live-claim fence on the mutation path (internal/storage/issueops/claim.go:321; cmd/bd/assign.go:74 "same live-claim fence as bd update -a"). What LOCAL lacks is lease-*expiry* fencing, not fencing per se.
> 
> 3. "Every reclaim is an unguarded manual write" — too strong. LOCAL's claim path is guarded: src/cli/commands/update.rs:1166-1168 sets `expect_unassigned: args.claim` and `claim_exclusive`, and update.rs:822-825 refuses to overwrite a live claim. It is the *reclaim* step that is unguarded, not the claim.
> 
> Severity: medium, not high. Reasons to downgrade despite the gap being real:
> - Not data loss and not data integrity. A stuck claim hides work; it does not corrupt or destroy the database, and the issue row plus its history remain fully intact and auditable.
> - LOCAL has a reasonable alternative, which the claim itself concedes: the advisory `br coordination status` machine plus the AGENTS.md:579 protocol, with explicit evidence requirements and staleness thresholds (2h swarm / 1 business day human) and a rule to never steal a claim with live reservations or dirty work in the same files. That is a deliberate, documented answer to exactly this failure mode, not an omission.
> - TTL-based liveness is not alien to the local product: br already ships TTL/ephemerality primitives (src/model/mod.rs:555+ `WispType` "TTL-based compaction"; src/merge_slot/mod.rs:294 merge-slot mutual exclusion), and AGENTS.md states Agent Mail is the coordination layer whose `file_reservation_paths(..., ttl_seconds=3600)` reservations are TTL'd and expiring. The lease concept exists one layer out, so br not re-implementing it is a defensible division of labour, not a hole.
> - The feature is daemon-shaped and br's stated architecture is the opposite: AGENTS.md "Key Design Decisions — Non-invasive by design — br NEVER executes git commands automatically; no daemons, no hooks." A lease/heartbeat loop that something must keep alive is a meaningful architectural cost for a tool whose selling point is that it does nothing on its own.
> 
> Net: a real operational hazard for large multi-agent swarms (a crashed agent's bead does stay invisible to `br ready` until a human or peer intervenes, and that is the claim's strongest and correctly-stated point), worth a roadmap item — but not a core-workflow blocker, not a correctness or data-loss defect, and not "the highest-severity lifecycle gap" for a product that intentionally delegates liveness to Agent Mail and ships a documented manual recovery path. Claimed effort "large" is fair.

### A16. bd doctor --fix has confirmation / --yes / --interactive; br doctor --repair is unconditionally non-interactive

- **Domain:** `observability-ops` · **Type:** `missing_flag` · **Claimed severity:** low · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> cmd/bd/doctor.go:47-48 (doctorYes, doctorInteractive vars); :343-344 (Flags: --yes/-y 'Skip confirmation prompt (for non-interactive use)', --interactive/-i 'Confirm each fix individually'); cmd/bd/doctor_fix.go:70-75 (lists fixable issues, then 'if doctorInteractive { applyFixesInteractive(...); return }'); :82-100 ('if !doctorYes { ... term.IsTerminal(os.Stdin.Fd()) ... non-interactive mode warning naming `bd doctor --fix --yes` ... Continue? (Y/n) ... bufio ReadString }'); also :312 fix.RepoFingerprint(path, doctorYes) and cmd/bd/doctor_validate.go:155 the same --yes gate.

**Why the refuter killed it**

> I searched hard to refute and could NOT find a literal `--yes`/`--interactive` implementation in LOCAL — but I did find a documented, deliberate equivalent, and the `--yes` half of the claim is a functional no-op. Both facts point to refutation at info severity.
> 
> WHAT I CONFIRMED ABSENT IN LOCAL (all read-only; no cargo, no writes):
> - `DoctorArgs` (src/cli/mod.rs:3606-3704) has no `yes`/`interactive` field. `rg -n 'long = "yes"|long = "interactive"|visible_alias = "yes"|short = .y.' src/` -> 0 hits repo-wide. `rg 'pub (yes|assume_yes|non_interactive|force_yes)|long = "(yes|assume-yes|non-interactive)"' src/cli/` -> 0 hits, i.e. no `br` command anywhere has this flag.
> - src/cli/commands/doctor.rs is 768KB; `rg -c -i 'read_line|stdin\(\)|is_terminal|confirm|prompt'` -> 5 hits, ALL non-prompts: `issue.status.is_terminal()` (lines 10400, 10410), prose "confirm" in remediation text (10544), a "confirm that the DB is truly healthy" comment (11599), and a TOCTOU "Re-confirm" comment (8593). Zero `IsTerminal`/TTY gating on the repair path.
> - Not feature-gated: `rg 'cfg\(feature' src/cli/commands/doctor.rs src/cli/commands/doctor_subsystems/*.rs` -> 0 hits (Cargo.toml features are only `web`/`mcp`/`self_update`; `self_update` gates only `upgrade`).
> - Not reachable via MCP: `rg 'doctor' src/mcp/` -> 0 hits.
> - doctor_subsystems/{mutate,refuse_gates,capabilities_doctor,exit_codes,surface,run_dir}.rs searched for consent/prompt/confirm/yes/interactive -> no prompt gate.
> - No env var: only `BR_DOCTOR_RUNS_DIR`, `BR_DOCTOR_STALE_LOCK_THRESHOLD_SECS`, `BR_HISTORY_SNAPSHOT_THRESHOLD`, `BR_NO_AUTOFLUSH` — none is a consent token.
> - No alternate command name: no `br repair`/`fix`/`heal`. `br admin doctor` (AdminDoctorArgs {}) and `br admin reset --force` exist but are not doctor-confirmation.
> - Sibling modules named in the task: closest hit is src/cli/commands/orphans.rs:332, a genuine per-item `[y/N]` interactive confirm — but that is `br orphans --fix`, not doctor, so it does not cover this gap.
> 
> So the claimed local_evidence is accurate and verified, not hand-waved.
> 
> WHY I STILL REFUTE — three findings:
> 1. The `--yes` half is a functional no-op. I read GO's own logic (cmd/bd/doctor_fix.go:83-100): with `!--doctorYes`, if `term.IsTerminal(stdin)` is FALSE, GO prints "Running in non-interactive mode" + "use: bd doctor --fix --yes" and RETURNS WITHOUT MUTATING. So in CI/pipes GO's default is a refusal, and `--yes` is what unlocks automation. LOCAL's `--repair` in a pipe simply applies. LOCAL's default therefore already sits exactly at GO's `--yes` position; a `--yes` flag on `br` would change nothing. I verified LOCAL has no TTY gate at all.
> 2. There is a documented deliberate consent model, in current code, not just the finder's read. src/cli/commands/doctor_subsystems/capabilities_doctor.rs:338-341: "`auto_fixable: true` means `--repair` will attempt the fix without further prompts". src/cli/mod.rs:3692-3696: the opt-in is "load-bearing ... to preserve the doctor's 'doctor mutates only when the operator has consented' SACRED INVARIANT" — consent is the explicit opt-in flag itself, not a prompt answer. docs/AGENT_INTEGRATION.md:30 states the project-wide principle "Non-interactive - no prompts, no TUI in normal operation". The WP6 handbook at src/cli/commands/doctor_subsystems/surface.rs:203-217 tabulates the mutate surface.
> 3. The `--interactive` role is covered non-promptly: review via `--dry-run` (planned mutations to stderr, writes nothing) and `br doctor capabilities`/`explain <finding-id>`; selective application via `--only`/`--skip <fm-id>` (the declarative form of per-fix approval, respecting the chokepointed fixers); rollback via `br doctor undo <run-id>`/`latest` restoring from `.doctor/runs/<id>/backups/`. src/cli/mod.rs:3660-3680 documents --only/--skip precisely for "when an operator wants the doctor to run everything except one known-flaky path".
> 
> Also relevant: docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:6925 explicitly enumerates Go's doctor flags including `--yes` and `--interactive` and marks them "Excluded" — a recorded scope decision (stale, since `br doctor --repair` shipped later, CHANGELOG.md:593).
> 
> UPSTREAM SANITY CHECK — the claim's citations are accurate. /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/doctor.go:47-48 (`doctorYes bool`, `doctorInteractive bool // per-fix confirmation mode`) and :343-344 (`--yes`/`-y` "Skip confirmation prompt (for non-interactive use)", `--interactive`/`-i` "Confirm each fix individually") both match verbatim. cmd/bd/doctor_fix.go:78 `applyFixesInteractive`, :83-100 the `!doctorYes` + `IsTerminal` + "Continue? (Y/n): " + `reader.ReadString('\n')` path, :113-157 per-fix loop, :312 `fix.RepoFingerprint(path, doctorYes)` — all confirmed. DWS confirmed to have neither: `rg 'long = "yes"|long = "interactive"|pub yes|pub interactive' /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/` -> 0 hits, and its DoctorArgs has only repair/repair_indexes/dry_run. So `present_in: go-only` is correct.
> 
> Severity: low -> info. This is a design difference on a command whose agent-facing target audience is precisely the case GO's prompt protects against, with the --yes component already satisfied by the default and the --interactive component deliberately replaced by documented dry-run/--only/--skip/undo levers.

**Corroborator note**

> Keep as an informational note, not a low-severity gap, and rewrite the impact statement. The claim's factual description is accurate (all citations verified in shipped code, present_in=go-only confirmed by reading DWS `src/cli/mod.rs:3332-3440` and LOCAL `src/cli/mod.rs:3606-3700`, both with zero stdin reads in doctor.rs). But two things change the verdict:
> 
> (a) The claimed impact — "a targeted interactive repair session requires leaving br and hand-applying the actions --dry-run prints" — is factually wrong. `br doctor --repair --only <fm-id>` / `--skip <fm-id>` (src/cli/mod.rs:3674, :3683; implemented `FixerFilter` at src/cli/commands/doctor.rs:204-241, enforced on the rebuild path at :11685-11689) already gives targeted selective repair without leaving br. The right workflow is `br doctor --dry-run` to review, then `br doctor --repair --only <approved ids>` — review-then-apply-some, just not prompt-driven.
> 
> (b) It is a deliberate fork divergence, not an unported feature. LOCAL's whole product direction is agent-first and non-interactive (structured `--json`/`--robot` output, no TTY anywhere in doctor.rs), where a blocking prompt is an anti-pattern, and the code explicitly names the posture it protects: "the doctor's 'doctor mutates only when the operator has consented' SACRED INVARIANT" (src/cli/mod.rs:3694-3696, backing the deliberate `--unsafe-auto-fix` opt-in). Note also that GO itself has no `--only`/`--skip` (its full flag block is doctor.go:342-352), so the per-fixer selectivity LOCAL already ships is a capability the "upstream" does not have.
> 
> Separate nit, not part of this claim: docs/CLI_REFERENCE.md:1484-1497 documents only `--repair` and `--allow-repeated-repair` for `br doctor`, omitting the existing `--dry-run`, `--only`, `--skip`, `--quick`, `--robot-triage`, `--allow-warnings`, `--unsafe-auto-fix`. That is a genuine doc-coverage gap worth a one-line fix, and it likely explains why a reviewer believed the only levers were `--dry-run` and all-or-nothing.

### A17. doctor --only/--skip fixer filtering does not gate legacy repair paths

- **Domain:** `observability-ops` · **Type:** `divergent_behavior` · **Claimed severity:** medium · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: both-upstreams · corrected severity: info

**Claimed upstream evidence**

> src/cli/commands/doctor_subsystems/capabilities_doctor.rs:370-377 (FixerRow 6-tuple: id, subsystem, mutates, gated, finding ids, filter_ids); :379-427 EARLY_CHOKEPOINT_FIXER_ROWS each carrying a filter_ids entry (e.g. :386 &["fm-configs-gitignore-leaking-beads"], :394 &["fm-state_files-merge-artifact-stuck"], :402 &["fm-configs-startup-cache-poisoned"], :410 &["fm-state_files-recovery-artifacts-orphaned"], :418 &["fm-caches_indexes-export-hash-cache-divergence"]); :113-121 doc on the filter_ids field warning that an --only list disables every fixer whose ids it omits; src/cli/mod.rs:3385-3395 the DWS --only help text: "All repair paths respect the filter, including the legacy `repair_*` ones".

**Why the refuter killed it**

> REFUTED — the claimed behavioral gap does not exist in LOCAL. LOCAL already gates EVERY legacy `repair_*` path on the FixerFilter; the finder was misled by stale documentation.
> 
> WHAT I RAN AND FOUND:
> 
> 1) Gating exists at every legacy call site. `rg -n "filter_allows_jsonl_rebuild|filter_allows_recoverable_db_state_repair"` in src/cli/commands/doctor.rs returns the two helpers plus their use sites. `filter_allows_jsonl_rebuild()` (doctor.rs:1223) ORs six rebuild FMs and is checked at doctor.rs:11669, BEFORE the JSONL preflights, and early-exits with `std::process::exit(DoctorExitCode::RefusedUnsafe)` and "Refusing JSONL rebuild: filtered out by --only/--skip" — so `repair_database_from_jsonl` (doctor.rs:11727) is unreachable when filtered. `filter_allows_recoverable_db_state_repair()` (doctor.rs:1214) gates `repair_recoverable_db_state` at both doctor.rs:11535 and the warn-path at 11403. `repair_database_sidecars` is gated a second time internally at doctor.rs:2432 on "fm-state_files-wal-shm-sidecar-orphan". `repair_partial_indexes` is gated on FM_PARTIAL_INDEX_STALE; all three `repair_via_vacuum` call sites (11432, 11564, 11589) are gated on FM_SQLITE_PAGE_MALFORMED.
> 
> 2) `FixerFilter::allows` (doctor.rs:221) returns false for any FM absent from a non-empty `--only`, so with `--only fm-configs-gitignore-leaking-beads` every gate above evaluates false. The claim's own worked example is therefore false: the DB rebuild, sidecar deletion, VACUUM, and REINDEX do NOT run.
> 
> 3) The code's own comments document this explicitly. At doctor.rs:11395-11399: "Pass-5 cycle 2: legacy repair_* paths now consult the FixerFilter. AND-ing the predicate against `filter.allows(...)` makes downstream `if has_X` branches treat a filter-excluded FM as 'no finding to repair'". At 11528-11531: "Pass-5 cycle 2: gate the post-failure fallback repair_* paths on the FixerFilter."
> 
> 4) Unit tests lock it. `test_recoverable_db_state_filter_preserves_sidecar_fm` (doctor.rs:16097) and `test_jsonl_rebuild_filter_matches_addressed_fms` (doctor.rs:16131) assert both the allow and refuse directions, including `--only X --skip X`.
> 
> 5) The finder read STALE PROSE. Two locations still carry the "Pass-5 cycle 1" wording: the `--only` help at src/cli/mod.rs:3660-3669 ("legacy `repair_*` paths run unconditionally") and the comment at doctor.rs:10901-10902 ("legacy repair_* paths run unconditionally for now"). The gating landed in the immediately following pass ("Pass-5 cycle 2") and the prose was never updated. `rg -n "run unconditionally" src/` confirms these are the only two.
> 
> 6) `filter_ids` is DECLARATIVE, not the enforcement mechanism. In DWS, `rg -n "filter_ids" src/` shows it is consumed only in doctor_subsystems/surface.rs:1956,2160,2168 (capabilities-envelope discoverability) and a drift gate in doctor.rs:20444-20550 that cross-checks advertised ids against runtime gates. DWS enforces via the SAME call-site predicates: it has byte-identical `JSONL_REBUILD_FILTERED_REASON` (doctor.rs:413), `filter_allows_recoverable_db_state_repair` (1800), `filter_allows_jsonl_rebuild` (1809), the same refusal call site (14591), the same error strings, and the same tests (20558, 20601). DWS's help text even points operators at `filter_ids` as the "authoritative vocabulary" — i.e. the field is the discoverability manifest, which is exactly why its absence misled the finder.
> 
> 7) The claim's counts (12 vs 7 `fn repair_`, 49 vs 48 `filter.allows`) are drift artifacts, not evidence of missing gating — verified both counts directly.
> 
> 8) GO (gastownhall/beads) has no FixerFilter and no doctor `--only`/`--skip` at all, so this filter is Rust-only in both trees.
> 
> RESIDUAL (info, not medium): two stale help/comment lines in LOCAL, and a missing `fixers[].filter_ids` discoverability column in LOCAL's capabilities envelope. Neither changes the user-visible outcome — the allowlist already bounds what `--repair` mutates, identically to DWS.

**Corroborator note**

> The claim is REFUTED on its central behavioral assertion. LOCAL's `--only`/`--skip` DOES gate every legacy `repair_*` path; the allowlist bounds what `--repair` mutates, exactly as in DWS. I verified each destructive legacy path in LOCAL/src/cli/commands/doctor.rs is behind a FixerFilter gate: the JSONL database rebuild is explicitly REFUSED (exit RefusedUnsafe) at doctor.rs:11669 via `filter_allows_jsonl_rebuild`; sidecar deletion is gated at 2429-2433 (`filterer_filter.allows("fm-state_files-wal-shm-sidecar-orphan")`); VACUUM is gated at 11561 and 11409; partial-index repair at 11407 and 11551-11552; blocked-cache rebuild at 11405 and via `filter_allows_recoverable_db_state_repair` (1213-1218). LOCAL also carries the identical "Pass-5 cycle 2: legacy repair_* paths now consult the FixerFilter" comment (11399-11403) and two unit tests that lock the gating (16096, 16128). The upstream citations (DWS 6-tuple FixerRow with filter_ids, DWS help text, LOCAL 5-tuple, LOCAL help text) are all accurate, but they do not support the impact claim. The only genuine residue is that LOCAL's help text at src/cli/mod.rs:3668 is STALE (claims legacy repair_* run unconditionally when the code gates them) and LOCAL's capabilities envelope omits DWS's `filter_ids` field (a metadata/discoverability nicety). Both are cosmetic/maintainability issues that err toward caution, not a safety-envelope correctness gap. Severity corrected medium -> info.

### A18. Search is not Unicode case-insensitive (ASCII-only folding)

- **Domain:** `search` · **Type:** `partial` · **Claimed severity:** high · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: dws-only · corrected severity: low

**Claimed upstream evidence**

> DWS src/cli/commands/search.rs:184 `let unicode_query = !query.is_ascii();` with routing at 208-209 to search_unicode_issues -> src/storage/search.rs:29 `search_unicode_issues_unpaginated`. Regression tests in src/cli/commands/search/unicode_tests.rs (e.g. unicode_case_variants_match_fields_and_all_comment_history_in_every_format, 'CAFÉ' vs 'café'). Header comment at search.rs:184-186 states the low-level API stays ASCII-folded and non-ASCII is routed to the client matcher.

**Why the refuter killed it**

> REFUTED as framed, on a real reachable implementation found in LOCAL on the very same command using the very same mechanism DWS relies on.
> 
> WHAT I FOUND (the refutation):
> `br search` flattens `ListArgs` (`src/cli/mod.rs:2413-2419`), and `ListArgs` carries `desc_contains` (`src/cli/mod.rs:2125`) and `notes_contains` (`src/cli/mod.rs:2129`). `build_filters` pushes ONLY `title_contains` into SQL (search.rs:~474); desc/notes contains are handled purely client-side. `src/cli/commands/search.rs:543-556` builds the matcher:
>   RegexBuilder::new(&regex::escape(needle)).case_insensitive(true)
> for both desc and notes. The `regex` crate (Cargo.lock:4222 -> 1.13.1; Cargo.toml:100 `regex = "1.12"`) does Unicode simple case folding for `case_insensitive(true)` — this is the IDENTICAL mechanism DWS uses in its dedicated Unicode module. Critically it is NOT gated by the ASCII SQL pre-filter: `search.rs:118` `let use_list = query.is_empty();` routes an empty positional query to `list_issues` (no `instr(lower())` gate), then `apply_client_filters` applies the Unicode regex. So `br search --desc-contains 'café'` DOES match a stored 'CAFÉ' in LOCAL. Reachable, on the search command, feature-free.
> Further Unicode-aware search paths in LOCAL: `src/cli/commands/list.rs:748-749` (`br list --desc-contains/--notes-contains` via `str::to_lowercase`); `src/query/evaluator.rs:536-573` (`br query desc=/notes=/title=` via `to_lowercase`); `src/cli/commands/memory.rs:148` (`br memory recall` via `to_lowercase`). `src/storage/trait_.rs:314-325` is a Unicode default `search_issues` but is overridden by `sqlite.rs:4978`, so it is dead on the production backend.
> 
> WHAT SURVIVES (the narrow residual): the positional full-text query and `--title-contains` are genuinely ASCII-only. I verified `src/storage/sqlite.rs:5134` `let needle = trimmed.to_ascii_lowercase();` with `instr(lower(title), ?)` at 5132 / 5246 / 5276, and `--title-contains` uses `title LIKE ?` (ASCII-only CI). `rg -c 'search_unicode|unicode_issue_fields_match|is_ascii' src/cli/commands/search.rs` returned exit 1 / no output, and `rg -n 'to_ascii_lowercase|to_lowercase'` on that file returns only lines 694/701 (title sort). MCP `list_issues` "search" routes to the same ASCII path (`src/mcp/tools.rs:1057-1061`). So the claim's citations are accurate — the local evidence is NOT hand-waved.
> 
> WHY REFUTED ANYWAY: the filed claim is "Search is not Unicode case-insensitive (ASCII-only folding)" at severity high. LOCAL's search command does perform Unicode case-insensitive matching today, via a flag on that same command, using the same library primitive upstream added a whole module to obtain. The residual is one path (default full-text + --title-contains), not "search". Per the "ANY real implementation" bar, this is a real hit.
> 
> UPSTREAM SANITY-CHECK (plausible, and present_in=dws-only is correct): DWS `src/cli/commands/search.rs:184` `let unicode_query = !query.is_ascii();` present, with the comment "SQLite lower() folds ASCII only" above it, routing to `search_unicode_issues_default_page` / `search_unicode_issues` (208-209). `src/storage/search.rs:29` `search_unicode_issues_unpaginated` present, building the same `RegexBuilder::new(&regex::escape(query)).case_insensitive(true)`, matching via `unicode_issue_fields_match` over id/title/description PLUS all comment history. Tests at `src/cli/commands/search/unicode_tests.rs:34` `unicode_case_variants_match_fields_and_all_comment_history_in_every_format` query 'café'/'CAFÉ'/'CaFé' against 'CAFÉ' titles, and line 85 adds Greek sigma (σ/ς) and Cyrillic (я/Я) — real Unicode coverage, not a one-off. GO has no equivalent: `internal/storage/sqlbuild/filter.go:67-78` folds with Unicode-aware `strings.ToLower` but matches via SQLite `LOWER(title) LIKE ?` (ASCII-only), and there is no `instr(` anywhere in GO. GO's in-memory `internal/query/evaluator.go:854,874,893` uses `strings.ToLower` (Unicode-aware), mirroring LOCAL's `src/query/evaluator.rs` — so both LOCAL and GO have a Unicode in-memory path and an ASCII SQL full-text path; only DWS routed the default full-text query.
> 
> COMMANDS RUN (read-only, no cargo, no writes): `ls` on all three search files; `rg -c/-n 'search_unicode|unicode_issue_fields_match|is_ascii'` and `rg -n 'to_ascii_lowercase|to_lowercase'` on LOCAL search.rs; `rg -n 'instr\(|lower\(|LIKE|NOCASE' src/`; `rg -n 'fn search_issues' src/`; `sed` reads of sqlite.rs:4978-5300, search.rs:30-135 and 417-505, list.rs:675-790, trait_.rs:290-345, evaluator.rs:1-120 and 510-600, memory.rs:135-175, cli/mod.rs SearchArgs/ListArgs; `rg` for FTS5, `cfg(feature`, MCP search; `rg` over GO `filter.go` and for `instr(`/`unicode`.

**Corroborator note**

> Gap confirmed and dws-only attribution is correct, but severity should drop high -> low, and the impact example needs fixing.
> 
> Severity: this is i18n search-recall polish, not a correctness or core-workflow defect. It causes no data loss and no data integrity problem. The failure requires a narrow conjunction: a non-ASCII query AND a differing case variant of the SAME accented string. The ASCII path — the dominant case, and the only path for issue-ID lookups, since LOCAL's IDs are ASCII hash-based like beads_rust-36dk — is entirely unaffected, so agent triage workflows are not blocked. CJK is unaffected, as the claim itself notes.
> 
> Impact overstatement: the claim's example "br search cafe/café will not surface a 'CAFÉ ...' issue" is only half true. DWS's matcher is `RegexBuilder::new(&regex::escape(query)).case_insensitive(true)` (src/storage/search.rs:113-114) — Unicode simple case folding, with no diacritic stripping. DWS's own tests only ever compare same-accent variants ('café' vs 'CAFÉ', 'σ' vs 'ς', 'я' vs 'Я'). So `cafe` would not match 'CAFÉ' in DWS either; porting this would not deliver accent-insensitive search, only correct case folding. The claim also does not mention that DWS's Unicode path additionally searches full comment history, which the ASCII path in both LOCAL and GO does not.
> 
> On effort, "large" also looks high for LOCAL: the DWS implementation is a self-contained ~11KB module (src/storage/search.rs) plus a routing guard, and LOCAL's search already has the candidate/pagination scaffolding to hang it off.
> 
> No LOCAL doc explains why this was not ported, so per the calibration rules info is not warranted on that basis — but the honest grade is low, not high.

### A19. Content-hash algorithm diverges: DWS uses length-prefixed fields (EXP-101 fixed), LOCAL still uses Go-compatible NUL separators

- **Domain:** `sync-jsonl` · **Type:** `divergent_behavior` · **Claimed severity:** critical · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: dws-only · corrected severity: low

**Claimed upstream evidence**

> DWS src/util/hash.rs:3-7 '//! Uses SHA-256 over stable ordered fields with length-prefixed serialization. //! Each field is encoded as an unsigned 64-bit little-endian byte length followed by the raw UTF-8 bytes. This prevents the EXP-101 NUL-injection field-boundary collision and intentionally breaks the old Go bd NUL-separated `content_hash` byte format; schema v14 rebuilds stored hashes.' Encoding at src/util/hash.rs:179-183 (field() writes field_len_prefix then bytes) and src/util/hash.rs:198-204 (field_len_prefix -> 8-byte LE). Regression test at src/util/hash.rs:288 `test_content_hash_distinguishes_embedded_nul_boundaries` hashes ("x", Some("y\0z"), ...) vs ("x\0y", Some("z"), ...) and asserts they differ. DWS field set at src/util/hash.rs:77-99 (adds issue.prerequisites, no metadata/bonded_from/await_*/waiters/mol_type/work_type/event_*). DWS CHANGELOG.md:2658 'Hash collision vulnerability fix and dep tree fix (458a77b)'. DWS is v0.6.0 (Cargo.toml:3).

**Why the refuter killed it**

> The literal algorithm divergence is real and I confirmed it by reading code, but BOTH user-visible consequences the claim rests on are refuted, so the gap as characterized (critical, collision-exploitable, cross-fork dedup broken) does not stand.
> 
> WHAT I RAN AND FOUND:
> 
> 1) Literal divergence CONFIRMED. LOCAL src/util/hash.rs:276-279 `field()` = `hasher.update(value.as_bytes()); hasher.update(b"\x00")`; :287-292 `field_strptr` likewise. Doc :3-5 pins Go parity, and tests/conformance.rs:398 `conformance_content_hash_matches_go_bd_fixture` enforces it. No length-prefix in the content-hash path: `rg 'length-prefix|field_len_prefix|EXP-101|u64::to_le_bytes'` over src/ returns only src/util/id.rs:260 (`generate_id_seed` uses ASCII `len:value`) and src/sync/witness.rs:1338 (Merkle witness `hash_field`) — both are different subsystems, not content_hash. I also searched for a feature gate, flag, env var, config key, MCP route, or sibling-module equivalent (`dual.?hash|legacy.?hash|hash_version|rehash|migrate.*hash`) — 0 hits. So there is genuinely no length-prefixed content hash in LOCAL. GO side verified NUL-separated at gastownhall_beads/internal/types/types.go:259-262.
> 
> 2) CONSEQUENCE (1) — EXP-101 collision — REFUTED as unreachable. src/validation/mod.rs:192-194 `reject_nul` blocks NUL in every content-hash input that is user-settable: title, description, design, acceptance_criteria, notes, status, issue_type, assignee, owner, created_by, external_ref, source_system. Pinned by test `issue_validation_rejects_nul_in_content_hash_fields` (validation/mod.rs:643-678). Validation is enforced on every write path: CLI create (create.rs:503, 1074), import (sync/mod.rs:4042, 4073 — runs BEFORE `normalize_issue` hashes), preflight (sync/mod.rs:827), MCP (mcp/tools.rs:1644). The hash fields that are NOT NUL-validated (payload, target, event_kind, waiters, bonded_from, await_type, await_id) are hardcoded to None/vec![] at create.rs:469-493 and are not CLI-settable. Decisive: LOCAL's own fuzz target fuzz/fuzz_targets/content_hash.rs:30-32 early-returns on any issue failing validation before asserting any hash invariant, and its `assert_nul_space_equivalence` (line 238) encodes the assumption that NUL never reaches the hasher. The claimed colliding pair (title 'x'/'x\0y') cannot be constructed through any user-facing path.
> 
> 3) CONSEQUENCE (2) — cross-fork dedup — REFUTED. sync/mod.rs:4002-4004, inside `normalize_issue` (defined at :3900), unconditionally recomputes `issue.content_hash = Some(content_hash(issue))` using LOCAL's own algorithm after all import repairs, with the comment "Recompute after all import repairs so the stored row hash matches the canonical issue state used by collision detection and export hashes." So a DWS-authored issues.jsonl does NOT carry DWS hashes into LOCAL — they are normalized on import. No `--trust-hashes` escape hatch exists (`rg 'trust.hash|preserve.hash|use_existing_hash'` = 0 hits). Further, `detect_collision` (sync/mod.rs:3808-3846) matches external_ref in Phase 1 and ID in Phase 2 BEFORE content hash in Phase 3, so cross-fork dedup keys on stable identity, not on hash bytes.
> 
> 4) DWS ships the SAME NUL rejection (dicklesworthstone_beads_rust/src/validation/mod.rs:238-240, with the same `reject_nul` helper across title/description/design/acceptance_criteria/prerequisites/notes/status/issue_type/external_ref, plus comment body at :477). DWS's length-prefix is therefore defense-in-depth layered on top of an identical NUL-rejection control — not a fix for a collision that is reachable there either.
> 
> Upstream evidence is plausible and non-fabricated: I read DWS hash.rs :1-7 (the EXP-101 docstring), :77-99 (prerequisites field, no metadata/bonded_from/await_*/waiters/mol_type/work_type/event_*), :179-183 `field()` writing field_len_prefix, :198-204 `field_len_prefix` → 8-byte LE, and the `test_content_hash_distinguishes_embedded_nul_boundaries` test. All citations check out.
> 
> Net: LOCAL lacks the length-prefix hardening (a real interop nit), but the critical premise — that a NUL-boundary collision is reachable and that the collision detector silently merges distinct issues — is blocked by validation on every write path, and cross-fork dedup survives via import-time recompute plus ID/external_ref-first matching.

**Corroborator note**

> Severity should be low (defense-in-depth nit), not critical. The cited DWS capability is real and accurately quoted, but (a) the exact collision the claim names is impossible in LOCAL — `src/validation/mod.rs:192-196` `reject_nul` hard-rejects NUL in title/description/notes/assignee/etc., and that validator runs on import at `src/sync/mod.rs:4042`/`:4073`, preflight at `:827`, and create at `src/storage/sqlite.rs:2398`, so the import aborts with a loud error instead of silently collapsing; (b) DWS has the same NUL rejection in its own `src/validation/mod.rs:169-195`, so this is defense-in-depth, not a fix for an exploitable path in DWS; (c) the remaining surface (spec_id, metadata, event_kind, target, payload, waiters, await_*, bonded_from) has no NUL validation but is also not settable from any CLI flag, is only reachable by hand-crafting JSONL, and even a collision resolves to a counted Update/Skip reported in `ImportResult` (`src/cli/commands/sync.rs:2495-2520`) — not silent loss; (d) the cross-fork argument collapses because `content_hash` is `#[serde(skip)]` in both repos and recomputed on import, so it is absent from issues.jsonl entirely. LOCAL's NUL scheme is a deliberate, documented Go-parity choice (`src/util/hash.rs:3-5`) that matches the still-NUL-based Go upstream at `gastownhall/beads internal/types/types.go:198-247`; adopting DWS's scheme would break parity with the actual Go tool that LOCAL is written to interop with, and per AGENTS.md ("Go parity", "Schema compatibility") is the wrong direction for this project. Worth an eventual bead to align content_hash with DWS only if the two forks are ever declared merge-compatible, and to close the reject_nul gap on spec_id/metadata/event_kind/target/payload/waiters regardless of the hash decision.

### A20. DWS relocates one side of a convergent child-ID collision during 3-way merge; LOCAL silently drops the loser

- **Domain:** `sync-jsonl` · **Type:** `missing_feature` · **Claimed severity:** high · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: dws-only · corrected severity: low

**Claimed upstream evidence**

> DWS src/sync/mod.rs:16016 `pub id_collisions: Vec<IdCollision>` on MergeReport, doc 'Issue ids that two clones minted independently for different issues. Both issues are kept; see IdCollision (GitHub #512).'; struct at src/sync/mod.rs:16030-16051; hook at src/sync/mod.rs:16288-16304 (three_way_merge calls plan_id_collisions and assigns report.id_collisions); planner at src/sync/mod.rs:16804-16885 (detects `left.created_at != right.created_at` for ids present in both sides; on failure returns 'Never let a strategy pick one of two issues: leave every contested id to the user'); id minting at src/sync/mod.rs:16890 mint_relocated_issue_id ('the next child number under the same parent for <parent>.N, or a new hash id with the same prefix and length for a root id'). Tests: src/sync/mod.rs:27157, 27208, 27264, 27305, 27393.

**Why the refuter killed it**

> I searched LOCAL hard and could not find any relocation mechanism — but the claim's decisive harm assertion is factually wrong, and LOCAL ships a documented, deliberate equivalent safety guard, so the gap as framed ("LOCAL silently drops the loser by default") is refuted.
> 
> WHAT I CONFIRMED (the mechanism half of the claim is true):
> - rg -c 'relocated_id|id_collisions|already_relocated' src tests -> 0. No plan_id_collisions, mint_relocated_issue_id, or rekey/mint/free-id helper anywhere in src (rg 'fn .*(mint|relocat|rekey|free_id|new_id_for)' -> 0 hits).
> - LOCAL three_way_merge (src/sync/mod.rs:5112-5171) has no pre-pass; it iterates all_issue_ids() and calls merge_issue per id. MergeReport (src/sync/mod.rs:4876-4888) has kept/deleted/conflicts/tombstone_protected/notes and no id_collisions. merge_issue case 7 (src/sync/mod.rs:5058-5092) hands the convergent same-id row to the strategy and never keeps both. So the RELOCATION capability is genuinely absent from LOCAL, and present_in is dws-only (GO has no equivalent either; my GO grep for IdCollision/plan_id_collisions found nothing in the merge path).
> - No feature gates in sync/mod.rs (cfg(feature -> 0 hits) and no MCP merge/import surface that could hide it. The import-path collision code (detect_collision 4-phase, scan_import_collision_renames, apply_collision_renames, src/sync/mod.rs:3767-4374) is a different, additive/last-write-wins path for dedup/ext_ref/id match — it is NOT an equivalent to keeping-both-by-relocation, so I do not count it as a mitigation.
> 
> WHY THE HARM CLAIM IS REFUTED (the decisive part): The claim says "Default strategy is PreferLocal (src/sync/mod.rs:4916-4927, #[default] PreferLocal), so the data-losing side is the default" and that the losing issue is "silently" dropped. This is wrong for the user-facing path. The CLI never uses the enum's #[default]; merge_conflict_resolution (src/cli/commands/sync.rs:382-391) hardcodes the effective default to Manual: force_db->PreferLocal, force_jsonl->PreferExternal, force->PreferNewer, else->Manual. With Manual, merge_issue case 7 returns Conflict(ConvergentCreation), and the CLI then HARD-STOPS the merge (src/cli/commands/sync.rs:2700-2710, returns Err "Merge conflicts detected"). Neither issue is dropped by default; the user must explicitly opt in to dropping a side. There is even a regression test locking this in: test_merge_conflict_resolution_defaults_to_manual (src/cli/commands/sync.rs:3205-3212) asserts default -> ConflictResolution::Manual. So the claim misread the enum's #[default] attribute (an unused default for the library API) as the CLI's behavior.
> 
> DOCUMENTED DELIBERATE EQUIVALENT: LOCAL documents convergent same-ID creation as a first-class merge guard, not a silent auto-pick:
> - docs/SYNC_SAFETY.md:76 — Merge Guards table: "Convergent creation conflict | prevents: Silently choosing between independently created same-ID issues | override: --force, --force-db, --force-jsonl".
> - docs/CLI_REFERENCE.md:1153 — "Without an explicit conflict policy, semantic conflicts stop the command. This covers both-modified, delete-vs-modify, and convergent same-ID creation conflicts."
> So LOCAL achieves the same user-visible outcome (you are never silently forced to choose between two same-id issues) by a stop-and-ask guard + explicit-opt-in override rather than DWS's relocate-and-keep-both. Even when a user does opt in (--force-db/--force-jsonl/--force), the decision is recorded as a note AND persisted as an issue comment ("Add merge notes as comments", src/cli/commands/sync.rs ~2745-2750) and surfaced in JSON output, so it is not a silent trace-free loss.
> 
> CONCLUSION: the relocation FEATURE is truly absent (dws-only), but the claimed "silent default data loss / default is PreferLocal" harm is not real — LOCAL's default stops the merge and a documented guard deliberately prevents the silent pick. The local_evidence's central claim is therefore hand-waved/incorrect on the severity-driving fact, so per the refute rules the gap as framed is refuted and downgraded to low.

**Corroborator note**

> The upstream feature is real and correctly attributed to DWS-only, so this is not refuted as a mischaracterised capability — but the claim's stated impact is substantially wrong and the severity must drop from high to low. Three corrections: (1) LOCAL's default merge strategy is Manual, not PreferLocal (src/cli/commands/sync.rs:390, asserted by the test at :3211); PreferLocal only applies with an explicit --force-db. (2) LOCAL does NOT silently drop the loser and does NOT report success: on the default path the merge returns Err at src/cli/commands/sync.rs:2707-2719 before applying any DB change, listing the contested id and naming the three override flags. (3) "There is no flag to change this and no record of the loss" is false — the overrides exist and the exact scenario is already a documented Merge Guard at docs/SYNC_SAFETY.md:76 ("Convergent creation conflict | Silently choosing between independently created same-ID issues | --force, --force-db, --force-jsonl"), restated at docs/CLI_REFERENCE.md:1153. Two further overstatements: because JSONL is exported sorted by id (src/sync/mod.rs:1679, :1694) a genuine two-clone same-child-id collision surfaces as a git text conflict on issues.jsonl before br's merge is ever reached, and a file that did end up with two rows of one id is rejected by the loader with "Duplicate issue id ... at line N" (src/sync/mod.rs:1517/1662/3164/3309/3754/4155). The "same root id" variant is also weak, since LOCAL root ids are content hashes (src/util/id.rs:263) so distinct issues effectively cannot collide. What genuinely remains is that DWS auto-resolves convergently while LOCAL stops and requires a human decision — real automation friction for an agent-first tool, but not silent data loss, so low rather than high. Note also that DWS shares LOCAL's Manual-by-default design (DWS sync.rs:1015-1025) and only falls back to reporting ConvergentCreation conflicts when no relocation can be planned, so this is an additive robustness layer on identical philosophy, not a fix for a divergent default.

### A21. DWS `sync --import-only --skip-invalid-records` JSONL salvage with a rejection receipt has no LOCAL equivalent

- **Domain:** `sync-jsonl` · **Type:** `missing_flag` · **Claimed severity:** medium · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: dws-only · corrected severity: low

**Claimed upstream evidence**

> DWS src/cli/mod.rs:2978-2985 `--skip-invalid-records` ('Only valid with --import-only. Before replacing issues.jsonl, br keeps an exact non-rotating backup under .beads/.br_history, reports every rejected line, and validates the remaining generation. Merge-conflict markers are never skipped.'). src/sync/mod.rs:9711 JsonlSalvageRejectedRecord, :9718 JsonlSalvageReceipt, :9770 salvage_invalid_jsonl_records_under_authority, :9858 receipt construction. Command-layer validation src/cli/commands/sync.rs:807-819 ('--skip-invalid-records is an additive recovery mode and cannot be combined with --force, --rebuild, or --rename-prefix') and :797 (it forces the reviewed non-rebuild path).

**Why the refuter killed it**

> I searched hard and can confirm the literal flag is absent from LOCAL — but the claim's load-bearing harm narrative is factually wrong, and the absence is a documented deliberate design invariant, not a gap.
> 
> WHAT I RAN (LOCAL = /Users/tranquangdang21/Projects/beads_rust):
> 1. `rg -c -i 'skip_invalid_records|skip-invalid-records|JsonlSalvage|salvage_jsonl|SalvageReview|backup_before_jsonl_salvage|pre-salvage' src tests` -> 0 hits.
> 2. `rg -n -i 'salvage|skip_invalid|skip-invalid|rejection.receipt|rejected_record|invalid_record' src tests` -> only one unrelated hit (doctor_subsystems/surface.rs:1290, an SQL identifier string).
> 3. Read the full `SyncArgs` struct (src/cli/mod.rs:3330-3462). Flags are: flush_only, import_only, merge, status, witness, witness_chunk_lines, witness_parallelism, export_parallelism, force, force_db, force_jsonl, allow_external_jsonl, manifest, error_policy, orphans, rename_prefix, rebuild, robot. No salvage flag.
> 4. Checked every escape hatch the task listed: config keys / env vars (`rg -i 'BR_SKIP|SKIP_INVALID|salvage' src/config/` -> 0), feature gates (`rg 'cfg\(feature' src/sync/mod.rs` -> none), MCP surface (`rg -i 'skip.invalid|salvage' src/mcp/` -> 0), doctor_subsystems (mutate.rs, refuse_gates.rs, surface.rs -> 0), lint.rs, CHANGELOG.md, UPGRADE_LOG.md, docs/.
> 
> So the capability is genuinely NOT implemented in LOCAL. The import path is fail-whole exactly as claimed (src/sync/mod.rs:3749 in read_issues_from_jsonl; import_from_jsonl at :4568 aborts via `?`).
> 
> WHY I REFUTE ANYWAY — the claim's local_evidence is affirmatively contradicted on its central point:
> 
> The claim asserts: "the operator's only option is `br sync --rebuild`, which physically deletes every DB row not present in the (broken) JSONL." That is FALSE. `--rebuild` is hard-guarded against a malformed JSONL on every path I could find:
> - Delegated path: sync.rs:436 `maybe_delegate_rebuild` -> config/mod.rs:2577 `recover_database_from_jsonl` -> config/mod.rs:1306 `repair_database_from_jsonl` -> config/mod.rs:1353 `preflight_import(jsonl_path, &preflight_config, Some(&prefix))?.into_result()?`. `into_result()` (sync/mod.rs:754) converts any Fail into `Err`, and Check 5 (sync/mod.rs:1258) Fails on `invalid_count > 0`.
> - Doctor repair path: doctor.rs:2266 `preflight_jsonl_rebuild_authority` refuses with `JSONL_REBUILD_AUTHORITY_ERROR_PREFIX` (doctor.rs:295) on both conflict markers AND invalid records.
> - In-place path: sync.rs:2230-2240 runs `ensure_no_conflict_markers` + `get_issue_ids_from_jsonl` BEFORE `reset_data_tables()` (sync.rs:2292), and the ID reader itself fail-closes (sync/mod.rs:1512).
> This is test-asserted, not incidental: doctor.rs tests `test_repair_database_from_jsonl_refuses_duplicate_ids_without_backup` assert "original DB should remain untouched after refused repair" and "JSONL authority preflight failures should not create recovery backups."
> 
> LOCAL also has a PARTIAL equivalent for the reporting half: `validate_jsonl_issue_records` (src/sync/mod.rs:805) collects every invalid line into `JsonlIssueValidationSummary { record_count, invalid_count, failures:[{line, message}] }`, surfaced by `br doctor --json` as `invalid_lines` + `invalid_examples` (doctor.rs:9147) and by the `json_valid` preflight check. So "reports every rejected line with line numbers" is substantially present (capped at a 10-line preview, JSONL_VALIDATION_PREVIEW_LIMIT).
> 
> DECISIVELY, the absence is a written architectural invariant, not an oversight:
> - docs/ARCHITECTURE.md:626 (Invariant Matrix) lists as FORBIDDEN SILENT BEHAVIOR for `.beads/issues.jsonl`: "Best-effort partial import of malformed or conflicted JSONL"; allowed action is "Reject import and preserve the file for manual repair."
> - docs/ARCHITECTURE.md Primary-Data Repair Rules #4: "malformed JSONL ... promote the workspace to `quarantined` for import/rebuild purposes."
> - docs/SYNC_SAFETY.md:67 — Schema validation guard override: "**None** - must fix JSONL".
> - docs/TROUBLESHOOTING.md:81 — desired response: "Refuse import, preserve the original file, and require line-level repair rather than best-effort partial mutation."
> 
> UPSTREAM SANITY CHECK: present_in=dws-only is correct. DWS has it (src/cli/mod.rs:2985 `pub skip_invalid_records: bool`; src/sync/mod.rs:9711 JsonlSalvageRejectedRecord, :9718 JsonlSalvageReceipt, :9770 salvage_invalid_jsonl_records_under_authority; src/cli/commands/sync.rs:797/807 command-layer gating; src/sync/history.rs:640 backup_before_jsonl_salvage). Citations in the claim are accurate. GO (gastownhall/beads) has 0 hits for skip_invalid_records/JsonlSalvage across all .go files, confirming dws-only.

**Corroborator note**

> Capability confirmed shipped in DWS (not a proposal), and present_in=dws-only is correct — Go has no salvage at all. But the severity rationale collapses: the claim's central "destructive escape hatch" premise is factually wrong. In LOCAL src/cli/commands/sync.rs the JSONL parse (get_issue_ids_from_jsonl at :2237) fails ~55 lines BEFORE the destructive reset_data_tables() at :2292, so `br sync --rebuild` on a broken JSONL aborts with "Invalid JSON at line N" and deletes nothing. LOCAL additionally validates the full import plan before any storage mutation (src/sync/mod.rs:4586-4590) and has a regression test pinning the fail-closed behaviour (tests/e2e_sync_fuzz_edge_cases.rs:156-180). The genuine but much smaller gap is ergonomic: no first-party in-tool line quarantine with a typed receipt, leaving the operator to use git (JSONL is git-tracked) or jq. That is worth medium-to-low effort to port, but it is a convenience/recovery-UX gap, not a data-integrity one, so severity drops medium -> low.

### A22. No `cargo check --all-targets` or `cargo clippy --all-targets` gate — integration test files are never even compiled in CI

- **Domain:** `testing-parity-docs` · **Type:** `missing_feature` · **Claimed severity:** critical · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: both-upstreams · corrected severity: medium

**Claimed upstream evidence**

> DWS `.github/workflows/ci.yml:47` installs `components: rustfmt, clippy`; `:56` `cargo clippy --all-targets --all-features -- -D warnings`; `:59` `cargo clippy --all-targets --no-default-features -- -D warnings`; `:62` `cargo check --all-targets --all-features`. DWS `.github/workflows/release.yml:120` comment "A release must not ship with a red unit suite or clippy", `:125` fmt check, `:129` clippy all-targets all-features `-D warnings`.

**Why the refuter killed it**

> REFUTED as filed ("missing_feature", "dws-only", "critical"). The capability exists in LOCAL in full — only its enforcement layer differs.
> 
> WHAT I RAN:
> - `ls .github/workflows/` + `find .github -type f`: exactly 3 workflows (ci.yml, release.yml, audit.yml) + dependabot.yml + 2 pin inventories. No reusable workflows, no `.github/actions/`.
> - `grep -rnE 'ci-local|all-targets|clippy|cargo check|cargo test' .github/` -> ONE line: `ci.yml:35 cargo test --lib --all-features`. So the claim's local evidence is accurate/verified, not hand-waved.
> - Checked for alternative CI/build systems (Makefile, Justfile, Taskfile, .pre-commit-config.yaml, .gitlab-ci.yml, .buildkite, .drone, appveyor, azure-pipelines): none exist. `git config core.hooksPath` unset; `.git/hooks` holds only stock `.sample` files. So no hidden runner.
> 
> WHY REFUTED — THE GATE EXISTS IN LOCAL VERBATIM:
> - `/Users/tranquangdang21/Projects/beads_rust/scripts/ci-local.sh` (checked in, mode 0755, actively maintained by commit 4b4ab5a5 "ci: gate releases on reliability suites (beads_rust-rsei.5.1)") runs the EXACT four gates, in the SAME ORDER as DWS ci.yml, including the very second clippy pass the claim presents as DWS-distinctive:
>   - L46 `cargo fmt --all -- -> --check`
>   - L49 `cargo clippy --all-targets --all-features -- -D warnings`
>   - L52 `cargo clippy --all-targets --no-default-features -- -D warnings`
>   - L55 `cargo check --all-targets --all-features`
>   - plus L58 `cargo test --all-features`, L61 `cargo test --no-default-features`, L64 `cargo test --doc`, and 4 reliability gates using `cargo test --test <name>`.
>   Header comment: "Run CI checks locally before pushing. Mirrors .github/workflows/ci.yml steps."
> 
> - DOCUMENTED AS MANDATORY (the "equivalent capability by a different mechanism" case):
>   - docs/TEST_HARNESS.md:38 — table row: `scripts/ci-local.sh` | "Full CI simulation" | 2-5min | "Before pushing"; :484 lists it under "Local Development Workflow -> Before Pushing".
>   - docs/TESTING_GUIDELINES.md:121 — `cargo clippy --all-targets -- -D warnings` under a "## CI gates" heading.
>   - docs/CI_SUPPLY_CHAIN.md:126 — "Whole-crate `cargo check --all-targets` and `cargo clippy --all-targets -- -D warnings` only when Rust code changed; run them through RCH for agent sessions."
>   - AGENTS.md "Compiler Checks (CRITICAL)" mandates all three commands; its supply-chain section restates the whole-crate `--all-targets` requirement. This file governs every agent session in this repo.
> 
> - DELIBERATELY REMOVED FROM THE HOSTED RUNNER WITH A STATED RATIONALE (the "deliberately removed" case I was told to check):
>   - Commit 273daa2b (2026-07-05) "fix(ci): remove clippy (200+ nightly lint noise), use fmt + test gates" deleted `- run: cargo clippy --all-targets --all-features -- -D warnings` and dropped the `clippy` toolchain component from ci.yml.
>   - `git log -p --follow .github/workflows/ci.yml` proves LOCAL's ci.yml PREVIOUSLY carried `cargo check --all-targets --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, AND `cargo clippy --all-targets --no-default-features -- -D warnings`. This is an implemented-then-reasoned decision, not an unimplemented idea.
>   - CHANGELOG.md "Validation" sections record these gates passing: "Passed `cargo check --all-targets --all-features`" / "Passed `cargo clippy --all-targets --all-features -- -D warnings`" / "Passed `cargo fmt --check`".
> 
> - `scripts/build-web.sh` (the only other step CI runs) is a Next.js static export; it invokes no cargo test/clippy target. Confirms nothing else in CI compiles integration tests.
> 
> UPSTREAM EVIDENCE SANITY CHECK (plausible, verified): /tmp/beads_gap_audit/dicklesworthstone_beads_rust/.github/workflows/ci.yml ~L43-62 has `components: rustfmt, clippy`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo clippy --all-targets --no-default-features -- -D warnings`, `cargo check --all-targets --all-features` — all as quoted. release.yml L120 carries the quoted comment ("A release must not ship with a red unit suite or clippy: the v0.5.x line shipped eleven lib-test failures...") with fmt check L125 and `cargo clippy --locked --all-targets --all-features -- -D warnings` L129. Citations accurate.
> 
> MINOR FACTUAL CORRECTION: the claim's "155 integration test binaries" is overstated — actual count is 133 `tests/*.rs` cargo targets (142 .rs files under tests/ excluding tests/common/).

**Corroborator note**

> Gap is real and every citation checks out, but the claim is mis-scoped and over-severitied on three points. (1) present_in is wrong: the exact cargo flags are dws-only, but the capability is present in BOTH upstreams — gastownhall/beads compiles every test package via "go test … ./..." (main.yml:583, pr.yml:973) and gates lint via gofmt (pr.yml:1042) + golangci-lint (pr.yml:1076-1083). LOCAL is the only one of the three that neither compiles its integration tests nor runs a strict lint gate. (2) "155 integration test binaries" is inflated: the real count of Cargo auto-discovered targets is 133 (top-level tests/*.rs). The other 22 .rs files are helper modules in subdirectories (tests/snapshots/, tests/common/, tests/e2e/, …) and there is no tests/*/main.rs, so they are not test binaries. (3) Severity "critical" -> medium: this is CI/process infrastructure with zero impact on the shipped binary, no data-integrity or data-loss risk, and no effect on core agent workflows. LOCAL does have a documented alternative (AGENTS.md mandates all three commands; docs/CI_SUPPLY_CHAIN.md:126 routes check/clippy through the 8-worker RCH fleet for agent sessions) — weaker than a CI gate, since an outside PR bypasses it, but deliberate rather than an oversight. Retained substance: LOCAL installs no clippy component at all (ci.yml:26), so the deny-level pedantic/nursery contract in Cargo.toml:186-188 never executes in CI, and 133 integration tests are neither built nor run. Cheap fix: add "components: clippy" and the two --all-targets lines; or minimally switch ci.yml:35 to "cargo test --all-features".

### A23. UPGRADE_LOG is stale (last entry 2026-05-14) and duplicated in two divergent files; DWS records 1,133 lines of migration work LOCAL never logged

- **Domain:** `testing-parity-docs` · **Type:** `missing_docs` · **Claimed severity:** low · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: dws-only · corrected severity: info

**Claimed upstream evidence**

> DWS `wc -l UPGRADE_LOG.md` = 1,133; `ls -la UPGRADE_LOG.md` = 100K. Headings via `grep -nE '^#{1,3} ' UPGRADE_LOG.md`: `:3` "September 18: the `otrgz` pending-merge-inspection reproducer closes on 0.4.4 — COMPLETE", `:32` "September 17: FrankenSQLite 0.4.4 uniform family + stable catch-ups (`beads_rust-0edxa`) — COMPLETE", `:91` "In progress: September 16 published engine qualification", `:210` "Blocked on native capacity: current-source Windows qualification", `:503` "Completed: 2026-09-15 CLI dependency patches", `:542` "In progress: 2026-09-15 MCP startup admission", `:627` "Completed security patch: 2026-09-15", `:1035` "fastmcp-rust (optional `mcp` feature): =0.7.1 → =0.8.1", `:1042` "fsqlite family (15 manifest entries, 20 crates in the lock): 0.3.15 → 0.3.16", `:1075` "fsqlite stack: 0.1.18/0.1.19 → 0.3.1 (with asupersync 0.4.4)". DWS also keeps `CHANGELOG.md` (179K) and `CHANGELOG_RESEARCH.md` (105K), the latter a category LOCAL has no analogue for.

**Why the refuter killed it**

> I searched LOCAL exhaustively for any implementation, alias, or documented deliberate equivalent of a maintained dependency-upgrade record, and found none — but the gap as claimed is materially misdescribed and rests on two false evidence points, so I discard it as stated.
> 
> REFUTATION 1 — the duplication is NOT a LOCAL-vs-DWS differential. I ran `diff /Users/tranquangdang21/Projects/beads_rust/docs/operations/UPGRADE_LOG.md /tmp/beads_gap_audit/dicklesworthstone_beads_rust/docs/operations/UPGRADE_LOG.md` (output: identical, no differences) and `md5 -q` on both, which returns the SAME hash 8f4a48a8925723b11a163a913d5b9f81 for both. Both files are 188 lines / 7.9K / dated 2026-01-18. DWS carries the exact same stale secondary copy. A defect present byte-for-byte in both upstreams is not a gap LOCAL has and DWS lacks; the claim's framing ("a second, older and differently-formatted copy lives at docs/operations/") presents a shared inherited artifact as a LOCAL peculiarity.
> 
> REFUTATION 2 — the local_evidence is partly false. I ran `wc -l UPGRADE_LOG.md` in LOCAL: it is 88 lines, not the claimed "ends around line 40". I ran `ls -la` on all three CHANGELOG.md files: LOCAL 68K, DWS 179K, GO 546K — so LOCAL's CHANGELOG is the SMALLEST of the three, directly contradicting the claim's "the largest changelog of the three audited for this domain's purposes only in the sense of size." These are load-bearing, independently checkable citations in the claimed evidence, and both fail.
> 
> REFUTATION 3 — "1,133 lines of migration work LOCAL never logged" is largely a category error. I checked both manifests: DWS Cargo.toml pins fsqlite 0.4.4, toon_rust 0.2.4, fastmcp-rust =0.10.0; LOCAL Cargo.toml pins fsqlite 0.1.7, toon_rust 0.2.3, fastmcp-rust 0.3.1, and LOCAL's Cargo.lock resolves fsqlite 0.1.19 / asupersync 0.3.10. The specific items the claim enumerates (fsqlite 0.1.18/0.1.19 -> 0.3.1 -> 0.4.4, toon_rust 0.2.3 -> 0.2.4, fastmcp 0.7.1 -> 0.8.1, Windows/macOS native-capacity qualification of the 0.4.x engine) are migrations on a different dependency lineage that LOCAL never performed and has no obligation to record. Counting them as "unlogged work" inflates the gap.
> 
> WHAT I VERIFIED AS TRUE (the narrow residue): LOCAL's root UPGRADE_LOG.md is genuinely stale. I read all 88 lines; the header reads "**Date:** 2026-05-14" and the tail reads "Prepared beads_rust v0.2.10." Meanwhile `git log --oneline -5 -- UPGRADE_LOG.md` shows it was last touched by release(v0.2.10) commits, and LOCAL's Cargo.lock has since moved to fsqlite 0.1.19 / asupersync 0.3.10 — a step the log never records (it documents only 0.1.3/0.1.4). LOCAL's CHANGELOG.md (top section v0.2.0, 2026-07-28) records the fsqlite 0.1.3/0.1.4 update at line 245 but has no entry for 0.1.7 -> 0.1.19. I also confirmed there is no canonical-declaration anywhere: `grep -rnE 'UPGRADE_LOG'` across all .md/.toml/.rs/.yml (excluding target/) returned ZERO hits, so nothing in LOCAL tells a reader which log is authoritative. LOCAL also has no analogue to DWS's CHANGELOG_RESEARCH.md (105K).
> 
> UPSTREAM EVIDENCE PLAUSIBILITY: I confirmed DWS's cited figures directly — `wc -l` = 1133, `ls -la` = 100K, and `grep -nE '^#{1,3} ' UPGRADE_LOG.md` reproduced the claimed headings at :3, :32, :91, :210, :503, :542, :627, including the "In progress:" and "Blocked on native capacity:" state-machine markers. GO (gastownhall_beads) has no UPGRADE_LOG at all (only scripts/upgrade-smoke-test.sh and cmd/bd/upgrade*.go), so the maintained-log capability is genuinely dws-only.
> 
> NET: no implementation or documented equivalent exists in LOCAL (I checked docs/agent/AGENT_FRIENDLY_CHANGELOG.md, .github/dependabot.yml, docs/operations/, and the full docs tree as alternative homes), so I do not claim the residue is fabricated. But the claim's headline is half shared duplication plus a lineage mismatch plus two false data points, which per the discard criteria makes it unverified as stated. The surviving real observation — "LOCAL's root upgrade log went quiet after 2026-05-14 while its lockfile advanced to fsqlite 0.1.19" — is a documentation-hygiene nit, not a 1,133-line missing record.

**Corroborator note**

> Two of the three headline sub-claims are wrong, and the surviving gap is ~1 dependency bump.
> 
> (1) The "duplicated in two divergent files" problem is not LOCAL's. LOCAL docs/operations/UPGRADE_LOG.md and DWS docs/operations/UPGRADE_LOG.md are byte-for-byte identical (md5 8f4a48a8925723b11a163a913d5b9f81). DWS ships the same stale 2026-01-18 copy with the same missing supersession marker. Shared inherited hygiene, not a LOCAL-vs-DWS gap.
> 
> (2) "The exact information AGENTS.md tells agents to consult" is false. grep UPGRADE_LOG over AGENTS.md/README.md returns zero matches in LOCAL, DWS, and GO alike.
> 
> (3) "1,133 lines of migration work LOCAL never logged" is fork divergence, not a missed record. LOCAL is on fsqlite 0.1.7; DWS is on fsqlite 0.4.4 + asupersync 0.5.0. DWS's log opens at 0.1.18/0.1.19 and never mentions 0.1.7. LOCAL never ran that migration. Much of the 1,133 lines is DWS's own swarm journal (RCH log paths, engine-publication byte-comparison receipts, native-capacity waits, DWS-only bead IDs) - a project-specific scratchpad, not a portable pattern.
> 
> What actually survives: commit 2280f1ff (2026-06-05) "deps: bump fsqlite 0.1.3 -> 0.1.7 (#316 - resolves the MVCC #106 BusySnapshot)" is recorded in neither UPGRADE_LOG.md nor CHANGELOG.md, only in the commit body. One bump, rationale preserved in git, no product impact. LOCAL's CHANGELOG.md (1,011 lines) already has a "### Dependency Updates" section at line 243; GO uses a 546K CHANGELOG.md and has no UPGRADE_LOG.md at all.
> 
> Accurate restatement: "LOCAL's UPGRADE_LOG.md has not been updated since 2026-05-14, so the 2026-06 fsqlite 0.1.3->0.1.7 bump is documented only in its commit message rather than in UPGRADE_LOG.md or CHANGELOG.md. Cosmetic doc freshness; the same stale duplicate exists in DWS."

### A24. Five of DWS's eight CI workflows have no LOCAL counterpart — doctor health gate, scheduled full-E2E + benchmarks, compaction-certificate validation, package-manifest update, notification wiring

- **Domain:** `testing-parity-docs` · **Type:** `missing_feature` · **Claimed severity:** medium · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: dws-only · corrected severity: low

**Claimed upstream evidence**

> DWS `.github/workflows/` contains 8 files vs LOCAL's 3. `doctor.yml:36-154` job `doctor-health` with steps at :54 "Doctor health (own .beads/)", :77 "Doctor capabilities (contract sanity)", :88 "Doctor robot-docs (parseable)", :95 "Doctor --robot-triage (mega-envelope)", :102 "Fixture suite (default mode)", :111 "Fixture suite (idempotence replay gate, opt-in)", :124 "Scorecard regression check". `e2e-full.yml:33-90` jobs `e2e-full` ("Run full E2E suite") and `benchmarks-full` (`:110 run: scripts/bench.sh`); `ci.yml:987 run: scripts/e2e.sh` in the `e2e-quick` job. `compaction-certificate-validation.yml:65-241` steps "Run the differential controls before any repair", "Require the original named malformed-certificate failure", "Apply only the reviewed healthy-family fixture repair", "Prove the repaired hook and all refusal controls", "Require the complete release gate", with `:232-234` running fmt + `cargo check --locked --all-targets` + `cargo clippy --locked --workspace --all-targets --keep-going -- -D warnings`. GO additionally has `pr-risk.yml`, `ci-measurements.yml`, `nightly.yml`, `nix-build.yml`, `test-pypi.yml`, `triage-label.yml`, `docs-mintlify.yml`, `docs-autofix.yml`, `dependabot.yml`.

**Why the refuter killed it**

> REFUTED. The finder's local_evidence is a bare `ls` that misses (1) git history, (2) the project's documented replacement CI mechanism, and (3) that one of the five workflows is DWS-repo-specific incident scaffolding.
> 
> 1) FOUR of the five were DELIBERATELY DELETED, not "missing". `git log -1 d86d1b98` -> "chore: remove stale workflows (conformance, doctor, e2e, acfs, package-manifests)" (quangdang46, 2026-07-05), `git show --stat d86d1b98` -> 5 files / 711 deletions: doctor.yml, e2e-full.yml, notify-acfs.yml, update-package-manifests.yml, conformance.yml. `git log --all --diff-filter=A -- .github/workflows/` proves LOCAL once had 4 of the 5. The finder never looked at history, so "no LOCAL counterpart" is really "LOCAL chose to delete it."
> 
> 2) The gate CONTENT exists locally via the project's OWN documented mechanism. `docs/CI_SUPPLY_CHAIN.md:101-113` designates Cargo "workflow proof" targets run through RCH (AGENTS.md: "Agents run workflow proof Cargo targets directly through RCH") as canonical:
> - doctor gate: `tests/e2e_doctor_fixture_suite.rs:1-14` wires `tests/doctor_fixtures/run_all.sh` into `cargo test` ("wire the suite into `cargo test` and the CI pipeline"); the opt-in idempotence-replay gate is implemented at `tests/doctor_fixtures/run_all.sh:19-21,191` (`REPLAY_IDEMPOTENCE=1`) and `src/cli/commands/doctor_subsystems/mutate.rs:2293-2296`; `tests/e2e_doctor_chokepoint.rs:518` is "Test 3 — Idempotence"; `.githooks/pre-commit` runs `br doctor --quick --json` as a commit-boundary guard; `scripts/ci-local.sh:23-40` runs "doctor/recovery postconditions" reliability gates. Every subcommand DWS's doctor.yml invokes exists locally: `src/cli/mod.rs:3712-3752` (Health, Capabilities, RobotDocs) and `:3637` (`br.doctor.triage.v1` mega-envelope).
> - e2e/bench: `scripts/e2e.sh`, `e2e_full.sh`, `bench.sh`, `bench_regression.sh` + `scripts/check_regression.py` + 5 `tests/bench_*.rs` targets.
> - package manifests: `tests/package_manifests.rs` (410 lines) validates `packaging/homebrew/br.rb`, `packaging/scoop/br.json`, `packaging/aur/PKGBUILD` — the exact artifacts the deleted workflow maintained; `UPGRADE_LOG.md:25` documents their asset-name/checksum fix.
> - notify-acfs: the harness SURVIVED the workflow deletion — `tests/workflow_notify_acfs.rs` (15 KB), `.github/action-pins.jsonl:28-29` rows, `scripts/verify-notify-acfs-workflow.sh`, `docs/CI_SUPPLY_CHAIN.md:101-113`.
> 
> 3) The FIFTH (compaction-certificate-validation.yml) is DWS-repo-specific, not a portable capability. It self-describes as "A bounded #508 investigation" and is hardcoded `if: github.repository == 'Dicklesworthstone/beads_rust' && github.ref == 'refs/heads/main'` — it commits a reviewed repair straight to DWS main. `git log --all -- .github/workflows/compaction-certificate-validation.yml` is empty (never existed in LOCAL on any branch); LOCAL lacks its target test (`rg vacuum_into_private_maintenance... compaction_certificate_tests src/ tests/` -> no hits). It is incident scaffolding tied to a DWS bug, not a feature LOCAL is missing.
> 
> 4) Upstream citations are ACCURATE (so present_in=dws-only is a fair read): DWS doctor.yml has the cited steps verbatim, e2e-full.yml has both `e2e-full` and `benchmarks-full` (`:110 run: scripts/bench.sh`). GO has NONE of the 5 (grep of GO's 21 workflows -> no match), so this is a DWS-vs-LOCAL delta, not upstream-wide.
> 
> Honest residue: LOCAL's actual `.github/workflows/ci.yml` (37 lines) only runs `cargo fmt --check` + `scripts/build-web.sh` + `cargo test --lib --all-features`, so these harnesses are NOT enforced by GitHub Actions — they run via ci-local.sh/RCH/pre-commit. That is a real enforcement-surface difference, but it is a documented project policy, not an unimplemented capability.

**Corroborator note**

> Claim is materially overstated: 3 of the 5 listed workflows are not portable gaps, the counts are wrong (DWS 9 not 8; GO 23 not 21; GO has no dependabot.yml), and the core impact examples are false. Corrected scope: DWS's doctor.yml and e2e-full.yml are genuine LOCAL CI-wiring gaps (citations exact — ci.yml:987, doctor.yml:54/77/88/95/102/111/124), but compaction-certificate-validation.yml is a bounded #508 incident hotfix for code LOCAL does not have (src/franken_sync.rs, src/compaction_certificate_tests.rs absent), notify-acfs.yml is an org-specific cross-repo dispatch to Dicklesworthstone/agentic_coding_flywheel_setup, and update-package-manifests.yml is a post-release packaging nicety. The doctor impact claim is false on its own examples: the fixture suite already runs under cargo test (tests/e2e_doctor_fixture_suite.rs, executed by scripts/ci-local.sh:58), the robot-docs/capabilities/triage envelopes are already asserted in tests (e2e_doctor_chokepoint.rs:662,722; surface.rs:101-105), and the identical .githooks/pre-commit doctor gate exists in both repos. Real residual: the opt-in REPLAY_IDEMPOTENCE replay pass is not enabled in any LOCAL automated path, and there is no weekly full-E2E/benchmark schedule. Severity medium -> low.

### A25. No schema-migration or cross-version upgrade tests, despite a live versioned migration runner

- **Domain:** `testing-parity-docs` · **Type:** `missing_test` · **Claimed severity:** medium · **Claimed present in:** both-upstreams
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: both-upstreams · corrected severity: low

**Claimed upstream evidence**

> DWS `tests/e2e_schema_migration_upgrade.rs` (663 lines, 23 KB) and `tests/e2e_upgrade.rs` (711 lines), both DWS-only. GO `.github/workflows/migration-test.yml:20-40` — job `historical-upgrades` name "Historical ${{ matrix.version }} → candidate", matrix `version: v0.9.1 / v0.17.0 / v0.49.6 / v0.50.3` each with a `cache-identity` digest, plus `on: push: tags: ['v*']`. GO `.github/workflows/cross-version-smoke.yml:3-25` — "On tag push (release gate): test the last 30 releases ... On PR to main or a release branch: test only the last 5 releases for fast feedback. Release-targeted PRs are exactly the ones where cross-version upgrade breakage is most expensive to discover at tag time."

**Why the refuter killed it**

> REFUTED — the claim's central assertion ("No test exercises upgrading a database written by an older schema") is factually false.
> 
> The finder's evidence was `rg -ln 'upgrade' tests/*.rs` -> only `tests/e2e_schema.rs`. That search was methodologically broken on two counts: it scanned only the `tests/` integration directory (missing `src/` entirely), and only for the literal token "upgrade". The project's own AGENTS.md testing policy states unit tests live inline as `#[cfg(test)] mod tests` in each `src/` module — which is exactly where the migration tests are.
> 
> Ran `rg -n '#\[test\]|fn test_|user_version' src/storage/schema.rs`: 37 `#[test]` functions, ~14 of which are old-schema upgrade tests:
> 
> 1. `test_v10_migration_adds_source_repo_path_when_missing` (src/storage/schema.rs:2591) — the decisive one. Hand-builds the full canonical v9 `issues` table (26 columns, deliberately omitting `source_repo_path`), stamps `PRAGMA user_version = 9`, calls `run_migrations_atomic(&conn, 9, 10)`, asserts the column is healed AND that `user_version` advanced to 10. It is a literal cross-version upgrade test of the exact `run_migrations_atomic` runner the claim says is untested. Docstring ties it to regression beads_rust#289.
> 2. `test_migration_blocked_cache_upgrade` (:3010) — doc comment reads verbatim "Test that migrations correctly upgrade old schemas." Builds legacy `blocked_issues_cache` with `blocked_by_json`, runs `run_migrations`, asserts `blocked_by`/`blocked_at` present and `blocked_by_json` dropped.
> 3. `test_v7_rebuilds_content_hashes_and_marks_dirty` (:2436) and `test_v7_rebuild_works_when_dirty_issues_has_no_default` (:2484) — both stamp `user_version = 6` then call `run_migrations`.
> 4. `test_v6_repair_integer_datetime_columns` (:2312), `test_v6_repair_legacy_status_values` (:2393), `test_v8_backfills_storage_null_in_default_columns` (:2521), `test_migration_blocked_cache_missing_issue_id` (:3092), `test_migration_adds_missing_issue_columns` (:3180, hand-builds `issues(id, title)` only), `test_migration_adds_missing_dependency_type` (:3283), `test_migration_rebuilds_legacy_config_metadata_primary_keys` (:3317), `test_runtime_schema_compatible_rejects_legacy_kv_primary_keys` (:3387, recreates legacy config/metadata PKs on disk, reopens, asserts repair path), and four `test_check_schema_skew_*` (:3543-:3607, forward/behind-readonly/behind-readwrite/ignore-flag).
> 
> No escape hatch applies. `rg -n 'cfg\(feature' src/storage/schema.rs` returns zero hits — the migration runner and its tests are not feature-gated. `.github/workflows/ci.yml:35` runs `cargo test --lib --all-features`, which executes every one of these on every PR to main. The schema-version downgrade gate has its own tests at src/cli/commands/doctor_subsystems/refuse_gates.rs:352 and :377.
> 
> Checked every other surface the task asked about: CLI (src/cli/mod.rs:1075 `Upgrade` is the binary self-update subcommand, a different capability; no `migrate` subcommand exists, but none is needed since migration runs on open), MCP (only src/mcp/resources.rs references schema_version, no migration tool), sibling modules, and CHANGELOG.md/UPGRADE_LOG.md (UPGRADE_LOG.md is a dependency-upgrade log with no schema entries — no rename/supersession statement exists). No feature-gated or alternate-location implementation was found because the primary implementation is plainly present.
> 
> Upstream citations verified as plausible: DWS tests/e2e_schema_migration_upgrade.rs = 663 lines and tests/e2e_upgrade.rs = 711 lines (exact match to claim). GO .github/workflows/cross-version-smoke.yml:1-13 contains the quoted comment "Release-targeted PRs are exactly the ones where cross-version upgrade breakage is most expensive to discover at tag time", and migration-test.yml exists. present_in=both-upstreams is the correct read.
> 
> Residual truth (does not rescue the claim): LOCAL lacks the CI release-gate matrix layer. `.github/workflows/` holds only audit.yml, ci.yml, release.yml; ci.yml is fmt + `cargo test --lib --all-features` on ubuntu-latest with no historical-version matrix. So DWS/GO uniquely gate on real historical release artifacts. But that is a narrower, different finding than "no schema-migration or cross-version upgrade tests," which is what was claimed. docs/E2E_COVERAGE_MATRIX.md:213 marks the `upgrade` row 🔶, but that row covers the `br upgrade` binary self-update subcommand, not schema migration — the finder appears to have conflated the two.

**Corroborator note**

> The claim is refuted on its central assertion, though a narrower real gap survives.
> 
> WHAT IS WRONG:
> - "No test exercises upgrading a database written by an *older* schema" is false. LOCAL has at least 10 genuine legacy-schema upgrade tests. The decisive one, `src/storage/schema.rs:2591` `test_v10_migration_adds_source_repo_path_when_missing`, builds a canonical v9 `issues` table, stamps `user_version = 9`, runs `run_migrations_atomic(&conn, 9, 10)`, and asserts both the healed column and the advanced stamp — a regression test for shipped bug beads_rust#289. `src/cli/commands/doctor_subsystems/mutate.rs:2061` drives a full 7→21 `Op::DbMigrate` through the production chokepoint and checks snapshot fidelity, the v9 `close_metadata` step, the final version, and the actions.jsonl record.
> - The evidence method is invalid. `rg -ln 'upgrade' tests/*.rs` only scans the `tests/` integration dir (LOCAL's migration tests are inline `#[cfg(test)]` in `src/`) and greps the wrong token ("upgrade" not "migrat"/"user_version"). The claim never inspected where the tests actually are.
> - DWS `tests/e2e_upgrade.rs` is mischaracterized: it is the binary self-update command suite, not schema migration. Its own header (lines 1-14) says the tests "cannot actually perform upgrades as that would modify the binary under test."
> 
> WHAT IS ACCURATE:
> - GO's `cross-version-smoke.yml:4-13` quote and `migration-test.yml:23-24` job name and `tags: ['v*']` trigger are verbatim correct.
> - DWS `e2e_schema_migration_upgrade.rs` (663 lines) is genuine and is the single best artifact — real released-binary fixtures with an explicit "NOT synthesized PRAGMA user_version stamps" provenance note.
> - LOCAL's description of its migration runner (user_version stamping, CURRENT_SCHEMA_VERSION, the atomic from→target path) is accurate.
> 
> WHAT THE CLAIM UNDERSTATES: `migration-test.yml`'s matrix is 14 pinned versions (v0.9.1 … v1.2.2), not 4. And LOCAL's own exposure is larger than claimed: `CURRENT_SCHEMA_VERSION = 21` (schema.rs:12) means ~20 migration boundaries in the field, not "more than one."
> 
> RESIDUAL GAP WORTH KEEPING (as low, re-scoped): LOCAL has no cross-release fixture fidelity test (no DB produced by an actual old released binary — its old-version DBs are synthesized in-test from hand-written DDL, which can drift from what a real old binary wrote) and no CI release gate running upgrades against pinned historical releases. Port the DWS `.gz` release-binary fixture approach; the GO tag-gated matrix is lower value and largely Dolt-specific.

### A26. No dependency/dependent direction toggle on graph traversal (br graph --dependencies missing)

- **Domain:** `triage-analytics` · **Type:** `missing_flag` · **Claimed severity:** info · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: dws-only · corrected severity: info

**Claimed upstream evidence**

> DWS: src/cli/mod.rs:3774-3776 `#[arg(long, conflicts_with = "all")] pub dependencies: bool`; src/cli/commands/graph.rs:188 `GraphDirection::from_flag(args.dependencies)`, :702 `pub enum GraphDirection`, :711 `const fn from_flag`, consumed at :223-224 and :650-651.

**Why the refuter killed it**

> REFUTED. The finder's LOCAL search was too narrow — it only queried DWS-specific identifiers (`GraphDirection`, `from_flag`) and `struct GraphArgs`, and never checked the direction-toggle surface that actually exists in LOCAL.
> 
> WHAT I FOUND IN LOCAL (a real, reachable implementation):
> 
> 1. `src/cli/mod.rs:2641` defines `pub enum DepDirection { Down, Up, Both }` (`#[derive(ValueEnum, ...)]`, `#[default] Down`), wired into two arg structs:
>    - `src/cli/mod.rs:2625` `DepListArgs.direction` -> `#[arg(long, default_value = "down", value_enum)]` => `br dep list <id> --direction down|up|both`
>    - `src/cli/mod.rs:2659` `DepTreeArgs.direction` -> `#[arg(long, short = 'd', default_value = "down", value_enum)]` => `br dep tree <id> -d down|up|both`
>    Both registered as subcommands at `src/cli/mod.rs:2446-2458` (`DepCommands::{List,Tree,...}`).
> 
> 2. This is a genuine recursive graph closure traversal, not a one-hop edge list. `build_dep_tree_nodes_global` (`src/cli/commands/dep.rs:1128`) runs a queue BFS from the root, expanding via the direction-aware `dep_tree_neighbors` (`dep.rs:1026`, which dispatches Down->dependencies map, Up->dependents map, Both->union) until `args.max_depth` (default 10), with cycle detection via `item.path` and `dep_tree_truncated` markers. `dep_tree` (`dep.rs:1320`) renders it, including `--format mermaid` graph output via `render_dep_tree_mermaid` (`dep.rs:1410`).
> 
> 3. Not feature-gated: `rg 'cfg\(feature' src/cli/commands/dep.rs` returns zero hits. It is on the default build path.
> 
> 4. Tested: `dep.rs:1960-2000` asserts `dep_tree_neighbors` returns exactly `get_dependencies`/`get_dependents` for Down/Up/Both; `dep.rs:2082` exercises `DepDirection::Both`, `:2119` `DepDirection::Down`. LOCAL `CHANGELOG.md:890` records "Correct dep tree test to verify dependency traversal direction" (commit 1579204), confirming this is a deliberate, first-class LOCAL feature.
> 
> 5. The MCP surface also carries both sides: `src/mcp/resources.rs:205,217` exposes separate `beads://issues/{id}/dependencies` and `.../dependents` resources.
> 
> WHY THE CLAIMED `graph` HARDCODE IS NOT A GAP: DWS itself documents that `br graph` is a specialized dependents-only view and that `br dep tree` is the dependency-shaped view. DWS `src/cli/mod.rs:3750-3761`: "`--dependencies` walks the other way ... so one command covers both directions. `br dep tree <id>` remains the dependency-shaped view for a single issue." DWS added `--dependencies` as a convenience alias on top of the `DepDirection` machinery it already had (DWS `src/cli/mod.rs:2298` `DepDirection`, `:2274-2325` `DepListArgs`/`DepTreeArgs` — byte-for-byte the same design LOCAL carries). LOCAL ported the underlying design and omits only the redundant alias. Note LOCAL's `-d` on `dep tree` is `--direction`, whereas GO uses `-d` for `--max-depth` (GO `cmd/bd/dep.go:1553-1555`), so the flag letter differs by design lineage, not capability.
> 
> UPSTREAM SANITY CHECK: the DWS citation is real — `src/cli/mod.rs:3774-3776` has `#[arg(long, conflicts_with = "all")] pub dependencies: bool`, and `src/cli/commands/graph.rs` has `GraphDirection::from_flag` at :710, enum at :702, consumed at :188/:200/:223-224/:601/:650-651. So the finder did not fabricate it. But GO (`gastownhall/beads`) `cmd/bd/graph.go:357-362` shows the `graph` command has flags only `all/compact/box/dot/html/open` — GO has NO `graph --dependencies`; its direction toggle lives on `dep tree`/`dep list` (`cmd/bd/dep.go:1554-1555, 1564`, help text at :1213-1216 "down/up/both"). Hence the *capability* is present in BOTH upstreams, contradicting the "dws-only" framing: the only thing unique to DWS is the alternate spelling.

**Corroborator note**

> Claim stands as literally true but should be reframed from "missing capability" to "missing flag placement". The direction toggle is NOT absent in LOCAL — src/cli/mod.rs:2641-2649 (DepDirection Down/Up/Both) already provides it on `br dep tree --direction`, and LOCAL's version additionally supports a `both` mode that DWS's boolean `--dependencies` flag cannot express. DWS's own doc comment at src/cli/commands/graph.rs:695-701 frames its flag purely as avoiding a trip to `br dep tree`, so this is a deliberate ergonomics choice on DWS's side, not a capability DWS gained over LOCAL. The honest one-line description is: DWS moved the existing dep-tree direction toggle onto the graph renderer for discoverability; LOCAL left it on `dep tree`. Info is correct and already the floor. If anything is worth recording, it is the reverse observation — that LOCAL's `dep tree --direction both` is a capability DWS lacks — not the claimed gap.

### A27. No ASCII box rendering with per-node dependency counts (bd graph --box missing)

- **Domain:** `triage-analytics` · **Type:** `missing_flag` · **Claimed severity:** low · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = false` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> GO: cmd/bd/graph.go:359 `graphCmd.Flags().BoolVar(&graphBox, "box", false, "ASCII boxes showing layers")`; cmd/bd/graph.go:1203 `func computeDependencyCounts(subgraph *TemplateSubgraph) (blocks map[string]int, blockedBy map[string]int)`; :1241 `renderNodeBoxWithDeps`; :1155 `renderNodeBox`; :1097 `renderCompactChildren` (layer layout).

**Why the refuter killed it**

> I searched LOCAL exhaustively and the finder's literal local_evidence checks out — but the gap does not survive as a capability gap.
> 
> WHAT I CONFIRMED IS GENUINELY ABSENT (the narrow literal truth)
> - `rg -n 'renderNodeBox|computeDependencyCounts|renderCompactChildren|renderNodeBoxWithDeps' src/` -> 0 hits.
> - `rg -n '"box"|--box|graph_box|graphBox|box_mode|boxMode' src/ tests/` -> 0 code hits. The only `--box` matches in the whole repo are two lines of prose in `docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:6660,6969` describing the GO original.
> - Read `GraphArgs` at `src/cli/mod.rs:3978-3990`: exactly three fields — `issue`, `all`, `compact`. No `--box`.
> - Read the renderers (`src/cli/commands/graph.rs:1009 render_single_graph_plain`, `:1045 render_single_graph_rich`, `:1131`, `:1154`, `:1251`): all draw an indented tree inside a `rich_rust` `Panel` (`.box_style(theme.box_style)` at :1125 is panel chrome, not a per-node box). Content is id/title/`[P#]`/`[status]`/parent-list only. `GraphNode` (graph.rs:31-37) carries only `id,title,status,priority,depth` — so no `blocks`/`needs` count appears in the human OR `--json` graph output.
> - `rg 'cfg(feature' src/cli/commands/graph.rs` -> 0 hits, so it is not feature-gated and hidden. `src/web/mod.rs` is a static SPA + REST passthrough with no graph renderer.
> So the `--box` flag and the layered box-drawing renderer are really not in LOCAL.
> 
> WHY I STILL REFUTE THE GAP
> 1. The substantive capability the claim names is implemented in LOCAL by a different, documented mechanism. The claim's own thesis is a per-node centrality signal ("a user cannot see which nodes are hubs"). LOCAL computes exactly that, for every node, and surfaces it in three places:
>    - `src/mcp/resources.rs:972-1026` `compute_bottlenecks` — header comment "Compute bottleneck issues: those that block the most other open issues", builds `blocks_count: HashMap<&str,usize>` (:988), ranks descending (:995-996), serves `beeds://issues/bottlenecks` (:1041), and states its own rationale at :972 ("bv-inspired") and :1026 ("High blocks_count = high PageRank equivalent. Resolve these first"). This is a line-for-line twin of GO's `computeDependencyCounts` blocks half.
>    - `src/format/output.rs:114-121` `IssueWithCounts { dependency_count, dependent_count }` — the blocks/blockedBy pair per node, emitted by `br list --json` and `br search --json` (wired at `list.rs:337`, `search.rs:302-318`), computed by `storage.count_relation_counts_for_issues` (`src/storage/sqlite.rs:9308`, returning `(dependency_counts, dependent_counts)`).
>    - `src/format/output.rs:172-177` `BlockedIssue { blocked_by_count, blocked_by }`.
> 2. The flag omission is a DOCUMENTED DELIBERATE port decision, not an oversight. LOCAL's own porting doc puts it in the exclusion matrix: `docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:6872` heads section 15.82 "Non-Classic CLI Flag Matrix (Excluded / Optional for br v1)", and `:6969` reads `| graph | --all, --compact, --box | Optional |` — deliberately triaged as optional, alongside consciously excluded flags. `:6660` documents the GO behavior being consciously scoped out.
> 3. The claim's own description concedes the counts require "a separate counting command" — and `br list/search --json` plus `beads://issues/bottlenecks` are precisely that. So the user-visible outcome the claim says is missing is reachable in LOCAL.
> 
> UPSTREAM CITATION PLAUSIBILITY — VERIFIED
> GO `cmd/bd/graph.go`: flag at :359; dispatch `else if graphBox { renderGraph(layout, subgraph) }` at :219 (all) and :262 (single); `renderGraph` at :933 calls `computeDependencyCounts(subgraph)` at :959 and `renderNodeBoxWithDeps(node, boxWidth, blocksCounts[id], blockedByCounts[id])` at :968 — so the per-node `blocks:N` / `needs:N` annotation is genuinely reachable, not dead code. `:1155 renderNodeBox` and `:1203 computeDependencyCounts` and `:1241 renderNodeBoxWithDeps` all exist as claimed. Note GO's box is opt-in here (the `else` branch is `renderGraphVisual`), which is the opposite of what the porting doc's "box is default" note says — a minor doc/code drift worth flagging, but it does not change the verdict.
> 
> DWS CROSS-CHECK (the claim's "go-only" holds for the artifact)
> `/tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/commands/graph.rs` exists but has no `renderNodeBox*`; DWS `GraphArgs` (src/cli/mod.rs:3764-3782) is the same three flags plus `--dependencies` and DOT output. DWS's `src/mcp/resources.rs:1012-1033` carries the same `compute_bottlenecks`/`blocks_count` code LOCAL inherited. So the box renderer is genuinely GO-only, while the per-node count signal is present in all three codebases.

**Corroborator note**

> Claim stands as a real, go-only cosmetic gap, but severity should be info, not low, and the impact is overstated.
> 
> 1) IMPACT OVERSTATED: "a user cannot see which nodes are hubs without running a separate counting command" is false for LOCAL. LOCAL already ships a purpose-built ranked hub surface computing the same per-node `blocks_count`: src/mcp/resources.rs:977 `compute_bottlenecks` (exposed as `beads://issues/bottlenecks`, :972) and src/mcp/prompts.rs:420 `bottleneck_context` (used by the triage/plan prompts). Both rank hubs descending with interpretation hints, and resources.rs:976 describes the signal as "a practical approximation of PageRank/betweenness from bv". The companion `bv` engine adds betweenness/PageRank/HITS/eigenvector/k-core. GO's inline `blocks:N`/`needs:N` is the weaker form of the same signal.
> 
> 2) SEVERITY low -> info: it is a rendering-mode nicety (one of four text renderings of the same DAG: default/--box/--compact/--dot/--html), and its only added information is ASCII borders plus a count LOCAL already surfaces elsewhere. No core agent workflow, data-integrity, or data-loss impact.
> 
> 3) CITATION DEFECTS: (a) graph.go:1155 `renderNodeBox` is the variant WITHOUT dep counts and has no production caller — only graph_test.go:178; the live renderer is `renderNodeBoxWithDeps` (:1241). (b) graph.go:1097 `renderCompactChildren` is the `--compact` tree renderer, not box layer layout; `--box` layering comes from `computeLayout`/`layout.Layers` plus the "Layer %d" header at :986. The remaining citations (:359, :1203, :1241) are exact.
> 
> 4) MINOR GLOSS: "computed for every node, not just the root" is not strictly true — computeDependencyCounts excludes non-`DepBlocks` dep types (:1218) and deliberately skips edges whose blocker is the root (:1222-1226, to avoid "cognitive noise"), so the root's `blocks` count omits its children by design.

### A28. Dependency graph cannot be exported as Graphviz DOT (br graph --dot missing)

- **Domain:** `triage-analytics` · **Type:** `missing_output_mode` · **Claimed severity:** medium · **Claimed present in:** both-upstreams
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: both-upstreams · corrected severity: info

**Claimed upstream evidence**

> GO: cmd/bd/graph.go:360 `graphCmd.Flags().BoolVar(&graphDOT, "dot", false, "Output Graphviz DOT format (pipe to: dot -Tsvg > graph.svg)")`; full renderer in cmd/bd/graph_export.go:15 `func renderGraphDOT(out io.Writer, layout *GraphLayout, subgraph *TemplateSubgraph) error`, plus dotNodeAttrs (:98), dotEdgeStyle (:123), dotEscapeID (:137). DWS: src/cli/mod.rs:3782-3784 `#[arg(long)] pub dot: bool`; src/cli/commands/graph.rs:1249 `fn format_all_graph_dot`, :1227 `fn format_single_graph_dot`, :1196 `fn push_dot_node`; wired at graph.rs:176 and :187.

**Why the refuter killed it**

> REFUTED — the claim conflates "no `--dot` flag on GraphArgs" with "no machine-readable graph serialization." Its two load-bearing sentences are both false, and LOCAL self-documents the contradicting capabilities.
> 
> WHAT I RAN AND FOUND
> 
> (1) `br graph` DOES emit machine-readable graph serialization, including edges, via the global `--json` flag. The flag is declared `global = true` at /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs:709-711 (`#[arg(long, global = true)] pub json: bool`), so it binds to `graph` even though it is absent from `GraphArgs` (src/cli/mod.rs:3978-3990). The serializable graph types are at /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/graph.rs:30-59: `GraphNode{id,title,status,priority,depth}`, `SingleGraphOutput{root, nodes, edges: Vec<(String,String)>, count}`, `ConnectedComponent{nodes, edges, roots}`, `AllGraphOutput{components, total_nodes, total_components}`. Dispatch is at graph.rs:215-225 (single-issue) and graph.rs:277-286 / :411-420 (--all), routing to `ctx.json_pretty` / `ctx.toon`. Unit tests assert the emitted edge list: `test_single_graph_output_serialization` at graph.rs:1313-1338 builds `edges: vec![("bd-002","bd-001")]` and serializes it; also `test_connected_component_serialization`, `test_all_graph_output_serialization`.
> 
> (2) LOCAL's own capability manifest explicitly declares this contract at /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/capabilities.rs:977-982 — `"graph" | "orphans" | "changelog" | "lint" | "audit" => CommandContract { machine_output: &["json", "toon", "text"], ... }`. This directly contradicts the claim's "the terminal/rich/plain renderers are the only outputs."
> 
> (3) A second, independent, render-ready graph serializer exists: `br dep tree <id> --format mermaid`. `DepTreeArgs.format` at /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs:2665-2667 ("Output format: text, mermaid"; candidate list at src/cli/mod.rs:173). `render_dep_tree_mermaid` at /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/dep.rs:1410-1424 emits `graph TD`, node defs `key["id: title [P<n]"]`, and `parent --> child` edges — a complete graph DSL byte-for-byte parallel to Graphviz DOT, wired at dep.rs:1363-1367. It is self-documented in capabilities.rs:611 and given as a first-class example at capabilities.rs:703 (`br dep tree br-task --direction up --format mermaid`). It has a dedicated 25 KB e2e suite /Users/tranquangdang21/Projects/beads_rust/tests/e2e_dep_tree_mermaid.rs (parses and validates the `graph TD` header, node labels, and the edge list; covers linear/branching/diamond shapes and title escaping) plus coverage-matrix entries at docs/E2E_COVERAGE_MATRIX.md:85,89,241.
> 
> (4) docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:6650-6724 (§15.76 Graph Command port notes) lists the retained graph flags as `--compact`, `--box`, `--json` — i.e. JSON was a deliberate part of the port plan, and the `:6969` parity table row reads `| graph | --all, --compact, --box | Optional |`. The same doc at :4683-4684 documents DOT under `list --format` (`dot` → Graphviz DOT, `digraph` → edge list) rather than the graph command, so DOT parity was scoped to list-format templating.
> 
> WHY THE GAP CLAIM FAILS
> The claim's central assertion — "There is no machine-readable graph serialization of any kind, so an agent or human cannot visualize the dependency graph from `br` and must hand-roll an exporter" — is factually wrong. Visualization requires zero hand-rolling via `br dep tree --format mermaid` (mermaid-cli / mermaid.ink / GitHub / VS Code render it directly), and the full node+edge graph is available structurally via `br graph --json` / `--robot` / `BR_OUTPUT_FORMAT=toon` for feeding any external tool. The `rg -ni 'graphviz|digraph'` zero-hit result the finder cites only proves there is no DOT *string emitter*; it does not support the broader "no machine-readable graph output" conclusion the finder drew from it.
> 
> WHAT IS STILL TRUE
> The narrow literal point holds: LOCAL has no `graph --dot` flag and no DOT renderer (confirmed by the zero-hit grep, and every `dot` token in src/ is dot-notation issue IDs, e.g. src/util/id.rs, src/storage/sqlite.rs:7536 `get_open_dot_notation_children`). So a DOT-specific parity item is real — but it is a format-parity nicety, not a lost capability, and the medium severity rested entirely on the false "cannot visualize" premise.
> 
> UPSTREAM EVIDENCE SANITY-CHECK (both citations verified plausible)
> GO (/tmp/beads_gap_audit/gastownhall_beads): cmd/bd/graph.go:39 `graphDOT bool`; :213-214 and :256-257 dispatch to the renderer; :360 `graphCmd.Flags().BoolVar(&graphDOT, "dot", false, "Output Graphviz DOT format (pipe to: dot -Tsvg > graph.svg)")`; cmd/bd/graph_export.go:13,15 `renderGraphDOT(out io.Writer, layout *GraphLayout, subgraph *TemplateSubgraph) error`. All confirmed present.
> DWS (/tmp/beads_gap_audit/dicklesworthstone_beads_rust): src/cli/mod.rs:3781-3783 `/// Emit Graphviz DOT notation (pipe to 'dot -Tsvg'); overrides text/JSON rendering` + `#[arg(long)] pub dot: bool`; src/cli/commands/graph.rs:176,187 pass `args.dot` into `graph_all`/`graph_single`; renderers at :1196 `push_dot_node`, :1227 `format_single_graph_dot`, :1249 `format_all_graph_dot`; unit tests at :2416, :2462, :2480, :2532. All confirmed present. So `present_in = both-upstreams` is correct for the DOT feature itself.
> 
> No files were created, edited, or deleted; no cargo or git-mutating command was run. All operations were read-only greps, sed reads, and file listings.

**Corroborator note**

> Refuted. Both upstream citations are accurate (DWS line numbers exact; Go dotEdgeStyle is :124 not :123), and `--dot` is genuinely present in both upstreams — so `present_in: both-upstreams` is right for the flag itself. But the claim is built on a false premise: "There is no machine-readable graph serialization of any kind" is wrong. LOCAL's `br graph --json` and `br graph --all --json` already emit the complete graph — `SingleGraphOutput{root,nodes,edges,count}` and `AllGraphOutput{components[{nodes,edges,roots}],total_nodes,total_components}` (src/cli/commands/graph.rs:31-62, emitted at :215-228 and :411-424). The stated impact ("requires a third-party script re-implementing the traversal") is also wrong: br performs the traversal, and I verified a jq one-liner over that exact schema emits valid DOT. Corrected framing: LOCAL is missing only a ~20-line DOT *renderer* over data it already serializes — DWS's own `format_single_graph_dot(nodes: &[GraphNode], edges: &[(String,String)], root_id)` (DWS graph.rs:1227) takes precisely the same inputs, and LOCAL's `GraphNode` (graph.rs:31-37) is field-identical to DWS's. This is also a documented deliberate divergence, not an oversight: bead `beads_rust-3ayr` closed with "ERROR: --robot-* flags are bv's domain, not br's. br is non-invasive CLI only," AGENTS.md:683 designates `bv --robot-graph [--graph-format=json|dot|mermaid]` as the graph-export surface, and bv v0.16.4 is installed and supports `--graph-format dot` — so DOT export already ships in the local ecosystem. Severity corrected medium -> info.

### A29. No interactive HTML graph export (bd graph --html missing)

- **Domain:** `triage-analytics` · **Type:** `missing_output_mode` · **Claimed severity:** medium · **Claimed present in:** go-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: go-only · corrected severity: info

**Claimed upstream evidence**

> GO: cmd/bd/graph.go:361 `graphCmd.Flags().BoolVar(&graphHTML, "html", false, "Output self-contained interactive HTML (redirect to file)")`; mergeSubgraphsForHTML at cmd/bd/graph.go:780; dedicated cmd/bd/graph_visual.go (388 lines) and cmd/bd/graph_export.go (380 lines); consumption at graph.go:192 `if graphHTML && !graphOpen`.

**Why the refuter killed it**

> The claim's local evidence was scoped only to `src/` (Rust), which missed the entire `web/` frontend directory that ships in the same repo. LOCAL already delivers the user-visible outcome the gap describes — a human can pan/zoom a dependency graph, click into a node, and read edge semantics (blocking vs related) — via a default-on feature.
> 
> What I ran and found:
> 
> 1. Interactive graph view exists: `/Users/tranquangdang21/Projects/beads_rust/web/components/graph-view.tsx` (182 lines) is a real @xyflow/react (React Flow) graph. It imports `ReactFlow, Background, Controls, Handle, Position, ReactFlowInstance`; line 115 `rf.current?.fitView({ padding: 0.2, duration: 400 })`; line 159 `fitView`; line 162 `<Background gap={22} .../>`; line 163 `<Controls />` (the pan/zoom/fit widget). It builds nodes/edges from `b.dependencies` (lines 87-107) and styles edges by dep type: blocking types get `#ef4444` at strokeWidth 2, related get dashed `--brand`. `web/package.json:34` pins `"@xyflow/react": "^12.11.0"`. This is pan/zoom + hubs/cycles inspection, exactly the stated user need.
> 
> 2. It is reachable and default-on, not feature-gated off. `Cargo.toml:165` `default = ["web"]`; `Cargo.toml:167` `web = ["dep:axum", "dep:tokio", "dep:tower-http", "dep:rust-embed"]`; `src/lib.rs:55-56` `#[cfg(feature = "web")] pub mod web;`; `src/main.rs:522-523` `#[cfg(feature = "web")] Commands::Web(args) => beads_rust::web::run_server(...)`. `br web` opens a browser by default (`WebArgs.no_open` is opt-out, `src/cli/mod.rs:3867-3869`). Graph is a first-class nav item: `web/components/sidebar.tsx:42` `{ key: "graph", label: "Graph", icon: "graph" }`, dispatched at `web/components/app-shell.tsx:15,173` (`{view === "graph" && <GraphView />}`).
> 
> 3. The HTML/SVG artifact claim is factually wrong. The SPA is a static export built by `scripts/build-web.sh` into `src/web/static/` and embedded into the binary via `rust-embed` (`src/web/assets.rs:13,16` `#[derive(RustEmbed)]`; `src/web/mod.rs:38` `run_server`). `src/web/static/index.html` (10 KB) and the built chunks are present on disk. The built bundle `src/web/static/_next/static/chunks/0hokuz6-4p0-z.js` (1.4 MB) contains 50 `fitView`, 23 `d3`, 8 `Background`, 2 `Controls` occurrences — the graph code is compiled into the shipped artifact. So "there is no HTML/SVG graph artifact anywhere in the crate" is wrong, and the finder's `rg -ni 'svg|d3\.|forceSimulation'` was run against `src/` only and never against `web/`.
> 
> 4. "The web server likewise exposes only add_dep/remove_dep handlers and no dependency-graph read" is also wrong as a user-facing statement. The graph is client-side, fed by `GET /api/p/{project_id}/beads` (`src/web/mod.rs:82`, handler `src/web/api.rs:153 list_beads`), whose per-issue JSON includes `"dependencies"` at `src/web/api.rs:80` (`issue.dependencies.iter().map(|d| dep_to_json(d))`). Edges are therefore fully available to the visualization. That the dedicated `add_dep`/`remove_dep` handlers exist at `api.rs:556`/`:610` is consistent — `onConnect` at `graph-view.tsx:122-128` calls `addDep.mutate({ id: c.source, dependsOnId: c.target, type: "blocks" })`, i.e. drag-to-link writes through those same handlers. The finder treated the presence of write handlers as proof no read/visualization path exists, which is a non-sequitur.
> 
> 5. Additional equivalent I found that the claim missed: `br dep tree --format mermaid` (`src/cli/mod.rs:2665-2667`, renderer `src/cli/commands/dep.rs:1363-1416`, with control-char sanitization at `:1404`) emits a Mermaid graph-diagram artifact from the terminal, which any Markdown viewer renders as a pan/zoom SVG graph.
> 
> 6. I did not find the literal one-file `--html` self-contained artifact: `GraphArgs` (`src/cli/mod.rs:3978-3990`) has only `issue`, `--all`, `--compact` — no `--html`, no `--dot`, no `--open`. `rg -ni 'html' src/` returns only `web/assets.rs` MIME/fallback strings, `format/syntax.rs` language-name strings, and `recipes/mod.rs` an aider URL. No CHANGELOG/UPGRADE_LOG entry documents a rename or deliberate removal (docs/ hits are all Criterion `html_reports` or artifact-report tooling).
> 
> Upstream evidence check: plausible and accurate. In `/tmp/beads_gap_audit/gastownhall_beads`, `cmd/bd/graph.go:361` is exactly `graphCmd.Flags().BoolVar(&graphHTML, "html", false, "Output self-contained interactive HTML (redirect to file)")`; `graph.go:192` is `if graphHTML && !graphOpen {`; `mergeSubgraphsForHTML` is at `graph.go:780`; `cmd/bd/graph_visual.go` is 388 lines and `cmd/bd/graph_export.go` is 380 lines (768 total, matching "768 lines across two files"); `renderGraphHTML` is at `graph_export.go:161`. So GO genuinely has the self-contained-file variant, and DWS does not (`/tmp/beads_gap_audit/dicklesworthstone_beads_rust` has no `web/` dir at all and its `GraphArgs` at `src/cli/mod.rs:3764-3784` has `--dot` but no `--html`), so "go-only" is a fair read for the exact flag.
> 
> Net: the claim is refuted on its central assertion. LOCAL ships an interactive, pan/zoomable, default-enabled dependency graph (`br web` → Graph, React Flow, embedded HTML/SVG/JS artifact) plus a Mermaid export. The only genuinely missing piece is artifact portability — a single shareable `.html` file you can attach to a PR — which is a convenience delta, not a missing capability, and drops the severity from medium to low.

**Corroborator note**

> Gap is real in its narrow form (LOCAL ships no --html) but the claim mischaracterises it on three axes. (a) present_in "go-only" is wrong: DWS (dicklesworthstone_beads_rust) already ships `--dot` Graphviz export at src/cli/mod.rs:3781-3783, producing the same graph.svg the claim says is absent from the Rust ecosystem. (b) "Self-contained interactive HTML" is wrong: graph_export.go:286 loads D3 from https://d3js.org/d3.v7.min.js, so the file requires network at view time despite upstream's contradictory comment at graph_export.go:237-238. (c) Severity should be info, not medium: the repo's own AGENTS.md documents `bv --export-graph <file.html>` and `bv --robot-graph --graph-format=json|dot|mermaid`, and docs/BD_VS_BR.md explicitly records under Triage that "Graph analysis | bd: bv | br: bv (external) | Both use external bv tool" — interactive graph visualization is a deliberate, documented delegation to external tooling, not an oversight. It affects only human inspection, never agent workflows (agents use --json/--robot, fully supported). The actionable, low-cost subset is porting DWS's existing --dot flag (~55 LOC) rather than building an interactive D3 UI.

### A30. Linked git worktrees silently fork a second, divergent .beads workspace

- **Domain:** `workspace-discovery` · **Type:** `architecture_gap` · **Claimed severity:** high · **Claimed present in:** dws-only
- **Refuter verdict:** `refuted = true` (confidence: high)
- **Corroborator:** `refuted = true` (confidence: high) · present_in: both-upstreams · corrected severity: info

**Claimed upstream evidence**

> DWS CHANGELOG.md:1495-1499, 'Linked git worktrees resolve to the primary checkout (GitHub #429, commit 44c7a6f0)': "In a linked worktree, `.beads` discovery followed the worktree's own root and produced a second, divergent workspace. Discovery now resolves through `.git`-file indirection to the primary checkout's `.beads`." GO has the same limitation and the same marker-based design: cmd/bd/worktree_cmd.go:34 `BeadsState string // "redirect", "shared", "none"`, populated by `getBeadsState` at :558, with no `.git` indirection anywhere in its worktree or storage code — confirming the discovery-time resolution is DWS-only.

**Why the refuter killed it**

> REFUTED. The claim's local evidence grepped the wrong module, and its two load-bearing structural arguments are provably false. I found the actual implementation.
> 
> WHAT I RAN AND FOUND
> 
> 1. The claim grepped the WRONG file. Its evidence is `grep -rn 'worktree' src/sync/path.rs -> 0 hits; the discovery module has no worktree awareness at all`. `src/sync/path.rs` is the path VALIDATOR, not the discovery module. The real discovery module is `src/config/mod.rs`, which has 19 worktree hits (`rg -c -i 'worktree' -g '*.rs' src/`). Reading it directly (`sed -n '230,400p' src/config/mod.rs`) I found `discover_beads_dir_candidate_with_env` contains an explicit git-worktree-aware fallback, commented "Git worktree fallback: if we didn't find .beads/ walking up, check whether we're in a git worktree whose main repository has a .beads/ directory" (config/mod.rs:294-311), backed by `fn git_worktree_main_repo(dir: &Path) -> Option<PathBuf>` (config/mod.rs:322) which runs `git rev-parse --git-common-dir` and `git rev-parse --git-dir` and treats a difference as worktree. So "LOCAL's .beads discovery is git-unaware" is false, and "nothing consults it at discovery or init time" is false — it is consulted on every discovery miss. It has unit tests at :5204 and :5210. It is on the live path for all 94 discovery call sites, including CLI (`src/main.rs:762`), MCP (`src/mcp/mod.rs:699`), and hooks (`src/hooks/mod.rs:549`).
> 
> 2. The NGI-3 "structural tension" is false. The claim asserts LOCAL "cannot adopt DWS's fix as written" because the validator rejects any `.git` component. I checked: `rg -n 'validate_no_git_path' src/config/mod.rs` returns ZERO hits. config/mod.rs imports only `validate_sync_path_with_external` (line 17). The invariant is scoped to sync *file-write* operations ("br sync NEVER accesses .git/", NGI-1/NGI-3 at path.rs:194-232) and is never applied to discovery. Moreover LOCAL's fix does not read `.git` as a filesystem path at all — it shells out to `git rev-parse`, so there is nothing for the validator to reject. The alleged "structural tension between two LOCAL design decisions" dissolves: the two decisions never meet.
> 
> 3. "dws-only" is false — GO has the same mechanism. The claim says GO "has the same marker-based model as LOCAL and the same exposure, so the discovery-time fix is DWS-only." But `rg -rn 'func worktreeFallbackBeadsDirForRepo' -g '*.go'` finds `/tmp/beads_gap_audit/gastownhall_beads/internal/beads/beads.go:1054`, implemented with `git rev-parse --git-dir --git-common-dir` — architecturally identical to LOCAL's `git_worktree_main_repo`. It is consumed in `FindBeadsDirFrom` (beads.go:624, :654). GO even adds jj/jujutsu secondary-workspace handling. So the discovery-time resolution is in both upstreams, present_in is both-upstreams, not dws-only.
> 
> 4. Upstream citation sanity-check: DWS CHANGELOG.md:1495-1499 is real and matches the quoted text about GitHub #429 / commit 44c7a6f0. I confirmed DWS implements it in `src/config/mod.rs:340-440` via `linked_worktree_primary_beads_dir` + `primary_beads_dir_from_git_file` (parsing the `.git` file, never spawning git). The citation is plausible; the GO characterization attached to it is not.
> 
> RESIDUAL THAT SURVIVES (why medium, not info)
> 
> There is a real but much narrower gap, and it is not the one claimed. LOCAL's walk-up loop (config/mod.rs:279-291) returns the FIRST `.beads` it finds and only then reaches the git fallback. So a linked worktree that carries tracked `.beads/` metadata but no local DB (exactly the normal shape here — `git ls-files .beads` shows README.md/.gitignore/MCP_AGENT_MAIL_PATTERNS.md tracked while `.beads/.gitignore` excludes `*.db`) resolves to the worktree's own `.beads` instead of the primary. I confirmed LOCAL has no DB-aware guard: `rg -n 'has_local_database|has_beads_project_files|is_worktree_root' -g '*.rs' src/` returns NONE. Both upstreams do have one — GO at beads.go:638 (`isWorktreeRoot && fallbackHasDB && !hasBeadsDatabase(resolved)`) and DWS at `workspace_has_local_database`. So LOCAL is one guard clause behind, not architecturally absent. That is a real correctness bug worth fixing, but it does not support "high" severity, the "git-unaware" framing, or a dws-only attribution.

**Corroborator note**

> Refuted. Corrected reading: linked-worktree `.beads` resolution is present in BOTH upstreams and is ALREADY present in LOCAL. GO: `internal/beads/beads.go:1016 GetWorktreeFallbackBeadsDir()` + `:1053 worktreeFallbackBeadsDirForRepo()` (`git rev-parse --git-dir --git-common-dir`), wired into `FindBeadsDir()` :757 step 3c (~:897) and `FindBeadsDirFrom()` :589, plus `internal/config/config.go:405/:418`; shipped help at `cmd/bd/worktree_cmd.go:52` advertises it. DWS: `src/config/mod.rs:358-476` (`linked_worktree_primary_beads_dir`, `primary_beads_dir_from_git_file`). LOCAL: `src/config/mod.rs:294-311` already falls back to `<main_repo>/.beads` via `git_worktree_main_repo()` at `:322-372`. The claimed NGI-3 blocker is a sync *write*-path invariant (`src/sync/path.rs:125,194-218`; `.beads/SYNC_SAFETY_INVARIANTS.md:73`) and never applied to discovery — LOCAL solved it by spawning `git rev-parse` rather than reading the `.git` file. The claim's GO evidence sampled only the display helper (`worktree_cmd.go:34,:558`) and mistook a display-only marker model for the absence of the feature. Severity: info (worth an added positive test that a linked worktree resolves to the primary `.beads`, and optionally a `doctor` warning on `BeadsState::Local`), not high.


---

## Appendix B — Per-domain coverage notes (verbatim)

What each of the 13 find-agents recorded about the **scope it actually swept** — what it searched, what it concluded about coverage, and edge cases it saw but judged below the finding bar. This is the layer that answers "how far did you actually look?", and it is the first thing to read before trusting a gap as exhaustive.

### B — `completeness-critic`

> METHOD. Read-only throughout. No file written, edited, or deleted; no cargo invoked; no git mutation. The central technique that produced every finding here is the one the 12-domain sweep structurally could not perform: LOCAL is v0.1.3 (~Feb 2026) and Dicklesworthstone/beads_rust is v0.6.0 (~Sep 2026) — the SAME codebase, forked. So DWS-only features are precisely "added after the fork point", and DWS's own CHANGELOG.md (179KB) is a complete, dated, issue-linked index of them. Reading it top-down (v0.6.0 back to v0.2.0) and extracting feature lines surfaced 5 of my 7 findings that no domain had filed. I also did the top-down file/directory inventory diff the brief asked for, and profiled the GO top-level packages with no LOCAL analogue (journalops/, memoryops/, issueops/, plugins/, npm-package/, tools/, release-gates/, engdocs/adr/).
> 
> WHAT I RULED OUT, so the next pass does not re-investigate. These were live candidates that verification killed:
> - INDEX COVERAGE. The basename diff of DWS-only index names looked like a clean perf win (idx_comments_issue_id, idx_events_issue_id, idx_events_event_type, idx_labels_label_issue, idx_dependencies_issue_type, idx_dependencies_depends_on_type_issue all appearing DWS-only). It is a pure naming-convention artifact. LOCAL has every one of them under a different name — idx_comments_issue (src/storage/schema.rs:173), idx_events_issue (:197), idx_events_type (:198), idx_labels_issue (:162), idx_dependencies_type (:146), idx_dependencies_depends_on_type (:147). The 84-vs-154 CREATE INDEX count difference is mostly DWS-only tables (capacity_*, gate_result_history, rogue_*) plus LOCAL's own extras (interactions_*, gate_waiters_issue, fed_peers_sovereignty, repo_mtimes_checked). NO index gap exists. Nearly filed this as a false positive.
> - `br lint` rule coverage. DWS src/cli/commands/lint.rs is 1021 lines vs LOCAL's 709, which looked like missing validation rules. It is not: LOCAL implements the same BUG/TASK/EPIC/DECISION/SPIKE RequiredSection tables (src/cli/commands/lint.rs:77-118), the same markdown-heading matcher (:524+), and the same acceptance_criteria projection (:458-493). The delta is DWS regression-test volume, not rules.
> - FILE PERMISSIONS / umask. LOCAL has 0 umask calls and 0 `0o700` literals where DWS has 4 and 5, which looked like a hardening gap. DWS's apparent umask usage is a *test assertion* ("expect make database private regardless of the process umask", DWS src/config/mod.rs:11201), not a umask call. LOCAL enforces 0o600 at every point DWS does: src/util/mod.rs:104, src/cli/commands/config.rs:1568, src/cli/commands/init.rs:551, src/cli/commands/doctor.rs:14413, src/cli/commands/doctor_subsystems/mutate.rs:1694, src/util/credentials.rs:156. Parity.
> - `inherited_context` in `br show --json`. LOCAL has 12 src hits and 11 test hits — LOCAL is at parity or ahead. DWS gates it behind a config flag (src/inheritance.rs:98); LOCAL ships it.
> - Orphaned-sidecar recovery to `.br_recovery/` (DWS bead avhq). LOCAL has 26 `br_recovery` hits. Already ported.
> - `--lock-timeout`. LOCAL has 150 src hits. Parity.
> - `br sql` write exposure. LOCAL's SqlArgs (src/cli/mod.rs) documents `/// SQL query string (read-only: wrapped in a transaction that rolls back)`. Not a security gap.
> - GO-only `nocow_linux.go` / `nocow_other.go`. Dolt+btrfs-specific (FS_NOCOW_FL ioctl to stop kworker thrashing on btrfs compressed extents); the non-Linux build is an explicit no-op. No counterpart is possible or needed.
> - GO `sandbox_unix.go` isSandboxed() — a signal-0/EPERM probe used to auto-detect Codex/container sandboxes (bd-u3t, GH#353). Narrow, environment-detection-only, and it depends on GO's interactive-prompt gating. Not filed.
> - GO `rules.go` (23KB) audits `.claude/rules/*.md` for contradictions and token bloat. Real, but scoped to a Claude Code artifact rather than to issue tracking, and no LOCAL analogue is expected. Not filed.
> - GO `db_inode_lock`-equivalent OFD locking and DWS's `franken_sync/wal_index.rs` quarantine. I read wal_index.rs in full: it is a narrow, identity-bound rebuild of the *derived* WAL index for the GH#507 poison signature, wired into the open path (DWS src/franken_sync.rs:292), a read-only advisory (:165, :307, :586), a doctor detector (src/cli/commands/doctor.rs:1249) and a schema-migration gate (doctor_subsystems/schema_migration.rs:378). LOCAL has no franken_sync at all and runs fsqlite 0.1.7 vs DWS's 0.3. The storage-durability domain already filed the #507 poison as its critical finding and explicitly flagged reachability-on-0.1.7 as the open question; filing the recovery path separately would double-count the same wedge against an engine version where the poison may not be reachable. Deliberately not filed — noted here so a build-capable pass can settle it.
> - DWS's `model_based_storage.rs` differential/property test and `linearizability_multiprocess.rs` multi-process linearizability oracle. Both are excellent and both are DWS-only test files, but the testing-parity-docs domain already reported that LOCAL's test-file set is a strict subset of DWS's (33 DWS-only files, 0 LOCAL-only) and that these are absences of the same class. Filing them individually would restate a reported finding at a different granularity.
> 
> ADJACENCY DISCLOSURES, so the verifier can merge rather than double-count. Finding 3 (if_unchanged) is conceptually adjacent to model-lifecycle's row_version finding — that one is a wire-schema field, mine is a CLI flag plus MCP field plus storage-layer write-path guard plus a distinct exit code; the model-lifecycle domain explicitly verified DWS lacks `if_assignee`/`heartbeat`/`unclaim` and did not check `if_unchanged`, which DWS does have (37 code hits). Finding 2 (destructive guard) and finding 4 (acceptance items) both concern the `br update` flag surface that cli-ux-agent-contract diffed, but that domain compared flag *presence*; these two are flag *semantics* (what the write is allowed to destroy) and an item-level edit model, neither of which a presence diff surfaces. Finding 5 (worktree discovery) touches the same `sync/path.rs` module the sync-jsonl domain covered, but that domain's scope was merge/dedup/conflict-resolution, not path resolution.
> 
> ONE SYSTEMIC OBSERVATION about the sweep, which is the real answer to "what did it miss". All 93 findings are feature-by-feature comparisons within 12 declared domains, executed against a frozen LOCAL tree. None of them consulted DWS's changelog, which is a complete dated index of every post-fork addition, and none of them noticed that LOCAL and DWS are the same codebase at different commits. That is why all 7 of my findings are dws-only and why four of them (SIGPIPE, the destructive guard, if_unchanged, acceptance items) are agent-correctness features that all twelve domains could see only if they happened to compare the right file. If further passes are run, changelog-mining DWS v0.1.3 through v0.2.0 (CHANGELOG.md lines 1918-2030) and skimming CHANGELOG_RESEARCH.md (105KB, entirely unmined by anyone) is the highest-yield remaining action. GO's CHANGELOG.md (546KB) and the unreviewed docs/ subtrees (docs/multi-agent/, docs/recovery/, docs/core-concepts/) are the equivalent untapped sources on the GO side.

### B — `find:cli-surface`

> SCOPE AND METHOD. Read-only audit; no files written, no git mutations, no cargo/compilation. Command registration was extracted mechanically rather than by inspection: for LOCAL and DWS I brace-counted `pub enum Commands` in src/cli/mod.rs with a Python script (LOCAL src/cli/mod.rs:751, 65 variants; DWS src/cli/mod.rs:748, 51 variants) so nested enums (ConfigCommands, DepCommands, HistoryCommands, WorktreeCommand, etc.) could not contaminate the top-level set. An initial regex pass under-counted LOCAL at 67 and silently missed unit variants like `Where,` and `Sql(SqlArgs),` — the brace-counted extractor is the authoritative list. For GO I resolved cobra variable names to their `Use:` strings by parsing each `xxxCmd = &cobra.Command{...}` literal in cmd/bd/*.go (excluding *_test.go) and then intersected with every `rootCmd.AddCommand(&xxxCmd)` site, yielding ~120 top-level commands; a few needed manual resolution (rememberCmd -> `remember`, sendMetricsCmd -> hidden `metrics send`, serveCmd -> `serveCmdName`, migrateIssuesAliasCmd -> hidden deprecated `migrate-issues`). Wires FROM cmd/bd/*.go only — GO's non-CLI packages (internal/, cmd/ sub-binaries) were out of scope per the assigned domain.
> 
> COVERAGE GAPS AND UNVERIFIED ITEMS (deliberately NOT reported as findings).
> 1. Not verified: whether any LOCAL command provides a *behavioural* substitute for a missing GO command that I judged functionally covered. The clearest candidates I examined and accepted as covered: remember/recall/memories/forget (LOCAL `br memory`, src/cli/commands/memory.rs:1,46,86-91); comment singular (alias src/cli/mod.rs:784); onboard (alias on quickstart, :902); where (LOCAL top-level `Where`, :1095); sql (LOCAL `Sql(SqlArgs)`, :1097); swarm (LOCAL `Agents`, :3). I did NOT trace whether LOCAL's `br memory` persistence/eviction/TTL semantics match GO's, nor whether LOCAL `Agents` covers GO `swarm`'s full surface — only that a command exists under a different name.
> 2. Not verified: subcommand-level (second-level) flag diffs. I diffed flag surfaces for list, create, update, sync, show, close, search, doctor, dep, label, import, export. I did NOT systematically diff second-level subcommands (e.g. GO `dep` subcommands vs LOCAL `dep`, GO `doctor` subcommands vs LOCAL `doctor`'s 768KB / `doctor_subsystems/`, GO `config` subcommands vs LOCAL `config`, GO `audit` subcommands). LOCAL's doctor.rs is 768KB and was grepped only, never read whole, per instructions.
> 3. Not verified: global-flag parity beyond the specific six named in finding 11. I compared LOCAL's 14 root `Cli` fields against GO's `rootCmd.PersistentFlags()` block (main.go:883-902) but did not diff DWS's global flags at all.
> 4. Explicitly out of scope / not a gap: LOCAL-only extras (custom-status, custom-type, import, recipes, reflect, template, web, wisp, mol, federation, merge-slot, merge_slot, prime, prime-flag, formula, agents, coordination, worktree, gate, capabilities, prime, where, sql, admin). Per the task's definition these are not gaps. Note LOCAL has 9 top-level commands DWS lacks (CodexHook, Formula, Export, Hooks, Import, Mol, Prime, Quickstart, Recipes, Reflect, Template, Web, Wisp, CustomStatus, CustomType) — this is LOCAL being ahead, not behind.
> 5. Named suspects I checked and found to be NON-gaps: `upgrade` (present in all three: LOCAL src/cli/mod.rs:327, DWS :253, GO cmd/bd/upgrade.go); `mol`/`molecules` (present in LOCAL as `Mol`, :88, and in DWS and GO — but it appears in the GO-not-in-DWS list only because my DWS extraction predates nothing; DWS does have mol.rs, so DWS likely has it too — flag this as a probable extraction artifact); `wisp` (LOCAL has it, :281); `memory` (all three, namespaced differently); `list_fields` (DWS-only, `br list-fields` — I confirmed the DWS file src/cli/commands/list_fields.rs exists at 16KB but did NOT verify whether `br list` `--fields` in LOCAL covers its output; LOCAL's ListArgs DOES have a `fields` field, which may fully cover it, so I did not report it); `group` and `journal` — searched for in all three, no top-level command by either name in any codebase; no finding.
> 6. Known uncertainty: my GO top-level command extraction reads only `rootCmd.AddCommand` sites. If any GO command is registered by a different mechanism (plugin loading, a slice iterated at init) it would be missed. I checked `rg -n 'AddCommand' cmd/bd/*.go | grep -v rootCmd` and the non-root hits were all subcommand attachments to already-registered parents (e.g. commentsCmd.AddCommand at comments.go:297-298), so this risk is low but not zero.
> 7. `metrics`/`telemetry`: confirmed absent from LOCAL (`rg -ci 'telemetry' src/cli/` -> 0) and from DWS, but folded into the long-tail finding (12) rather than given its own entry, because GO's implementation is explicitly an internal hidden flush helper (cmd/bd/send_metrics.go:12 `Hidden: true`, `Short: "Internal: flush queued telemetry events (spawned by bd)"`) — a user-facing metrics surface would be a different finding and I did not find one.
> 8. No LOCAL doc explicitly declares any of these gaps intentional. I searched CHANGELOG.md, UPGRADE_LOG.md, and docs/ (including docs/porting/) for 'capacity', 'vcs-status', 'parity', 'not ported', 'out of scope' — the only hits are generic 'parity' references in changelog entries about ID-hash and error parity (CHANGELOG.md:143,317,791,792). Consequently no finding was downgraded to severity 'info' on the "documented as intentional" rule. In particular docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:6884 does document one Go command as deliberately unported (`daemon`, "daemon not supported") — that one is correctly absent from LOCAL and is not a gap.
> 9. Severity calibration: findings 5 and 6 are marked 'high' on user-visible capability loss, but finding 5 (Dolt family) is absent from DWS as well and is plausibly a deliberate SQLite-only architectural choice — a reviewer may prefer 'medium' there. Finding 4 (`create --graph`) is 'high' on atomicity grounds but is GO-only, not a DWS regression. I ranked the agent-ergonomics flag findings (1-4) above the whole-subsystem findings (5-6) because br's stated purpose is agent-first selection, and flag gaps degrade that on every invocation while whole-subsystem gaps affect narrower workflows.

### B — `find:cli-ux-agent-contract`

> METHOD / SCOPE. Read-only audit of the cli-ux-agent-contract domain across LOCAL (/Users/tranquangdang21/Projects/beads_rust, v0.1.3), DWS (/tmp/beads_gap_audit/dicklesworthstone_beads_rust, v0.6.0), GO (/tmp/beads_gap_audit/gastownhall_beads). No files written, no git mutations, no cargo invoked. Every local-side claim was verified with an explicit rg over src/ and re-inspected where the first regex returned a non-zero count that could have been a false positive (three such cases were checked individually and turned out unrelated: the 8 `pub all: bool` hits are command-scoped --all flags not a help flag; the 2 config-validate hits are `br formula validate` and a test name; the 1 readonly/sandbox hit was a `read_only` lock-classification helper). LOCAL src/cli/commands/doctor.rs is 768KB and was never read whole, only grepped; it was excluded from the report as out-of-domain (doctor surface).
> 
> CONFIRMED PARITY (not gaps, recorded so they are not re-investigated):
> - OutputMode enum is byte-identical between LOCAL and DWS: both are `Rich | Plain | Json | Toon | Quiet` (LOCAL src/output/context.rs:31-42, DWS src/output/context.rs:31-42). TOON is a LOCAL/DWS extension GO lacks, i.e. an extra, not a gap.
> - Shell completions: LOCAL and DWS both support bash, zsh, fish, powershell (alias pwsh), elvish (src/cli/mod.rs:1186-1199 LOCAL, :1023-1036 DWS) and both use clap_complete's dynamic `COMPLETE` registration (src/cli/commands/completions.rs:54-69 in both). GO relies on cobra's `__complete` with a live-store ID completer (cmd/bd/completions.go:15-57) and gains no shell GO has that LOCAL lacks. LOCAL is ahead on elvish.
> - Config file format and layering are at parity: both use YAML, `.beads/config.yaml` for project and `~/.config/beads/config.yaml` (GO falling back to `~/.config/bd/config.yaml`, LOCAL src/config/mod.rs:6-7) plus a legacy `~/.beads/config.yaml` layer (LOCAL :3610-3614, GO internal/config/config.go:66-69). GO's extra `BEADS_DIR/config.yaml` layer above the project layer (GH#2375) is not a LOCAL gap because LOCAL resolves the project config path from the discovered beads_dir, which BEADS_DIR already selects (src/config/mod.rs:226-229).
> - `br capabilities` publishes an exit-code contract in both LOCAL (src/cli/commands/capabilities.rs:22,298) and DWS (:25,316). LOCAL's doctor exit-code taxonomy (src/cli/commands/doctor_subsystems/exit_codes.rs:51-74, values 0-6 plus sysexits 64/66/73/74) has no GO counterpart at all and is ahead of GO.
> - Export/format parity favors LOCAL: LOCAL ExportFormat is jsonl|json|csv|obsidian with `obsidian|md|markdown` aliases (src/cli/mod.rs:1857-1882), whereas GO's `bd export` is JSONL-only with a separate obsidian path (cmd/bd/export.go:24-25, :67-75; cmd/bd/export_obsidian.go). LOCAL src/format/markdown.rs (546 lines) is larger than GO internal/uimd/markdown.go (~250 lines).
> - `robot-docs` exists in both LOCAL and DWS with the same `br.robot_docs.v1` contract_version and the same JSON/ToON/Text dispatch; LOCAL's CANONICAL_COMMANDS list is longer (adds `br prime` and `br reflect --json`), so it is ahead, not behind.
> - `--robot` as a JSON alias is present in both (LOCAL src/cli/mod.rs:1746-1748, 2534-2535 plus the resolver at :1951-2053; DWS :1494-1496, 1559-1562).
> - GO's central server config (internal/configfile/central_config.go, `~/.config/beads/server.json`) and its credentials/permissions/repos modules are Dolt-server and enterprise-shape configuration; per the audit rules these are info at most and are not listed as findings.
> 
> NOT FULLY VERIFIED / DELIBERATELY NOT CLAIMED:
> - GO's `bd help <topic>` prose pages (e.g. `bd help init-safety`, referenced in cmd/bd/errors.go's init-safety contract comment) are embedded prose supplements under cmd/bd/help_supplements/ (1 file, create_graph_plan.md). I verified the mechanism and its registration but did not audit the prose content, so finding 5 is scoped to the mechanism, not the prose volume.
> - GO's BD_AGENT_MODE is referenced in only 4 files and BD_NON_INTERACTIVE in 19. I confirmed both exist and how they gate rendering/prompts but did not enumerate every call site, so finding 7's impact is about the env surface LOCAL does not honor, not about the breadth of GO's use.
> - GO's OTEL telemetry surface (internal/telemetry/ ~20 files, cmd/bd/command_telemetry.go, telemetry_redact.go; env BD_OTEL_ENABLED / BD_OTEL_LOGS_URL / BD_OTEL_METRICS_URL / BD_OTEL_STDOUT) is a real config-surface difference — neither LOCAL nor DWS has any otel/opentelemetry reference (`rg -i 'otel|opentelemetry' src/` -> 0 hits in both) — but it is observability rather than the CLI machine-facing contract, so it is recorded here rather than occupying one of the 12 slots.
> - GO's `capability_registry.go` (one table for every command path plus a coverage test) was not compared against LOCAL's `br capabilities`, because GO's registry governs proxied-Dolt-server gating, which is a storage/backend concern rather than a CLI contract concern. LOCAL's capabilities.rs (39KB) vs DWS's (46KB) size difference was observed but not investigated.
> - GO's OSC-8 hyperlink emission (`ShouldUseHyperlinks`, internal/ui/terminal.go:60-121) is bundled into finding 7 but not separated out; it is a human-terminal nicety, not a contract concern.
> - LOCAL's `br schema` `Commands` target and DWS's are structurally identical apart from DWS's two extra schema targets (`AdditiveReconciliation`, `VcsStatus`), which track DWS-only subsystems. I did not verify whether the additive-reconciliation receipt shape is reachable in any DWS command a user could otherwise run against LOCAL data, so it is not reported as a gap.
> - Completion *quality* (dynamic value completers for issue IDs, labels, assignees, config keys) exists in both LOCAL and DWS and was not compared function-by-function; only shell coverage was compared.

### B — `find:graph-deps`

> SCOPE / METHOD: read-only three-way diff; no file written, no cargo, no mutating git. LOCAL doctor.rs (768KB) was never read whole; all doctor-adjacent claims come from targeted rg.
> 
> CONFIRMED NOT-GAPS (LOCAL is ahead — explicitly excluded per the brief): DWS has no mol.rs / formula.rs / wisp.rs / memory.rs / admin.rs / template.rs / reflect.rs / merge_slot / federation / import / rename / rename-prefix / quickstart / recipes / worktree / sql / prime / lint / stale / export / hooks / codex-hook; LOCAL has all of them. LOCAL also leads on: (a) DependencyType vocabulary — LOCAL src/model/mod.rs:314-355 has 18 built-in types (AuthoredBy, AssignedTo, ApprovedBy, Attests, Tracks, Until, Validates, DelegatedFrom, RepliesTo, Duplicates) vs DWS's 10 (src/model/mod.rs:272-286); (b) `br gate add-waiter/remove-waiter/list-waiters` (src/cli/mod.rs:2488-2510) which DWS lacks; (c) `br ready --claim` and `--gated` (src/cli/mod.rs:3113-3122) which DWS's ReadyArgs lacks; (d) `epic status --eligible-only` / `close-eligible --dry-run` are at parity, not a gap.
> 
> VERIFIED-AT-PARITY (checked and found equal, so not reported): ReadyFilters struct (LOCAL src/storage/sqlite.rs:11043-11077 vs DWS :18544-18578) is field-for-field identical including parent_member_ids pre-resolution; ReadySortPolicy; the blocked-cache architecture (full/incremental/deferred refresh plan, self-healing recover_blocked_ids) and the blocking type set {blocks, parent-child, conditional-blocks, waits-for} (LOCAL src/storage/schema.rs:150-152, DWS :369-371); the partially-indexed idx_dependencies_blocking.
> 
> NOT REPORTED / RAN OUT OF CONFIDENCE: (1) GO's `--max-rows` defensive row cap (cmd/bd/max_rows.go:25, registered at cmd/bd/graph.go:364 and cmd/bd/ready.go:794) is absent from LOCAL *and* DWS — `rg -n 'max_rows' src` returns 0 files in both. GO-only, but it is a hardening/DoS guard rather than a graph capability, so I left it here rather than spending a finding slot. (2) GO's `graph apply` (cmd/bd/graph_apply.go, 50KB) and `bd recompute-blocked` (cmd/bd/recompute_blocked.go) — I confirmed they exist and are large, but I did not verify their user-visible contract closely enough to characterize the gap without guessing; LOCAL's `sync --rebuild` and its self-healing blocked cache may already cover the recompute case, so this needs a follow-up pass. (3) GO `dep list` accepts multiple issue IDs (`Use: "list [issue-id...]"`, cmd/bd/dep.go:900) while LOCAL DepListArgs takes a single `issue`; I did not confirm whether GO's multi-ID form is reachable in a way that matters, so it is not reported. (4) GO's molecule/molecule-ready system (internal/molecules, cmd/bd/mol_*.go) overlaps LOCAL's mol.rs but was not audited — assigned-domain note says LOCAL is ahead there and the brief said not to report it. (5) I did not verify GO's `list_tree_deps.go` / `create_deps.go` user-visible surfaces against LOCAL's `list --tree`.

### B — `find:integrations-external`

> SCOPE AND METHOD. I audited external system integrations, network surfaces, and multi-repo features across LOCAL (beads_rust 0.1.3), DWS (Dicklesworthstone/beads_rust 0.6.0), and GO (gastownhall/beads, binary `bd`). I never read LOCAL src/cli/commands/doctor.rs (768 KB) whole; I used targeted rg. All three trees were treated read-only; no cargo, no mutating git.
> 
> VERIFIED PRESENT IN LOCAL (so NOT reported as gaps, per the "LOCAL having EXTRA features" rule): `br web` embedded Next.js SPA + axum REST API (Cargo.toml `default = ["web"]`); `br serve` MCP stdio server (src/mcp/); `br hooks install/list/run` with 5 managed git hooks (src/hooks/mod.rs); AES-256-GCM credential encryption at rest (src/util/credentials.rs); 18 IDE/agent integration recipes (src/recipes/mod.rs) which are at parity with GO's docs/integrations/ guides — that is why I filed no IDE-integration gap. LOCAL's AES at-rest credential encryption is arguably stronger than anything in GO's internal/creds; the real gap is only the resolution ladder, which I did file.
> 
> COULD NOT FULLY VERIFY / DELIBERATELY EXCLUDED. (1) GO's remote Dolt backend cluster (internal/doltserver 1.4 KB dir, internal/doltremote, internal/doltserver, cmd/bd/dolt_*.go, remotecache) is a storage-backend capability rather than an external *integration*; most of it also depends on cgo builds (`//go:build cgo` at cmd/bd/federation.go:1) and I did not trace how much of it is reachable in a default build, so I did not file it. (2) GO's integrations/beads-mcp is a separate 51 KB Python MCP server, not a `bd` capability — I mention it only in passing. (3) I did not audit the GO httpapi route-by-route against LOCAL's web API for per-endpoint semantic differences beyond the auth and stub findings; a full route diff would need parsing internal/httpapi/spec/ and is out of scope for this domain pass. (4) I did not compare GO's spec_parity_test.go (74 KB) guarantees to any LOCAL test.
> 
> A DOCUMENTATION DEFECT worth surfacing to the operator independent of the code gaps: docs/BD_VS_BR.md is stale in two ways. Line 75 and line 87 both list Federation as `br`-exclusive with `bd` having no equivalent, but GO ships cmd/bd/federation.go (14 KB) with a real Dolt-remote sync. Line 14 lists "RPC daemon | `bd daemon rpc`, `bd daemon start`" as the excluded GO feature, but `ls cmd/bd/ | grep -ci daemon` returns 0 — GO replaced the daemon with `bd serve`, which LOCAL also lacks. The doc therefore simultaneously understates GO and overstates LOCAL.
> 
> SEVERITY RATIONALE. I set the external-tracker finding to `high` rather than `info` despite LOCAL documenting it as out-of-scope, because the audit rules say to use `info` when LOCAL documents the gap as intentional AND the rationale still holds. The rationale at docs/BD_VS_BR.md:9 is "Agents use MCP / APIs directly; `bd` shell-outs are brittle" — but GO no longer shells out; it uses native in-process HTTP clients (internal/linear/client.go 41 KB with OAuth at oauth.go, internal/gitlab/client.go 22 KB, internal/github/client.go 12 KB with client_ratelimit_test.go) behind a typed interface. The stated reason no longer describes upstream behavior, so I treated it as an unclosed gap rather than a conscious omission. I flagged this reasoning explicitly in that finding's `impact` so a reviewer can downgrade it if they disagree.
> 
> HIGHEST-VALUE UNFINISHED WORK. The `br web` auth finding is the one I would fix first: it is a live, default-enabled, network-reachable, unauthenticated mutating API, and LOCAL's own docs assert the opposite design stance ("`br serve` uses MCP over stdio (no network daemon)", docs/BD_VS_BR.md:14). The gap is small to close relative to its blast radius — a Host allowlist plus refusing non-loopback binds without a token, both of which GO already implements in ~30 lines (internal/httpapi/server.go:451, :1487-1500).

### B — `find:mcp-agent-surface`

> SCOPE AND METHOD. Read-only throughout: no writes, no git mutations, no cargo/compilation. LOCAL src/mcp/ is 8186 lines (mod.rs 782, tools.rs 5130, resources.rs 1365, prompts.rs 909); DWS is 10342 (mod.rs 1706, tools.rs 6105, resources.rs 1614, prompts.rs 917). GO has no Go MCP server — `find integrations/beads-mcp -name '*.go'` returns nothing. GO's MCP is a **Python** package (`integrations/beads-mcp/src/beads_mcp/`, server.py 51KB, tools.py 27KB) that shells out to `bd` via bd_client.py; `bd serve` (cmd/bd/serve.go:57) is a separate HTTP/JSON-RPC server, not MCP. I treated the Python package as GO's MCP surface per the assigned domain.
> 
> SET DIFFERENCE (the "substantial tool-count gaps" the brief anticipated are mostly in GO, not DWS). Tools: LOCAL 7, DWS 7, GO 18. LOCAL and DWS have the *identical* seven names (list_issues, show_issue, create_issue, update_issue, close_issue, manage_dependencies, project_overview) — no tool is missing on the Rust side, and both deliberately sit at the "≤ 7 tools per cluster" ceiling stated in tools.rs:1-3. The DWS divergence is entirely in schema depth and error/transport plumbing, which is why the 8 DWS-only findings above are schema, contract, and architecture gaps rather than missing tools. GO's 18 are: discover_tools, get_tool_info, context, ready, list, show, create, claim, update, close, reopen, dep, comment, comments, note, stats, blocked, admin. Resources: LOCAL 12, DWS 12, GO 1 (beads://quickstart, server.py:326) — LOCAL is richer here, not a gap. Prompts: LOCAL 4, DWS 4, identical names and arguments (triage/focus, status_report/period, plan_next_work/goal, polish_backlog/focus); GO 0. Pagination: none in all three (`cursor|next_cursor|page_token|offset` -> 0 hits in LOCAL src/mcp/ and in GO server.py), so this is a shared limitation, not a gap. `output_schema` is `None` on all 7 tools in BOTH LOCAL and DWS, so the newer framework did not add output schemas — I dropped that as a candidate. LOCAL and DWS `project_overview_json` emit an identical key set (project, issue_prefix, beads_dir, total, counts, active, in_progress, blocked, blocked_by, blocked_issues, ready, ready_issues, deferred, dirty_unsaved, top_labels, id, title, status, label, count) — not a gap.
> 
> FRAMEWORK VERSION IS THE ROOT CAUSE. LOCAL resolves fastmcp-rust 0.3.2 (Cargo.toml:111 asks for 0.3.1; CHANGELOG.md:247 confirms the 0.3.1 bump) and uses the legacy `Server::new(...).run_stdio()` API. DWS pins `=0.10.0` and uses `fastmcp_rust::modern::ServerBuilder` plus `run_transport_returning_with_cx`. The `modern` module and the `CompleteResult` / `FinalCallToolResult` / `ResultMeta` / `ContentBlock` types that findings 1 and 5 depend on are not importable under 0.3.2. I could not confirm the 0.3.2 API surface directly: no vendored copy or `~/.cargo/registry/src/*/fastmcp-rust-*` exists on this machine, and I was not permitted to run cargo. The claim "LOCAL does not use these types" is verified by grep (0 hits); the claim "0.3.2 cannot express them" is inferred from LOCAL's own source and is the one inference in this report I could not close by reading the dependency.
> 
> UNVERIFIED, DELIBERATELY NOT REPORTED AS A FINDING — resource template/exact URI collision. DWS src/mcp/mod.rs:1679 says fastmcp "rejects overlapping exact/template registrations" and therefore moved the individual-issue resource to the singular namespace and registered it LAST. LOCAL registers `beads://issues/{id}` SECOND (src/mcp/mod.rs:757), before the five exact `beads://issues/*` collections (ready, blocked, in_progress, deferred, bottlenecks). If 0.3.2 resolves templates before exact matches the way 0.10.0 evidently does not, then reads of beads://issues/ready, /blocked, /in_progress, /deferred and /bottlenecks would be shadowed by the template matching with id="ready" etc., returning a not-found error instead of the collection. I could not test this: it requires building the `mcp` feature and speaking JSON-RPC, both forbidden here, and LOCAL has no protocol-level test (finding 6) that would have caught it. This is the highest-value item for a follow-up with a build available. Note that 0.3.2 evidently does NOT hard-reject, since LOCAL registers without error and ships — so the failure mode would be silent misrouting, not a startup crash.
> 
> NOT REPORTED AS GAPS, with reasons. (a) LOCAL's 7-tool ceiling versus GO's 18 is a deliberate, documented design constraint (tools.rs:1-3 "≤ 7 tools per cluster"), so I did not file "missing claim/reopen/note/comments/stats tools" — each GO-only action is reachable in LOCAL through an existing tool (reopen via update_issue status, note via the `comment` property, comments via show_issue, stats via project_overview) or, for claim, through an explicitly non-atomic two-field update. The genuinely unreachable GO capabilities are filed as findings 10-12 instead. (b) LOCAL's `manage_dependencies` is arguably richer than GO's `dep`: it supports seven dep types (blocks, related, parent-child, waits-for, duplicates, supersedes, caused-by) with alias auto-correction and `external:<project>:<capability>` cross-project targets (src/mcp/tools.rs:2726) — an extra, not a gap. (c) LOCAL's `mcp` cargo feature is non-default in both Rust repos (LOCAL default = ["web"], not the ["self_update"] that AGENTS.md claims — a doc inaccuracy outside my domain, noted only in passing), so feature-gating is at parity. (d) I found no LOCAL CHANGELOG.md, UPGRADE_LOG.md, or docs/ statement declaring any MCP-surface gap intentional, so no finding is downgraded to info on that basis; the only near-misses are docs/COORDINATION_EVIDENCE.md:99 and :190, which describe *future* work and do not disclaim current capability.
> 
> CONFIDENCE. Findings 1-8 (DWS-only) rest on direct side-by-side reads of corresponding source and are high confidence; the local_evidence for each is an explicit negative grep with a hit count, and the upstream_evidence cites line numbers in both handler schema and, where DWS has one, its regression test. Findings 9-12 (GO-only) rest on GO's own Python source and the same style of negative grep against LOCAL. The single soft spot across all twelve is the fastmcp 0.3.2 API-surface inference noted above, which affects only the effort estimate for finding 1 and the cause attribution in finding 5, not their substance.

### B — `find:model-lifecycle`

> SCOPE / METHOD. Read-only audit; no files written, no cargo/cargo-build, no mutating git. Repos read: LOCAL /Users/tranquangdang21/Projects/beads_rust (Cargo version 0.1.3), DWS /tmp/beads_gap_audit/dicklesworthstone_beads_rust (0.6.0), GO /tmp/beads_gap_audit/gastownhall_beads. Note: the GO path in the task brief was given as /tmp/beads_gap_audit/gastownhall/beads; the actual on-disk path is /tmp/beads_gap_audit/gastownhall_beads.
> 
> ISSUE STRUCT DIFF (exhaustive, done first). Extracted every `pub <field>:` from the LOCAL Issue struct (src/model/mod.rs:853-1010, 63 fields) and the DWS struct (src/model/acceptance/ + src/model/mod.rs:462-700, 45 fields), and every field of the GO struct (internal/types/types.go:20-190, 67 fields), then diffed. Upstream-only fields: actor, bypass_reason, bypassed_policy, compacted_at, compacted_at_commit, compaction_level, heartbeat_at, id_prefix, is_blocked, is_lite_partial, lease_expires_at, lease_granted_node, original_size, policy_gates_fired, prefix_override, prerequisites, row_version, storage_class, timeout, wisp_plane_override. LOCAL-only fields (EXTRA capability, explicitly NOT reported as gaps per the brief): agent_state, crystallizes, holder, hook_bead, points, quality_score, rig, role_bead, role_type, timeout_seconds.
> 
> CANDIDATES EXAMINED AND DELIBERATELY NOT REPORTED (with reasons, so they are not re-litigated):
> - bypassed_policy / bypass_reason / policy_gates_fired — LOCAL *has* these, just modelled as a separate `close_metadata` table (src/storage/schema.rs:269-278,1656-1665) rather than as Issue columns, with a CLI field at src/cli/mod.rs:3244. 43 hits in src/. Not a gap.
> - compaction_level / compacted_at / compacted_at_commit / original_size — LOCAL removed them on purpose: src/storage/sqlite.rs:634-635 "Then 46->42 after removing compaction_level, compacted_at, compacted_at_commit, original_size (beads_rust)". DWS keeps the columns but every DWS CLI call site hardcodes `compaction_level: None` (src/cli/commands/*.rs, ~20 sites), i.e. no DWS command drives them either. Not user-visible in either. Info-only, folded out of the finding list.
> - is_blocked (GO), persisted readiness projection — LOCAL computes readiness live via SqliteStorage::is_blocked (src/storage/sqlite.rs:5959) and has a blocked cache; GO persists the projection only so journal snapshots can replay graph deltas. LOCAL has no journal layer, so the projection has no consumer. Not user-visible.
> - storage_class (GO) — 0 hits in LOCAL, but it is a Go storage-plane routing marker tied to the wisps-table/GC and dolt-replication design; no GO CLI surface reads it as a user-facing concept beyond create-time selection. Would report as architecture_gap at most; left out for volume.
> - id_prefix / prefix_override / wisp_plane_override / is_lite_partial (GO) — explicitly documented internal Go plumbing with no user-visible behavior (types.go:60-62, :100-107, :181-190). Correctly excluded by the brief.
> - `bd kv` custom key/value store (GO cmd/bd/kv.go:141-297, internal/storage/kvkeys/) — initially a candidate, but LOCAL's `br config get/set/delete/list` (src/cli/mod.rs:3461-3495) is a genuine functional substitute for a workspace-scoped user KV store. LOCAL's per-issue `Issue.metadata: Option<String>` (src/model/mod.rs:1030-1032) plus `--metadata` on create and `--metadata key=value` filters (src/cli/mod.rs:2183,2327) cover the per-issue case. Dropped.
> - `bd duplicate` / `bd supersede` — retained but downgraded to info because LOCAL's porting scope doc lists them as excluded (see finding 8).
> - GO issue_roles_external_test.go (30KB) — read the head: it tests Go storage-role *interface segregation* (ReadyClaimer, BatchCloser decorator layering under telemetry, typed ErrUnsupported). It is not a user-facing roles/permissions model. The brief's "roles and permissions" question resolves to: neither GO nor DWS nor LOCAL has an RBAC model. No finding.
> - Status / IssueType / Priority enums — diffed all three. LOCAL Status (src/model/mod.rs:49-67) = Open, InProgress, Blocked, Deferred, Draft, Closed, Tombstone, Pinned, Hooked + Custom(String); GO AllStatuses (types.go:522-525) = open, in_progress, blocked, deferred, closed, pinned, hooked. LOCAL is a superset (extra Draft, Tombstone). LOCAL IssueType (src/model/mod.rs:191-210) = Task, Bug, Feature, Epic, Chore, Docs, Question, Decision, Message, Molecule, Gate, Spike, Story, Milestone, Event + Custom; GO AllIssueTypes (types.go:744-747) = bug, feature, task, epic, chore, decision, message, molecule, gate, spike, story, milestone. LOCAL is a superset (extra Docs, Question). No gap.
> - Custom statuses / custom types — both LOCAL and DWS and GO support them. LOCAL wires StatusCommands + TypeCommands into the CLI at src/cli/mod.rs:1037 and :1045 (custom_status.rs, with --category active/wip/done/frozen). GO uses `config set status.custom` + `types.custom` with typed CustomStatus (types.go:551-554). No gap.
> - Soft vs hard delete — LOCAL `br delete` creates a tombstone and supports `--hard` (purge all tombstones / purge individual) at src/cli/commands/delete.rs:100-142, :345, plus `--force` and `--cascade`. Matches GO's `bd delete` + `bd purge`. No gap.
> - Multi-id bulk close/update — LOCAL already has `pub ids: Vec<String>` on close (src/cli/commands/close.rs:29) and on 8 arg structs (src/cli/mod.rs:1442, 1579, 2426, 2964, 2986, 3016, 3196, 3252). No gap (the *transactional cross-command* batch gap is reported separately, finding 4).
> - Hierarchy (epic/parent/child) — LOCAL has `Br` Epic subcommands, the `parent-child` DependencyType (src/model/mod.rs:318), `br dep tree` and `br dep cycles` (src/cli/mod.rs:2455,2457). GO's `bd children` (cmd/bd/children.go:12) is a thin 'List child beads of a parent' wrapper over the same dep query, and `bd flatten` (cmd/bd/flatten.go:20) is Dolt *history* squashing, not hierarchy. No meaningful gap.
> - DWS is a port that itself lacks GO's lease/heartbeat/unclaim/if-assignee/batch (verified: `rg` on DWS src/cli/mod.rs returns 0 for Batch, unclaim, heartbeat, if-assignee, and 0 hits repo-wide for lease_expires/heartbeat_at/if_assignee). So findings 1, 2, 4, 6, 7 are GO-only by construction and correctly labelled present_in: go-only; finding 3 and 5 are dws-only.
> 
> CONFIDENCE. Findings 1, 2, 3, 4, 5, 6 are backed by an exact 0-hit exhaustive search of LOCAL src/ + tests/ (and docs/ where noted) paired with a named upstream file:line that defines the feature. Finding 7 (row_version) is a library-surface gap: it is absent from br's wire schema, but br is a single-writer local CLI rather than a multi-writer Go library, so the practical severity is the lowest of the medium group. Finding 8 is info by rule.

### B — `find:observability-ops`

> SCOPE COVERAGE. Audit ran read-only; no files were written, edited, or deleted, and no git/cargo command was run.
> 
> ARENAS PROBED AND FOUND AT PARITY (not reported as gaps):
> - `audit` command group: LOCAL's AuditRecordArgs/AuditCoordinationArgs/AuditLabelArgs/AuditLogArgs/AuditSummaryArgs (src/cli/mod.rs:2803-2878) are field-for-field identical to DWS's (src/cli/mod.rs:2465-2540), and both cover GO's cmd/bd/audit.go surface (record with --kind/--model/--prompt/--response/--issue-id/--tool-name/--exit-code/--error/--stdin, label with --label/--reason). LOCAL additionally has an `audit coordination` subcommand GO lacks.
> - doctor exit-code dictionary: `diff src/cli/commands/doctor_subsystems/exit_codes.rs` LOCAL vs DWS differs in exactly 2 lines, both a doc comment and a call target (std::process::exit vs crate::shutdown::exit_process). Same 11 codes.
> - doctor run-directory / mutate chokepoint / capabilities envelope / surface modules: present in both, surface.rs is near-identical (diff of dotted check names is 4 lines, all data file names).
> - doctor subcommand ergonomics: Capabilities / RobotDocs / Health / Ls / Undo / Explain exist in both. LOCAL is actually AHEAD on one: it has --allow-warnings (src/cli/mod.rs:3653-3658, "Exit 0 when findings are only warnings") which DWS lacks entirely. Not a gap.
> - logging: LOCAL's src/logging.rs (RUST_LOG-driven EnvFilter, stderr fmt layer, optional JSON file layer, per-verbosity fsqlite noise damping) and DWS's 6.6KB src/logging.rs are equivalent. Neither exposes a --log-file CLI flag, so there is no destination gap.
> - audit log storage: LOCAL src/storage/events.rs (925 lines) and DWS are architecturally equivalent append-only tables.
> 
> COULD NOT FULLY VERIFY / DELIBERATELY OUT OF SCOPE:
> - GO internal/fdhygiene (MarkInheritedCloexec) — a real Go-runtime fd-leak hardening helper, but it exists only to stop descriptors leaking into GO's detached Dolt sql-server and dbproxy children. LOCAL has no such child to protect, and the domain brief says Go-internal implementation details with no user-visible behavior are not gaps. Noted, not reported.
> - GO doctor "Circuit Breaker" stale-file check (cmd/bd/doctor/circuit.go) — the closest LOCAL analogue is src/util/circuit_breaker.rs:298-320, which DOES write `/tmp/beads-circuit/beads-circuit-{id}.json` via PersistentCircuitBreaker, exactly the file class GO scans. But I found zero references to PersistentCircuitBreaker or BREAKER_DIR anywhere in LOCAL outside its own defining module, so the type is currently unexercised dead code. The gap is real but latent/unverified as user-visible, so I left it here rather than asserting it. LOCAL does have a `daemon.pid` string (1 hit) but that is unrelated.
> - GO doctor --server, --migration, --orchestrator, --agent, --fix-child-parent and the entire cmd/bd/doctor/fix Dolt-specific fixers (database_integrity.go, migrate.go, repo_fingerprint.go, dolt_format.go) — all gated on GO's Dolt storage backend, which LOCAL does not have (SQLite/fsqlite). Deliberately not reported; these are backend-port artifacts, not missed capabilities.
> - GO --agent diagnostic mode — I did not verify whether LOCAL's --robot-triage (br.doctor.triage.v1 mega-envelope) is a genuine equivalent. It emits summary + findings + planned actions + a recommended command in one JSON read, which appears to cover the stated purpose, but I did not diff the two payloads field by field. Flagging as unverified.
> - GO internal/audit (5.5KB) — its Entry struct and AppendIfEnabled gate are fully covered by LOCAL's audit.rs; no gap.
> - DWS-only files I classified as OUT OF DOMAIN for other agents: src/cli/commands/capacity.rs (17KB, queue-capacity planning — it mentions "metrics" only in prose, 1 hit); src/cli/commands/lint.rs size difference (35KB DWS vs 22KB LOCAL) and src/cli/commands/list_fields.rs + search/ subdir — these look like the validation and query domains respectively, not observability-ops. I did not audit them.
> - DWS doctor_subsystems/run_dir.rs is 34KB vs LOCAL 28KB and DWS doctor.rs is 1002KB vs LOCAL 768KB; I diffed the detector/fixer registries and surface.rs, but did not do a line-by-line read of the ~232KB of doctor.rs delta (a full-file diff was out of budget and doctor.rs must not be read whole). The 4-detector and filter_ids findings are the registry-level residue I could verify; smaller behavioral divergences inside that delta may exist and are unverified.
> - LOCAL CHANGELOG.md and UPGRADE_LOG.md contain no statement about telemetry, metrics, OTEL, selftest, bundle, or the events journal, so none of the missing-observability findings carry a 'LOCAL consciously chose not to port this' justification. The single exception is docs/reliability/HEALTH_CONTRACT.md:143, which records the --bundle gap as '(Not yet implemented - tracked for future work.)'. I kept that finding at high rather than info severity because it is a tracked backlog item the upstream closed, not a deliberate non-port; the doc text is quoted in the finding's local_evidence either way.
> 
> COUNTING NOTES. Doctor check-name counts quoted in findings come from two different sources and should not be mixed: the raw string scan (LOCAL 153 vs DWS 199 unique dotted names) over-counts because it sweeps file names and path fragments; the authoritative number is the DETECTOR_ROWS table (LOCAL 54 vs DWS 58), which is what `br doctor capabilities` publishes. Findings use the latter.

### B — `find:query-search`

> Scope: LOCAL=/Users/tranquangdang21/Projects/beads_rust (br, v0.1.3), DWS=/tmp/beads_gap_audit/dicklesworthstone_beads_rust (v0.6.0), GO=/tmp/beads_gap_audit/gastownhall_beads (bd). All read-only; no files edited, no cargo/git mutation.
> 
> VERIFIED-NOT-GAPS (checked and found at parity or LOCAL better):
> - Full-text engine: NONE of the three use FTS5/BM25/snippet/MATCH (rg -rl 'fts5' and 'bm25' return 0 real hits in all three; DWS's only 'fts5' string is a test fixture "v USING f"). No semantic/vector/embedding search in any of the three. So 'LOCAL lacks FTS/semantic search' is FALSE.
> - Grouping/aggregates: no --group-by/SUM/COUNT/aggregate in any list/search/query command in any of the three. Parity.
> - Query DSL core: token set, precedence (OR<AND<NOT), operators (=,!=,<,<=,>,>=), and ~20 fields are a faithful port. LOCAL has the DSL wired to `--filter`/`-F` on list/search; DWS has NO query DSL at all (no src/query, no --filter). GO runs the DSL via a standalone `bd query <expr>`. So LOCAL has DSL parity-or-better vs both upstreams.
> - Saved queries: present in BOTH LOCAL (query save/run/list/delete, src/cli/commands/query.rs:297-300) and DWS (src/cli/commands/query.rs SavedQuery). GO's `query` is a plain DSL runner, not saved-query mgmt. Not a gap for LOCAL.
> - `query` command surface divergence: GO's `bd query <expr>` is a DSL entry point (with --sort/--limit/--all/--long); LOCAL's `br query` is saved-query management only, but LOCAL runs the same DSL via `br list --filter "<expr>"`. Capability parity via a different surface — reported here, not as a hard gap.
> - Pagination (limit/offset): present in all three. LOCAL's list JSON includes total/has_more; only LOCAL's search omits it (finding #3).
> - Negative exclude filters --exclude-label/--exclude-type: reachable in LOCAL via `NOT label=x` / `NOT type=x` in the DSL (evaluator Not handling, evaluator.rs:119-121,597-599) — deliberately excluded from finding #9 to avoid overclaiming.
> - closed-date range: reachable in LOCAL via the DSL `closed`/`closed_at` field (evaluator.rs:375,579) even without a --closed-after flag — deliberately excluded from finding #6.
> - LOCAL extra features (not gaps): owner/pinned/mol_type/unassigned/metadata.* /has_metadata_key DSL fields, and --filter itself, have no DWS equivalent.
> 
> UNVERIFIED / COVERAGE LIMITS (not asserted as findings):
> - I did not benchmark search performance; the Unicode and comment-search findings are correctness/feature gaps, not perf claims.
> - I did not exhaustively read LOCAL's 768KB doctor.rs (per instructions); a check for a hidden doctor-side search field projection is possible but not done.
> - GO's readiness/where/q commands were checked only for filter-surface parity at the flag level (no hidden flag found in LOCAL for due/defer/absence/regex/case), not re-implemented line-by-line.
> - The rg tool display mangled some literal substrings during this session; every finding's positive evidence was re-verified with a second, non-mangled grep/count before inclusion.

### B — `find:storage-durability`

> SCOPE COVERED. Four findings reported, all with upstream file:line and LOCAL zero-hit local_evidence. Severity ladder: 1 critical, 2 high, 1 medium. No speculative findings included.
> 
> NOT A GAP (verified and excluded, so downstream agents do not re-investigate):
> - Journal mode / WAL: LOCAL HAS a complete WAL implementation. src/storage/schema.rs:714-748 apply_runtime_pragmas sets journal_mode=WAL (guarded so steady-state opens do not reassert it), synchronous=NORMAL, temp_store=MEMORY, cache_size=-8000, journal_size_limit=33554432, and deliberately disables autocheckpoint (issue #219). src/storage/sqlite.rs:1805 try_wal_checkpoint (PASSIVE, every N mutations), :1825 checkpoint_full (TRUNCATE with PASSIVE fallback), :13563 Drop-time TRUNCATE gated on mutation count (issue #270), with signal handlers installed in src/main.rs:29 so SIGINT/SIGTERM unwind through Drop. A full 'rg journal_mode' over the whole repo returns only 9 hits, all in schema.rs. NOT a gap.
> - Retry / backoff on lock contention: LOCAL HAS this, and it is arguably better-shaped than DWS's. src/storage/sqlite.rs:1495-1530 with_write_transaction: busy_timeout deliberately 0 (issue #243, because frankensqlite's busy handler hot-spins and starves the competing writer), BEGIN IMMEDIATE returns SQLITE_BUSY immediately, then 8 attempts with jittered exponential backoff (50ms base, ~12.7s total). Same parameters duplicated at :745-803 for the metadata path. NOT a gap.
> - VACUUM / compaction: LOCAL is strong and arguably ahead. src/config/mod.rs:2090 compact_database_via_vacuum_into_in_place writes via VACUUM INTO to a .<stem>.vacuum.<pid>.tmp then installs it by atomic rename, with failure handling that returns the unchanged pre-compaction connection; :1764 in-place VACUUM, :1785-1798 the REINDEX + VACUUM INTO + atomic-rename sequence for issue #248; src/cli/mod.rs:3799 a user-facing `compact` command. NOT a gap. Note GO's internal/compact/ is AI issue-content summarization (compactor.go:32-46 compactableStore/SummarizeTier1), not database compaction — a category error, not a gap.
> - Backup / restore: LOCAL has JSONL history snapshots (src/sync/history.rs, prune/restore/list at src/cli/mod.rs:3559-3576) and a pre-migrate beads.db.pre-migrate snapshot (doctor_subsystems/mutate.rs:54). GO's BackupStore (internal/storage/storage.go:860) is Dolt remote-specific (BackupAdd/Sync/Remove/BackupDatabase/RestoreDatabase over dolthub://, file://, gs://). Not comparable; NOT a gap for a SQLite+JSONL design.
> - Schema migration framework: LOCAL is AHEAD. CURRENT_SCHEMA_VERSION = 21 (src/storage/schema.rs:12) vs DWS 19 (DWS src/storage/schema.rs:14). LOCAL has run_migrations_atomic (schema.rs:649), user_version pre/post verification with an internal error on mismatch (schema.rs:693-700), a forward-only refusal for a newer DB (schema.rs:931), a read-only-path refusal when the DB is behind (schema.rs:938), and a doctor migration chokepoint that verifies user_version == from and refuses on mismatch without stamping (doctor_subsystems/mutate.rs:1199-1223, regression test at :2015). NOT a gap.
> - Integrity checking: LOCAL has PRAGMA integrity_check at src/storage/sqlite.rs:1841 integrity_check_messages, registered as a doctor check (sqlite.integrity_check / sqlite3.integrity_check, doctor_subsystems/capabilities_doctor.rs:307-312). NOT a gap. (Its org-only scope is exactly why the -shm poison in finding 1 evades it — that is stated in finding 1, not double-counted here.)
> - Dolt versioned storage, remote push/pull, DoltGC / DoltGCFull / Flatten / RemoteRefPruner: all Dolt-specific (internal/storage/storage.go:690-726, internal/doltserver/doltserver.go 80KB, internal/doltremote/remote.go, internal/doltversion/*). LOCAL has no version-control storage engine and no remote, so these have no counterpart by construction. EXCLUDED as not-applicable, not as gaps.
> - DWS franken_sync/retry.rs: re-read in full. It is NOT a durability feature — it is a ReplaySafety proof that confines automatic replay to a single ordinary SELECT/INSERT/UPDATE/DELETE parsed via the engine grammar (fsqlite_parser), failing closed for transaction control, PRAGMA, VACUUM, ATTACH/DETACH, DDL, SAVEPOINT and trigger bodies. Its value is a safety invariant on an existing retry path, which LOCAL's coarser retry-around-the-whole-transaction already satisfies. Deliberately NOT reported as a separate finding.
> - DWS franken_sync/prepared.rs: re-read in full. It is a stale-prepared-program replacement on schema_stale errors (bounded to one recompile per invocation, with in_transaction() assertions so a snapshot conflict returns to its owner). It is a facade concern over fsqlite 0.3; LOCAL runs fsqlite 0.1.7 and its execute_raw path does not cache prepared programs, so there is no stale-program class of failure to fix. NOT reported.
> - DWS doctor_subsystems/selftest.rs (24KB) exists with no LOCAL counterpart (rg "selftest" -> 0 hits), and DWS health.rs/doctor_subsystems/{bundle,engine}.rs add sidecar-family helpers LOCAL lacks. These are doctor-surface breadth, not storage-durability guarantees, and are outside this domain's assignment. NOTED, not reported.
> 
> COULD NOT FULLY VERIFY:
> - GO's internal/migration/freeze.go is a candidate severity question I did not resolve: its package doc says bd "only ever reads" the marker, and doc tests confirm, but I did not audit every mutating bd command to prove none of them can bypass the gate. Finding 3 is scoped to the marker and the two documented gate shapes rather than to a claim of universal coverage.
> - DWS's engine-version delta (LOCAL fsqlite 0.1.7 per Cargo.toml:45-59 vs DWS's fsqlite 0.3) means some of DWS's crash-safety work lives in the engine crate rather than franken_sync; I read franken_sync but not the fsqlite 0.3 source, so I cannot say whether finding 1's poison is reachable on LOCAL's 0.1.7 engine or only on 0.3+. This does not weaken the finding — the #507 signature is a WAL-index property of the on-disk format, and LOCAL is the same engine family — but the reachability question is open and should be settled before scoping the port.
> - I did not verify whether LOCAL's fsqlite 0.1.7 has any built-in self-heal for the poison (an engine-level rebuild of the index on a BusyRecovery error). If it does, finding 1 downgrades from critical to high: the wedge would be self-healing rather than permanent, though the missing diagnosis and the missing "do not delete the WAL" warning would remain. This is the single most decision-relevant open question in this report.

### B — `find:sync-jsonl`

> SCOPE: sync-jsonl only (JSONL import/export, merge, conflict resolution, dedup, sync command surface). NOT audited: DWS's capacity.rs, list_fields.rs, search/, vcs.rs, upgrade.rs (self-update — LOCAL has the same self_update feature), and GO's tracker push/pull (ado/jira/linear/github/gitlab/notion), doltremote, httpapi, and doltserver subsystems — all out of the assigned domain or belonging to another agent.
> 
> VERIFIED AS PARITY (checked, NOT reported as gaps): (1) Raw export atomicity — LOCAL src/sync/mod.rs:2339-2340 uses durable_rename + parent fsync, matching DWS; only the receipt differs (reported separately). (2) Incremental export/auto-flush — LOCAL has try_incremental_auto_flush / finalize_incremental_auto_flush / collect_incremental_auto_flush_changes (src/sync/mod.rs:3053, 3399, 3406, 3521); DWS has the same shape. (3) Tombstone propagation — both LOCAL (src/sync/mod.rs:1915, 2050, 2218) and DWS (src/sync/mod.rs:11057, 11198) include unexpired tombstones in export. NOTE: LOCAL's docs/JSONL_COMPATIBILITY.md claims 'br never exports tombstones; deleted issues are simply omitted from the JSONL output' — that is STALE and contradicted by src/sync/mod.rs:2050 ('Include tombstones (for sync propagation)'). I treated the code as authoritative and did not report the doc drift as a gap. (4) Whole-record 3-way merge strategies — LOCAL src/sync/mod.rs:4916-4927 (PreferLocal default, PreferExternal, PreferNewer by updated_at, Manual) is byte-for-byte the same four-variant enum as DWS src/sync/mod.rs:16067-16082; neither upstream does FIELD-LEVEL merge, so that is not a gap against either. (5) Conflict-marker detection — both have scan_conflict_markers/ensure_no_conflict_markers (LOCAL src/sync/mod.rs:1408, 1447; DWS :10517, :10548). (6) Auto-import on startup — LOCAL src/main.rs:61 + src/sync/mod.rs:2571 auto_import_probe / :2775 auto_import_if_stale; GO cmd/bd/auto_import_upgrade.go:62 maybeAutoImportJSONL. Both are guarded; the guards differ (GO keys on DB emptiness + a .auto-import-issues.jsonl size/mtime stamp file, LOCAL keys on jsonl_content_hash metadata), but neither is strictly better. (7) --force-db/--force-jsonl, --error-policy, --orphans, --manifest, --allow-external-jsonl, --witness — present in LOCAL and DWS with the same names. (8) LOCAL HAS two things DWS does not and these are therefore NOT gaps: the read-only git probe block inside `sync --status` (src/cli/commands/sync.rs:94-142, DWS deliberately removed it in favour of a separate `vcs-status`, per DWS src/cli/mod.rs:3019-3022), and top-level `br import`/`br export` subcommands (DWS has neither).
> 
> COULD NOT FULLY VERIFY — put here rather than guessed:
> - DWS's `--migrate-source-repo-path` and `--resolve-source-id` source_repo normalization. LOCAL has a src/config/routing.rs with RouteEntry/resolve_route/follow_redirects and a federation_peers table with a source_repo-ish notion, and LOCAL docs/porting/EXISTING_BEADS_STRUCTURE_AND_ARCHITECTURE.md:2808 mentions a `jsonl_content_hash:<repo>` per-repo multi-repo mode, but I found NO corresponding per-repo metadata code in LOCAL src/sync/mod.rs (rg for 'multi.repo|multi_repo|per-repo' -> 0 hits there). I could not determine whether LOCAL's routing layer provides equivalent multi-repo JSONL behavior, so I did not file a finding. Flagging for whoever owns the routing domain.
> - GO's `bd import` restore path also accepts stdin (importInput == "-"), and LOCAL's docs/JSONL_COMPATIBILITY.md Testing section shows `br import --stdin`, but LOCAL's ImportArgs (src/cli/mod.rs:1629-1649) has no stdin flag. I did not report this: the local evidence is a doc example, not verified CLI surface, and `br import -i <file>` covers the same ground.
> - The exact blast radius of the content-hash divergence for an already-populated LOCAL database: I confirmed the two writers differ but did not verify how each tool behaves on encountering a foreign content_hash value during import (whether it recomputes, trusts, or treats it as a miss). That determines whether divergence causes a silent no-op or a mass re-import; I did not want to assert either without tracing both collision detectors end to end.
> - DWS's ExportPublicationAtomicity variant set (src/sync/mod.rs:3512) — I read the receipt type and the verification guards but not every variant, so the receipt finding is scoped to the missing artifact, not to a claimed behavior difference in atomicity itself.
> - DWS runs at Cargo version 0.6.0 (Cargo.toml:3) and LOCAL at 0.1.3. LOCAL's schema version is 21 (src/storage/schema.rs:12) versus DWS's 19 (src/storage/schema.rs:14) — LOCAL is ahead, which is why I read the missing DWS features as un-ported rather than reverted.

### B — `find:testing-parity-docs`

> SCALE BASELINE (all three, measured on disk this run): LOCAL tests/ = 155 .rs files / 105,934 LOC / 1,830 #[test]. DWS tests/ = 188 .rs files / 145,750 LOC / 2,300 #[test]. GO = 1,679 *_test.go files / 9,417 Test+Fuzz+Benchmark funcs, plus backend/conformance/ at 43,269 LOC, test/ (conformance, docsync, testmainconvention), internal/testutil/, and 27 release-gates/*.md. `comm -13 local_tests dws_tests` yields 33 DWS-only test files and ZERO LOCAL-only files — LOCAL's test-file *set* is a strict subset of DWS's, so every name-level finding below is an absence, not a rename. LOCAL's tests/conformance.rs is 13,599 lines vs DWS's 13,827: near-identical, so the conformance gap (#4) is purely a wiring gap, not a content gap.
> 
> WHAT I DELIBERATELY DID NOT CLAIM (verified negative, recorded so the verifier does not re-litigate):
> - Documentation inventory is at parity, not behind. `comm -13 local_docs dws_docs` yields only 10 DWS-only .md basenames out of 46 (AA_PRECISION_ANALYSIS.md, BRIDGE_PLAN_2026_09_01.md, ENGINE_OPERATING_MODEL.md, fsqlite_trailing_pages_report.md, GH384_ACCEPTANCE_MATRIX.md, LIST_FIELD_SELECTION.md, perf-negative-results.md, README.md, snapshot_review_2026_07_25.md, WAL_INDEX_RECOVERY_507.md), and those are perf investigations and dated snapshots rather than user-facing guides. LOCAL additionally holds two files DWS dropped (JSONL_COMPATIBILITY.md, BD_VS_BR.md). There is no missing-docs finding.
> - Linux/macOS/Windows packaging is at parity. `ls -R packaging/` gives LOCAL aur/ (PKGBUILD, PKGBUILD-git), homebrew/br.rb, scoop/br.json — byte-for-byte the same structure as DWS `packaging/{aur,homebrew,scoop}` (DWS's aur adds only a README.md and .SRCINFO). install.sh (LOCAL 59K vs DWS 66K) and install.ps1 (LOCAL 18K) both exist.
> - Fuzzing is at parity. LOCAL fuzz/ has Cargo.toml, corpus/ and fuzz_targets/ exactly as DWS does; LOCAL additionally has a checked-in `tests/proptest_hash.proptest-regressions` file. benches/ is at parity too (LOCAL storage_perf.rs 37K vs DWS 38K, same benchmarks.rs harness).
> - `.golangci.yml` / `.goreleaser.yml` / `npm-package/` / `winget/` / `.devcontainer/` / `test-pypi.yml` / `nix-build.yml` exist only in GO and were NOT reported as findings: goreleaser and npm/winget/pypi are distribution-channel choices, not capability parity, and LOCAL already reaches users via install.sh/install.ps1 plus three package-manager manifests. GO's `-race` flag in main.yml:482 is also not a transferable gap — it is a Go-runtime detector with no direct Rust analogue for a single-process CLI whose writes serialize on `.write.lock`.
> 
> COULD NOT FULLY VERIFY (no overclaim made):
> - The exact release-history content of LOCAL's 68K CHANGELOG.md was not read line by line; finding #12's claim is scoped to UPGRADE_LOG.md, whose full text I did read.
> - I did not execute any test, build, or conformance run — all findings are static (file presence, line counts, grep census, CI job graphs). The claim in #1 that `--lib` excludes `tests/` rests on Cargo target-selection semantics plus the direct observation that the command is literally `cargo test --lib --all-features`; I did not run it to observe the test count, per the read-only constraint.
> - DWS's `.github/workflows/doctor.yml` step names were read from `grep -nE '^  [a-z0-9_-]+:|name: '` plus targeted `sed` ranges; I did not read the full 6.2K file, so per-step command text (as opposed to step names) is unverified for that one workflow.
> - `tests/e2e_doctor_chokepoint.rs` at LOCAL is 1,550 lines / 57K; I inferred the 768KB `src/cli/commands/doctor.rs` figure from the task brief and did not independently measure it, so that number is not load-bearing in any finding.
> 
> INCIDENTAL OBSERVATION (not a capability gap, surfaced because it will break a build): LOCAL's `flake.nix` is a dangling symlink — `flake.nix ⇒ /Users/tranquangdang21/Projects/beads_rust/.claude/worktrees/wf_403ea9a0-9f1-9/flake.nix`, and `test -e flake.nix` fails, so the Nix entry point does not resolve. DWS ships a working 5.0K flake.nix. Worth a separate issue; it is packaging, not testing, and therefore outside this domain's finding list.

### B — `find:triage-analytics`

> NEGATIVE RESULT (the main deliverable of this audit): the assigned domain's premise did not hold. The brief assumed the upstreams ship graph-analytics intelligence (PageRank, betweenness, critical path, HITS, eigenvector, k-core, articulation points, slack, burndown, forecast, velocity, label health/flow) that LOCAL lacks. Both upstreams ship NONE of it. Verified by exhaustive vocabulary sweep, not inference:
> 
> - GO: `rg -ni -g '*.go' 'pagerank|betweenness|eigenvector|centrality|articulation|kcore|k-core|criticalpath|critical path' /tmp/beads_gap_audit/gastownhall_beads` -> exactly 1 hit across 2943 files, and it is a comment: internal/storage/dolt/dolt_benchmark_test.go:167 "This is the critical path for CLI commands that open/close the store each time."
> - DWS: `rg -oi --no-filename 'pagerank|page_rank|betweenness|centrality|eigenvector|articulation|burndown' /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src` -> only 2 pagerank, 1 betweenness, 1 velocity, 1 influence hits, all inside MCP resource doc-comments describing bv (the same approximation LOCAL already has).
> - GO has no MCP server at all: `rg -n -g '*.go' 'beads://' /tmp/beads_gap_audit/gastownhall_beads` -> 0 matches.
> 
> TWO FALSE POSITIVES I ruled out, worth recording so they are not re-investigated:
> - "forecast" 147 hits in DWS are `MigrationForecast`, a schema-migration dry-run plan struct in src/cli/commands/doctor_subsystems/schema_migration.rs:107 - not ETA forecasting.
> - "capacity" 2083 hits in DWS are WIP/admission policy (close_policy.rs:281 CapacityPolicy) plus ordinary `with_capacity`; "aging" 730 hits in GO are overwhelmingly the substrings "staging" and "managing" (e.g. internal/storage/dolt/federation.go). GO's cmd/bd/metrics.go is opt-in product telemetry, not issue analytics.
> 
> STATS PARITY IS EXACT, so there is no stats/velocity/aging/health metric gap to report. LOCAL src/cli/commands/stats.rs and DWS's are the same file modulo the capacity table: `diff` of the two `StatsArgs` structs in src/cli/mod.rs -> IDENTICAL (--by-type/--by-priority/--by-assignee/--by-label/--activity/--no-activity/--activity-hours/--format), and the whole-file diff's only substantive additions are `capacity_stats` + `print_capacity_table`. LOCAL additionally computes age_days and lead_time (14 and 13 hits in src/) - both present in DWS too (15 and 13), so parity, not a LOCAL-only extra worth reporting. scheduler.rs is likewise at parity (same 27-item function outline; DWS adds only scheduler_issue_is_unassigned and a moved stale_threshold_minutes). ReadySortPolicy (Hybrid/Priority/Oldest) exists in BOTH - LOCAL src/storage/sqlite.rs:5602-5607; LOCAL's is actually better typed (a SortPolicy value_enum at src/cli/mod.rs:3076 vs DWS's Option<String>). LOCAL's MCP analytics resources are line-for-line equivalent to DWS's: the beads:// URI sets are identical except LOCAL's `issues/{id}` vs DWS's `issue/{id}` (a naming difference, not a capability difference).
> 
> COULD NOT VERIFY / DELIBERATELY NOT REPORTED:
> 1. LOCAL src/web/api.rs:828 `stub_insights()` is a live route (wired at src/web/mod.rs:122) that always returns zeros - throughput[], createdClosed[], cycle p50/p90 all 0, aging[] empty, hasEvents:false. This is a genuine LOCAL analytics defect (the web dashboard's insights page is a stub), but it does NOT fit this audit's gap definition: neither upstream has the endpoint (`rg -n -g '*.go' -i 'insights' gastownhall_beads` -> 4 hits, all prose in memory.go; `rg -n -g '*.rs' -i 'insights' dicklesworthstone_beads_rust/src` -> 0 matches). It is a LOCAL-only build-out gap, not an upstream parity gap, so I did not fabricate a present_in value for it. Flagging it here for whoever owns the web/MCP domain.
> 2. LOCAL AGENTS.md:631-713 documents the richer analytics engine `bv` (PageRank, betweenness, HITS, eigenvector, critical path, k-core, articulation, slack, --robot-triage/--robot-insights/--robot-burndown/--robot-forecast) as a SEPARATE binary outside this crate, with an explicit scope boundary at AGENTS.md:635: "bv handles *what to work on* (triage, priority, planning)". Per the audit rules I checked whether this is an intentional documented gap: it is documented as a deliberate boundary, not an unported upstream feature - and neither upstream has a bv equivalent, so there is no gap to book. LOCAL does carry in-tree approximations of the two most useful bv signals behind the `mcp` feature: beads://graph/health (density, max_chain_depth, fan-out hotspots, stale count, cycle detection; src/mcp/resources.rs:921 labels it "Deep chain - critical path is long, hard to parallelize") and beads://issues/bottlenecks (blocks_count ranking, src/mcp/resources.rs:976 "a practical approximation of PageRank/betweenness from bv"). These are reachable only via `br serve` (mcp feature), not from the plain CLI - worth noting as a discoverability limit, but DWS has the identical MCP surface, so it is not a parity gap.
> 3. GO's `bd human stats` (cmd/bd/human.go:417-484) is the only stats command in the Go tree and prints just Total/Pending/Responded/Dismissed for human-labeled beads. LOCAL's `br stats` is far richer. Not a gap in LOCAL's favour-loss direction, so not reported.
> 4. DWS-only files I inspected and ruled OUT of this domain: model/acceptance.rs (acceptance_criteria checklist editing, GitHub #477 - an editing feature, not analytics), cli/commands/list_fields.rs (column selection for list output), storage/lint.rs, cli/commands/upgrade.rs, cli/commands/vcs.rs, franken_sync/*, doctor_subsystems/{bundle,engine,schema_migration,selftest}.rs, storage/search.rs, sync/db_inode_lock.rs. None compute a graph or prioritization metric.
> 
> Files most relevant to this domain for any follow-up: /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/stats.rs (2448 lines, parity with DWS), /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/graph.rs (2091 lines, no serializers), /Users/tranquangdang21/Projects/beads_rust/src/mcp/resources.rs (graph/health + bottlenecks approximations), /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs:3978-3991 (GraphArgs - the flag surface to extend), /Users/tranquangdang21/Projects/beads_rust/src/web/api.rs:828 (stub_insights), /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/commands/graph.rs:1196-1249 (DOT renderers to port), /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/graph_export.go and /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/graph_visual.go (DOT + HTML renderers to port).
> 
> METHOD NOTE: no files were created, edited or deleted; no cargo or compilation was run; no mutating git command was used. All findings rest on read-only rg/find/wc/diff/sed output captured above. One self-correction worth recording: my first pass used `rg -rn` where -r is the replace flag, which mangled the DWS forecast output into "Migrationn"; I detected it from the nonsense output and re-ran without -r before drawing any conclusion.


---

## Appendix C — All 99 verified findings, grouped by domain

The 21 ranked gaps in §4 are a **merge** of these 99 verified findings. The table below preserves every one of them at full resolution — the layer §4 deliberately compressed. Where a gap in §4 rests on more than one finding, the rows that fed it appear together.

> **How to read this against §4:** §4 is the decision layer (21 items, ranked, with consequences for an agent). This appendix is the evidence layer (99 items, unranked, with per-finding description and impact). Use §4 to decide what to do; use this to check whether a gap is one problem or several.

> **Why 22 group headings when §3 dispatched 12 domains?** The find-agents self-labelled each finding with a *finer* theme than their dispatch domain, and this appendix preserves their own label rather than normalising it. The mapping is roughly: `agent-write-surface`, `concurrency-correctness` and `data-integrity-guards` are sub-themes of `storage-durability`; `filtering`, `field-selection`, `pagination`, `query-dsl` and `search` are sub-themes of `query-search`; `cli-output-modes` and `cli-ux-agent-contract` are sub-themes of `cli-ux-agent-contract`; `audit-trail`, `graph-deps`, `mcp-agent-surface`, `model-lifecycle`, `observability-ops`, `robustness-portability`, `sync-jsonl`, `testing-parity-docs`, `triage-analytics` and `integrations-external` map one-to-one. The finer labels are kept because they are more informative than the dispatch name — and because altering an agent's own label would misrepresent what it reported.

### C — `agent-write-surface` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | dws-only | `missing_feature` | Acceptance-criteria checklist cannot be ticked or edited item-by-item on any surface |

<details><summary><b>Acceptance-criteria checklist cannot be ticked or edited item-by-item on any surface</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> LOCAL stores `acceptance_criteria` as one opaque string, so ticking off a checklist item means reading the whole field, string-editing it, and rewriting the whole field — exactly the destructive rewrite that the missing guard above fails to catch. DWS ships a 746-line shared acceptance parser plus in-place tick/untick/append selectors on both the CLI and the MCP `update_issue` tool, and projects the parsed checklist back out of `br show --json` and the MCP `show_issue` tool as a structured `acceptance_items` array. The read side and the write side are validated through one shared helper, so the two cannot drift. This finding was explicitly seen and deferred by the triage-analytics domain, which classified `src/model/acceptance.rs` as "an editing feature, not analytics" and left it for another owner; no domain picked it up.

**Impact**

> Acceptance criteria are the mechanism by which an agent decides whether a task is done. In LOCAL, recording progress on a checklist requires a read-modify-write of the entire field by hand, which is slow, token-expensive, and the exact operation that loses data under concurrency or truncation. On the MCP side — the surface LOCAL tells agents to prefer over shelling out — there is no way to record checklist progress at all short of rewriting `acceptance_criteria` wholesale. Combined with findings 2 and 3, LOCAL forces agents into the one write pattern most likely to silently destroy or clobber acceptance state.

**Local evidence**

> `grep -rn 'check_acceptance|uncheck_acceptance|add_acceptance|acceptance_items' --include=*.rs src/` -> 0 hits each; the same four terms -> 0 hits in tests/ as well. LOCAL's `UpdateArgs` (src/cli/mod.rs) offers only whole-field `pub acceptance_criteria: Option<String>` alongside `pub notes_push: Option<String>` — no item-level selector of any kind. DWS has src/model/acceptance.rs as a DWS-only basename with no LOCAL counterpart in the file inventory.

**Upstream evidence**

> DWS CLI flags: src/cli/mod.rs:1256 `pub check_acceptance: Vec<String>`, :1266 `pub uncheck_acceptance: Vec<String>`, :1276 `pub add_acceptance: Vec<String>`; conflict test at :3874 and parser assertions at :3909-3911 (selectors accept "1,4" 1-based indices and unique item text). Read surface: src/cli/commands/show.rs:920-922 and :995-997 build `acceptance_items` via `.with_acceptance_items()`. MCP read surface: src/mcp/tools.rs:1359-1360 `if !details.acceptance_items.is_empty() { obj.insert("acceptance_items", ...) }`. MCP write surface: src/mcp/tools.rs:2445 declares `check_acceptance` with "Tick acceptance checklist items in place without rewriting acceptance_criteria: 1-based item numbers (\"1,4\") or unique item text. show_issue lists items under acceptance_items." Selector plumbing at :2085; end-to-end MCP regression at :4464-4467 asserting `shown["acceptance_items"][1]["checked"] == true`. Implementation: DWS src/model/acceptance.rs, 746 lines. Shipped as GitHub #477 per CHANGELOG.md:712-717, which also records that these flags "tick, untick, and append acceptance items in place without --force" and that "both surfaces validate and rewrite through one shared helper (#477, bead iw7k.5)".

</details>

### C — `audit-trail` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | low | dws-only | `missing_feature` | Gate results have no append-only history — prior verdicts are overwritten |

<details><summary><b>Gate results have no append-only history — prior verdicts are overwritten</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** small

**Description**

> LOCAL's `gate_results` table holds only the current verdict per gate, so a gate that passed and later failed leaves no record that it ever passed, and a gate that was waived leaves no record of the waiver. DWS keeps a second append-only `gate_result_history` table with two composite indexes, so `br gate report` records both the current state and the full sequence of how it got there. LOCAL applies exactly this append-only-plus-current-state pattern elsewhere — `storage/events.rs` for issue mutations, `close_metadata` for close decisions — which makes the gate table the outlier. The observability-ops domain compared the `audit` command group and `storage/events.rs` against DWS and found parity, so this specific divergence in a sibling audit table went unreported.

**Impact**

> Gate verdicts become unreviewable after the fact: an auditor cannot answer "was this gate ever satisfied, and when did it stop being?" because the passing row is gone. That is the same evidentiary standard LOCAL already meets for issue mutations and close decisions, so the gap is an inconsistency within LOCAL's own design rather than a missing category. It also removes the evidence base for the audit pattern AGENTS.md prescribes for dependency cycles, where a triage decision has to be demonstrable after the fact. Moderate: narrow blast radius, but cheap to close given the table-and-index pattern is already established twice in the same schema file.

**Local evidence**

> `grep -rn gate_result_history --include=*.rs src/` -> 0 hits. `src/storage/schema.rs:284` defines `CREATE TABLE IF NOT EXISTS gate_results` and :299 `CREATE TABLE IF NOT EXISTS gate_waiters`; there is no history table anywhere in the schema (84 CREATE INDEX statements total, none on a gate history). LOCAL's only gate index is `idx_gate_results_issue` at src/storage/schema.rs:295.

**Upstream evidence**

> DWS src/storage/schema.rs:27 `CREATE TABLE IF NOT EXISTS gate_result_history (`, indexed at :41-42 by `idx_gate_result_history_issue ON gate_result_history(issue_id, id)` and at :43-44 by `idx_gate_result_history_scope ON gate_result_history(issue_id, from_status, to_status, status_revision, id)`. The behavioral change is recorded in DWS CHANGELOG.md as "`br gate report` writes `gate_results` as well as `gate_result_history`" — the current-state table is retained, the history table is added alongside it, matching the shape of LOCAL's own events/close_metadata pair.

</details>

### C — `cli-output-modes` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | low | both-upstreams | `missing_output_mode` | No `list --tree` parent/child hierarchy output mode |

<details><summary><b>No `list --tree` parent/child hierarchy output mode</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** small

**Description**

> LOCAL can render a dependency tree (`br dep tree`) but has no way to render the issue hierarchy as a tree in list output. DWS groups children under their parents with tree connectors in text output, which is the readable form of the epic/subtask structure that both `br epic` and the `parent-child` dependency type already store. The graph-deps domain explicitly left this thread open, noting it had not verified GO's `list_tree_deps.go` / `create_deps.go` surfaces against LOCAL's `br list --tree` — but LOCAL has no `--tree` to compare against.

**Impact**

> Reading the shape of an epic and its subtasks currently requires a filter plus manual assembly, or a dependency-tree render that answers a different question (what blocks what, not what contains what). For a human scanning a backlog, and for an agent summarizing one, the tree form is the cheapest readable projection of hierarchy and is a routine expectation of a tracker with epics. Moderate rather than high: the data is present and reachable via `br dep tree` and the `parent-child` dependency type, so nothing is lost — only the readable projection.

**Local evidence**

> `grep -c 'pub tree' src/cli/mod.rs` -> 0; no `tree` field exists on any args struct in the CLI. LOCAL's `ListArgs` fields (src/cli/mod.rs) run status, type_, assignee, unassigned, owner, pinned, mol_type, id, label, label_any, priority, priority_min, priority_max, title_contains, desc_contains, notes_contains, all, limit, offset, sort, reverse, deferred, overdue, long — no tree/grouping option. `grep -rn 'list --tree' src/` -> 0 hits. The only tree surface is the dependency graph, at src/cli/mod.rs:2455 (`br dep tree`) and :2457 (`br dep cycles`), which walks dependency edges rather than the parent-child hierarchy.

**Upstream evidence**

> DWS src/cli/mod.rs:1931-1936: `/// Group children under their parents with tree connectors (text output).` followed by `pub tree: bool`, on the list args struct; DWS CHANGELOG.md:808-809 records "`br list --tree` groups children under their parents with tree connectors in text output (#475) -- d461a399." GO ships the same capability through a different surface: cmd/bd/list_tree.go and cmd/bd/list_tree_deps.go.

</details>

### C — `cli-surface` (6 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | both-upstreams | `missing_flag` | `update` has no optimistic-concurrency guards (GO --if-assignee/--if-status, DWS --if-unchanged) and no --set-metadata/--unset-metadata |
| — | low | go-only | `missing_flag` | `show` exposes only 5 flags vs GO's 12 — no --include-comments, --include-dependents, --brief-deps, --as-of, --children, --refs, --current, --watch, --long, --local-time |
| — | info | go-only | `missing_feature` | No third-party tracker/integration commands (github, gitlab, jira, linear, notion, ado, mail) — 7 GO commands |
| — | low | go-only | `missing_flag` | `close` missing --reason-file/--claim-next/--continue and GLOBAL flags missing --directory(-C)/--readonly/--sandbox/--global/--ignore-schema-skew |
| — | low | dws-only | `missing_feature` | DWS `capacity` (audited WIP-limit exemptions with approval provider, expiry, and append-only audit history) has no LOCAL equivalent |
| — | low | dws-only | `missing_flag` | `sync` lacks DWS's 8 reconciliation-plan safety flags (--dry-run, --reconcile, --apply, --expect-plan-sha256, --resolve-source-ids, --skip-invalid-records, --reconcile-additive, --migrate-source-repo-path) |

<details><summary><b>`update` has no optimistic-concurrency guards (GO --if-assignee/--if-status, DWS --if-unchanged) and no --set-metadata/--unset-metadata</b></summary>

- **Severity:** high · **Present in:** both-upstreams · **Effort:** small

**Description**

> LOCAL `UpdateArgs` has 30 fields and none of them is a compare-and-set guard. GO ships two: `--if-assignee` and `--if-status`, which apply the update only if the current stored value matches, writing *nothing* and exiting **13** (distinct from exit 1 for ordinary failures) on mismatch. DWS adds a third, `--if-unchanged`, for content-hash-based optimistic concurrency. LOCAL has no equivalent: the only concurrency-adjacent flag is `--force`, which is a bypass rather than a guard. An agent that reads an issue, decides, and writes back has no way to detect that another agent mutated the row in between — the lost update is silent. The same command also lacks GO's repeatable `--set-metadata key=value` / `--unset-metadata key` (LOCAL exposes `--metadata` as a *read filter* on `list` but has no way to write metadata on `update` at all), and lacks DWS's `--append-notes`, `--check-acceptance`, `--uncheck-acceptance`, `--add-acceptance`, and `--transition-comment`.

**Impact**

> This is the primary lost-update guard for the multi-agent swarm workflow br is built for. AGENTS.md's degraded-coordination protocol tells agents to inspect before editing; without a compare-and-set primitive the write itself is still unguarded, so two agents on the same issue both "succeed" and one silently clobbers the other. The distinct exit code 13 also gives agents a branchable signal. AGENTS.md's stale-claim rule ("not any old claim is free work ... reclaim after evidence") is unenforceable without it.

**Local evidence**

> `python3` field extraction of `pub struct UpdateArgs` in /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs returns: ids title description design acceptance-criteria notes notes-push status priority type- assignee owner claim force due defer estimate add-label remove-label set-labels parent external-ref source-repo source-repo-path agent-context session agent-name harness model. Greps over /Users/tranquangdang21/Projects/beads_rust/src/cli/ return 0 hits for: `if-assignee`=0, `if_assignee`=0, `if-status`=0, `if_status`=0, `check-acceptance`=0, `set-metadata`=0, `unset-metadata`=0. `force`=present but is a bypass flag, not a precondition check.

**Upstream evidence**

> GO /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/update.go:997 `updateCmd.Flags().String("if-assignee", "", "Apply the update only if the current assignee equals this value (--if-assignee '' requires unassigned); a mismatch writes nothing and exits 13 (vs 1 for other failures). Requires a field update; cannot combine with --claim")`; :998 `if-status` (same semantics for status); :1027 `--set-metadata` (repeatable); :1028 `--unset-metadata` (repeatable). DWS /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/mod.rs:1341 `pub if_unchanged: Option<String>` in `UpdateArgs`.

</details>

<details><summary><b>`show` exposes only 5 flags vs GO's 12 — no --include-comments, --include-dependents, --brief-deps, --as-of, --children, --refs, --current, --watch, --long, --local-time</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** small

**Description**

> LOCAL `ShowArgs` is `{ ids, format, wrap, stats, oneline }` — five flags, all cosmetic or global-mode overrides. GO's `bd show` adds 12, three of which are load-shaping flags that exist specifically to keep the JSON payload bounded for agents: `--include-comments` (stream full comment bodies), `--include-dependents` (stream full dependent issues), and `--brief-deps` (reduce each dependency to identity fields, dropping description/design/notes/acceptance-criteria). LOCAL has no way to request a narrow payload: the dependents graph and comment bodies are always either included or not, chosen by the binary rather than the caller. `--as-of` (historical view at a commit/branch) and `--children` (children-only listing) are also absent, as are `--refs`, `--current`, `--watch`, `--long`, `--local-time`.

**Impact**

> On a hub issue (a parent with hundreds of children) or a heavily-commented issue, an agent calling `br show <id> --json` pays a payload cost proportional to the whole neighborhood on every poll. AGENTS.md tells agents to always use `--json`; without `--brief-deps`/`--include-comments` there is no way to keep a context window bounded for exactly the issues most worth polling. `--children` also forces `br list`-then-filter loops for parent traversal.

**Local evidence**

> `python3` field extraction of `pub struct ShowArgs` in /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs returns exactly: ids format wrap stats oneline. Greps over /Users/tranquangdang21/Projects/beads_rust/src/cli/ return 0 hits for every missing flag: `include-comments`=0, `include_comments`=0, `include-dependents`=0, `include_dependents`=0, `as-of`=0, `as_of`=0, `brief-deps`=0. Note this is a *capability* gap not an absence of data: /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/show.rs:498,558,638,674,709,730,763 already builds `dependents` index maps and renders them, so the payload exists and the caller simply cannot turn parts of it off.

**Upstream evidence**

> /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/show.go:309 `showCmd.Flags().Bool("include-dependents", false, "Stream full dependent issues in JSON output (--json only; may be slow on hub beads)")`; :310 `include-comments`; :311 `brief-deps` ("Reduce each dependency to its identity fields in JSON output (--json only; drops description, design, notes and acceptance criteria)"); :302 `refs`; :303 `children`; :304 `as-of`; :306 `local-time`; :307 `watch`; :308 `current`; :299 `thread`; :300 `short`; :301 `long`.

</details>

<details><summary><b>No third-party tracker/integration commands (github, gitlab, jira, linear, notion, ado, mail) — 7 GO commands</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** large

**Description**

> GO registers seven top-level commands that bridge beads to external trackers: `github`, `gitlab`, `jira`, `linear`, `notion`, `ado` (Azure DevOps), and `mail`. `ado` alone is 30KB (cmd/bd/ado.go) with a dedicated roundtrip test file, indicating a real bidirectional sync implementation, not a stub. LOCAL has no integration surface at all. The only outbound network call in LOCAL's CLI is the self-update path — src/cli/commands/mod.rs:124-129 hardcodes `GITHUB_REPO_OWNER = "quangdang46"`, `GITHUB_REPO_NAME = "beads_rust"`, and builds a releases-latest API URL for version checking. A case-insensitive grep of /Users/tranquangdang21/Projects/beads_rust/src/cli/ for the integration names returns 0 hits for `gitlab` and `ado`, and 1 incidental hit each for `jira`, `linear`, `notion` (substring matches inside unrelated identifiers, not commands).

**Impact**

> Teams that use br alongside an external tracker get no bridge in either direction — no push of issue state, no pull of remote status. This is the largest *feature-area* delta in the CLI surface by breadth (7 commands, 6 distinct vendors). It is plausibly a conscious scope decision for a local-first, non-invasive tracker, but it is undocumented as such: no LOCAL doc states that integrations are out of scope, so the gap is invisible to anyone auditing the project.

**Local evidence**

> `rg -ci -- 'gitlab' /Users/tranquangdang21/Projects/beads_rust/src/cli/` -> 0. `rg -ci -- 'ado\b' /Users/tranquangdang21/Projects/beads_rust/src/cli/` -> 0. `rg -n 'jira|linear|notion' /Users/tranquangdang21/Projects/beads_rust/src/cli/` -> 1 incidental substring hit each, none a command registration. `python3` brace-count of `pub enum Commands` in src/cli/mod.rs (65 variants) contains none of the seven. The only GitHub reference in the whole CLI is the self-update constant pair at src/cli/commands/mod.rs:124-129 (`GITHUB_REPO_OWNER`, `GITHUB_REPO_NAME`, `github_latest_release_api_url()`), which targets this repo's own releases, not the user's tracker.

**Upstream evidence**

> GO registration sites: cmd/bd/github.go (`githubCmd`), cmd/bd/gitlab.go (`gitlabCmd`), cmd/bd/jira.go (`jiraCmd`), cmd/bd/linear.go (`linearCmd`), cmd/bd/notion.go (`notionCmd`), cmd/bd/ado.go:30KB with cmd/bd/ado_roundtrip_test.go and cmd/bd/ado_test.go, cmd/bd/mail.go:111 `rootCmd.AddCommand(mailCmd)`.

</details>

<details><summary><b>`close` missing --reason-file/--claim-next/--continue and GLOBAL flags missing --directory(-C)/--readonly/--sandbox/--global/--ignore-schema-skew</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> Two related flag-surface gaps. (a) `close`: LOCAL `CloseArgs` has 9 fields (ids reason force suggest-next session robot agent-name harness model bypass-policy bypass-reason); GO registers 12 including `--reason-file` (read the close reason from a file or `-` for stdin), the `--resolution`/`--message`/`--comment` hidden aliases, `--claim-next` (atomically close *and* claim the next highest-priority ready issue), and `--continue`/`--no-auto` (advance a molecule, optionally without claiming). LOCAL's `suggest-next` covers only the read half of `claim-next`. (b) GLOBAL: LOCAL's root `Cli` struct has 14 fields (command db actor json no_daemon no_auto_flush no_auto_import allow_stale lock_timeout no_db verbose quiet no_color). GO's persistent flags add `-C/--directory` ("Change to this directory before running the command (like git -C)"), `--database` (per-invocation database override without mutating project config), `--sandbox` (disables Dolt auto-push), `--readonly` ("Read-only mode: block write operations (for worker sandboxes)"), `--global` (use the global shared-server database), `--ignore-schema-skew` (proceed despite forward schema drift), plus `--cpu-profile`/`--mem-profile`. `--readonly` in particular is the mechanism for confining a worker agent; LOCAL's nearest option is `--no-db`, which is strictly more restrictive (no database at all) and therefore not a substitute.

**Impact**

> Three practical effects. (1) Long agent-generated close reasons must be inlined into argv, hitting shell length limits and quoting hazards; `--reason-file -` is the clean fix. (2) `--readonly` is the standard way to hand a worker agent a guaranteed-non-mutating view of the tracker; without it the only such mode is `--no-db`, which also removes all read access, so a worker cannot read. (3) `-C/--directory` is the conventional way to target a workspace without `cd`, which matters when br is invoked from an agent tool that cannot change cwd. `-C` in particular is a near-universal CLI convention (git, cargo, npm all have it).

**Local evidence**

> `python3` field extraction: `pub struct CloseArgs` in /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs -> ids reason force suggest-next session robot agent-name harness model bypass-policy bypass-reason. `pub struct Cli` (root, :697) -> command db actor json no_daemon no_auto_flush no_auto_import allow_stale lock_timeout no_db verbose quiet no_color. Greps over /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs all return 0 hits for `long = "directory"`, `short = 'C'`, `long = "readonly"`, `long = "sandbox"`, `long = "global"`, `long = "ignore-schema-skew"`. `rg -n 'reason-file|reason_file' src/cli/` -> 0 hits, so close reasons must be passed inline via `--reason` or `--bypass-reason`.

**Upstream evidence**

> GO /tmp/beads_gap_audit/gastownhall_beads/cmd/bd/close.go:388 `closeCmd.Flags().String("reason-file", "", "Read close reason from file (use - for stdin)")`; :393 `--claim-next` ("Automatically claim the next highest priority available issue"); :390-391 `--continue`/`--no-auto`; :382-387 the `--resolution`/`--message`/`--comment` aliases. Global flags: cmd/bd/main.go:883 `--directory`/`-C`; :885 `--database`; :890 `--sandbox`; :891 `--readonly` ("Read-only mode: block write operations (for worker sandboxes)"); :892 `--global`; :894-895 `--cpu-profile`/`--mem-profile`; :898 `--ignore-schema-skew` ("Proceed despite forward schema drift (some queries may fail)").

</details>

<details><summary><b>DWS `capacity` (audited WIP-limit exemptions with approval provider, expiry, and append-only audit history) has no LOCAL equivalent</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS registers a top-level `br capacity` command with four subcommands — `exempt`, `renew`, `revoke`, `exemptions` — implementing GitHub #384 phase 4: audited, issue-specific exemptions from named workflow capacities. An exemption grants a narrowly scoped waiver: one issue, one named capacity (`--status` or `--group`), gated behind an authorized `--provider` that must appear in `workflow.capacity.exemptions.providers`, a mandatory `--reason`, and an `--expires` timestamp. `renew` extends, `revoke` withdraws, `exemptions --history` replays an append-only audit trail. All four take `--robot` for machine-readable output. Enforcement lives in the storage counting engine, so an active authorized exemption excludes its issue from the named capacity's counted total while keeping it visible in queue metrics. LOCAL has no capacity accounting, no exemption record type, and no command.

**Impact**

> Capacity/WIP governance is a DWS-specific governance feature, so this is a br-behind-its-Rust-upstream gap rather than a parity miss. It matters for anyone adopting DWS's workflow-capacity model: there is no way to record a reviewed exception when a capacity limit must be bypassed, and therefore no audit trail for that decision. Notably the exemption records are explicitly *not* synced through JSONL (they are project-local auxiliary metadata like gate results), so porting them would not perturb the sync format.

**Local evidence**

> `python3` brace-count of `pub enum Commands` in /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs returns 65 variants, none named Capacity. `rg -in 'capacity' /Users/tranquangdang21/Projects/beads_rust/src` returns 58 hits, every one of which is the Rust standard-library method `Vec::with_capacity` / `HashSet::with_capacity` / `BufReader::with_capacity` (src/close_policy.rs:606, src/sync/mod.rs:807,1411,1543,1919,1965,1986,2191, etc.) — zero semantic hits. `rg -in 'workflow.capacity|exemptions.providers'` -> 0 hits. `rg -in 'capacity' /Users/tranquangdang21/Projects/beads_rust/docs` and CHANGELOG.md -> 0 hits, so this is not documented as an intentional omission.

**Upstream evidence**

> DWS /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/mod.rs:765-768 `Capacity { #[command(subcommand)] command: CapacityCommands, }` inside `pub enum Commands` (declared at :748); arg structs `CapacityExemptArgs` at :2123, `CapacityRenewArgs` at :2156, `CapacityRevokeArgs` at :2188, `CapacityExemptionsArgs` at :2216. Implementation /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/commands/capacity.rs (17KB), whose module doc states: "Workflow capacity management commands (`br capacity`), GitHub #384 phase 4: audited issue-specific capacity exemptions ... `br capacity exempt <id> --status <name> --provider <p> --reason <r>`".

</details>

<details><summary><b>`sync` lacks DWS's 8 reconciliation-plan safety flags (--dry-run, --reconcile, --apply, --expect-plan-sha256, --resolve-source-ids, --skip-invalid-records, --reconcile-additive, --migrate-source-repo-path)</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** medium

**Description**

> LOCAL `SyncArgs` has 19 fields. DWS has 27. The eight LOCAL lacks form a two-phase plan/apply safety mechanism: `--dry-run` (compute the reconcile plan without writing), `--reconcile` (produce a reconciliation plan for divergent histories), `--apply` (commit a previously produced plan), `--expect-plan-sha256` (apply only if the plan is still the one the caller reviewed — optimistic concurrency on the *plan*, not the row), `--resolve-source-ids` (map source-side IDs during migration), `--reconcile-additive` (additive rather than destructive reconcile), `--migrate-source-repo-path`, and `--skip-invalid-records`. LOCAL's sync flags are `flush-only import-only merge status witness witness-chunk-lines witness-parallelism export-parallelism force force-db force-jsonl allow-external-jsonl manifest error-policy orphans rename-prefix rebuild robot` — powerful operational knobs, but no dry-run-to-review-then-apply cycle. AGENTS.md's mandated workflow is `br sync --flush-only`, which is a narrower path than any of these.

**Impact**

> `--expect-plan-sha256` is the highest-value of the eight: it closes the window between "agent reviewed the reconcile plan" and "agent applied it", so a concurrent sync cannot slip a divergent change past the reviewer. Without it, any reconcile-style operation in LOCAL is review-free or all-or-nothing at the operator's discretion. AGENTS.md's sync-safety checklist is otherwise well covered by LOCAL.

**Local evidence**

> `python3` field extraction of `pub struct SyncArgs` in /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs returns: flush-only import-only merge status witness witness-chunk-lines witness-parallelism export-parallelism force force-db force-jsonl allow-external-jsonl manifest error-policy orphans rename-prefix rebuild robot. Greps over /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs return 0 hits for: `skip-invalid-records`=0, `skip_invalid_records`=0, `reconcile`=0, `reconcile-additive`=0, `reconcile_additive`=0, `migrate-source-repo-path`=0, `migrate_source_repo_path`=0, `expect-plan-sha256`=0, `expect_plan_sha256`=0, `resolve-source-ids`=0, `resolve_source_ids`=0. (`dry_run`=8 hits, all in other arg structs, not SyncArgs.) LOCAL does have `--error-policy` and `--force`, which are the closest analogues but neither is a reviewable plan artifact.

**Upstream evidence**

> DWS /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src/cli/mod.rs `pub struct SyncArgs`: `skip_invalid_records` at :2985, `reconcile` at :3002, `dry_run` at :3013, `apply` at :3057, `expect_plan_sha256` at :3065.

</details>

### C — `cli-ux-agent-contract` (8 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | info | both-upstreams | `missing_feature` | No schema_version canary stamped into any br --json wire blob |
| — | low | go-only | `missing_flag` | No global -C/--directory flag to run a command against another directory |
| — | low | go-only | `missing_flag` | No global --readonly / --sandbox flag to enforce a read-only posture |
| — | low | go-only | `missing_feature` | No pager integration (--no-pager, BD_PAGER, BD_NO_PAGER, height-aware skip) |
| — | low | go-only | `missing_feature` | No `br config validate` subcommand |
| — | low | go-only | `partial` | Terminal/agent env surface is one variable wide: no CLICOLOR, CLICOLOR_FORCE, TERM=dumb, BD_AGENT_MODE, BD_NON_INTERACTIVE, BD_NO_EMOJI, BD_GIT_HOOK |
| — | medium | dws-only | `missing_feature` | No `br config schema` — no machine-readable inventory of honored config keys, aliases, types, or defaults |
| — | info | dws-only | `divergent_behavior` | Structured JSON error envelope goes to stdout, and the published schema doc contradicts the implementation |

<details><summary><b>No schema_version canary stamped into any br --json wire blob</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** small

**Description**

> GO stamps an integer `schema_version` into every object-shaped JSON blob and into the error envelope via `wrapWithSchemaVersion`/`outputJSONError`. LOCAL emits zero `schema_version` in the machine-output path, so a downstream decoder consuming `br list --json`, `br show --json`, `br create --json` has no way to detect that the wire shape changed. `br robot-docs` and `br capabilities` do publish a `contract_version`, but that is a static string constant, not a per-payload wire canary. GO's own protocol catalog names this field the mechanism a consumer "keys its pinned-decoder migration off".

**Impact**

> A robot/consumer pinned to a br JSON decoder cannot detect an upstream wire change and must either break silently or re-derive compatibility by trial. GO makes the same change an explicit, reviewable integer bump. This is the single largest machine-contract gap, and also the cheapest to close (one wrapper in src/output/context.rs).

**Local evidence**

> `rg -n 'schema_version' src/output/ src/format/` -> 0 hits. `rg -c 'schema_version' src/` -> 14 files, all domain-specific sub-envelopes that never cover the core wire: src/coordination.rs, src/policy.rs, src/write_combining.rs, src/storage/schema.rs, src/mcp/resources.rs, src/cli/commands/{audit,info,sync}.rs, src/sync/witness.rs, src/cli/commands/doctor_subsystems/*. None is in src/output/context.rs (the JSON writer) or src/format/output.rs (the row types). `rg -n 'write_json_array_page_to_writer' -A 20 src/output/context.rs:132-163` shows the emitted object is `{<array>,"total":N,"limit":N,"offset":N,"has_more":bool}` with no version field. DWS is identical: `rg -n 'schema_version' src/output/ src/format/` -> 0 hits.

**Upstream evidence**

> cmd/bd/output.go:12 `const JSONSchemaVersion = 1`; cmd/bd/output.go:69-97 `wrapWithSchemaVersion` adds `m["schema_version"] = JSONSchemaVersion` to every object payload; cmd/bd/output.go:114-134 `outputJSONError` stamps it on the error envelope; cmd/bd/output.go:38-42 envelope mode emits `{schema_version, data, pagination}`; cmd/bd/protocol/CATALOG.md ("Regenerate after any deliberate wire change ... and bump `JSONSchemaVersion` ... the `schema_version` field in every blob is the coordination canary a downstream consumer keys its pinned-decoder migration off"); docs/reference/json-schema.md:3,70-77 (version policy: bumps on breaking change, additive optional fields do not bump).

</details>

<details><summary><b>No global -C/--directory flag to run a command against another directory</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** small

**Description**

> GO registers a persistent `-C`/`--directory` flag with `git -C` semantics, so an agent can target a workspace without a shell `cd`. LOCAL has no global directory flag; the only directory-scoped arg in the whole tree is `recipes`' command-scoped `project` option. An agent that wants `br -C /work/repo ready` must wrap the call in `sh -c 'cd ... && br ready'`, which loses structured error propagation and adds a subshell.

**Impact**

> Agents fanning out across multiple checkouts must shell out, which defeats the point of a structured CLI. It also removes GO's ability to have a single chokepoint resolve workspace context for a non-cwd target.

**Local evidence**

> `rg -n 'short = .C.|long = "directory"|pub chdir|pub directory' src/` -> 0 hits. The 12 global flags on `struct Cli` (src/cli/mod.rs:697-748: db, actor, json, no_daemon, no_auto_flush, no_auto_import, allow_stale, lock_timeout, no_db, verbose, quiet, no_color) contain no directory/chdir entry. The only directory arg is src/cli/mod.rs:1831 `/// Project directory (defaults to current working directory)` under `recipes`, which is command-scoped, not global. DWS: `rg -n 'pub chdir|long = "directory"' src/` -> 0 hits.

**Upstream evidence**

> cmd/bd/main.go:883 `rootCmd.PersistentFlags().StringVarP(&changeDir, "directory", "C", "", "Change to this directory before running the command (like git -C)")`; cmd/bd/main.go:52 `changeDir string`; cmd/bd/main.go:944-971 `changeDirEnvSnapshot` resolves the beads dir for the target tree and restores the env afterward; cmd/bd/errors.go freeze-search comment explicitly reasons about "`bd -C /work/repo create` (which sets BEADS_DIR and never chdirs)".

</details>

<details><summary><b>No global --readonly / --sandbox flag to enforce a read-only posture</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** medium

**Description**

> GO has a global `--readonly` flag whose stated purpose is "block write operations (for worker sandboxes)", enforced through a single chokepoint (`CheckReadonly`) that every write command already calls, plus a global `--sandbox` flag. LOCAL has no way to declare a read-only intent: an operator handing `br` to a sandboxed worker cannot prevent a mutating subcommand from running. LOCAL's write-combining queue has per-command read-only *classification* (which commands are safe) but no user-facing switch that turns the whole tool read-only.

**Impact**

> A sandbox integrator running untrusted agent-generated `br` invocations has no declarative way to forbid writes. GO also documents that the chokepoint approach covers commands added later, a property a hand-maintained per-command flag list cannot give.

**Local evidence**

> `rg -n 'long = "readonly"|long = "sandbox"|pub readonly|pub sandbox' src/` -> 0 hits. The 12 global flags on src/cli/mod.rs:697-748 include no readonly/sandbox entry. `rg -n 'read_only' src/` returns only internal classification helpers (src/main.rs:1095 `supports_read_only_fast_open`, src/main.rs:1124-1126 `is_read_only_dep_command` etc., src/write_combining.rs:1808 `read_only_dependency_commands_stay_direct`) that decide lock behavior, not a caller-selectable read-only mode. `rg -n '"readonly"|"sandbox"' src/config/mod.rs` -> 0 hits, so there is no config key either. DWS: `rg -n 'long = "readonly"|long = "sandbox"|pub readonly|pub sandbox' src/` -> 0 hits.

**Upstream evidence**

> cmd/bd/main.go:891 `rootCmd.PersistentFlags().BoolVar(&readonlyMode, "readonly", false, "Read-only mode: block write operations (for worker sandboxes)")`; cmd/bd/main.go:890 `--sandbox` ("Sandbox mode: disables Dolt auto-push"); cmd/bd/main.go:765,1113 `readonlyMode = config.GetBool("readonly")` (also a config key); cmd/bd/errors.go:196-215 `CheckReadonly(operation string)` — "the chokepoint essentially every write command already calls first", exiting 1 on readonly and `ExitMigrationFrozen` otherwise; cmd/bd/main.go:1525 `effectiveRootStorePolicy(cmd.Name(), readonlyMode)`.

</details>

<details><summary><b>No pager integration (--no-pager, BD_PAGER, BD_NO_PAGER, height-aware skip)</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> GO pipes long human output through a pager, gated on a per-command `--no-pager` flag, the `BD_PAGER` then `PAGER` then `less` env chain, a `BD_NO_PAGER` kill switch, a TTY check, and a terminal-height comparison so short output is printed directly rather than paged. LOCAL has no pager: on a TTY, `br list` on a large result set scrolls off with no way to scroll back. This is human-facing rather than machine-facing, but it is a visible output-mode capability and it is explicitly opt-out-able for scripts.

**Impact**

> Long `br list` / `br show` / `br audit` output is unrecoverable once it exceeds scrollback. Low risk for robots (they use --json), real cost for the human-in-the-loop case the AGENTS.md agent flow assumes.

**Local evidence**

> `rg -n 'BD_NO_PAGER|BD_PAGER|pub no_pager|ToPager|to_pager' src/` -> 0 hits. `rg -ni 'pager' src/` returns only fsqlite internals (src/logging.rs:81,95,133 `fsqlite_pager=warn`) and unrelated PageRank prose in src/mcp/{prompts,resources}.rs — no output pager. DWS: `rg -n 'BD_NO_PAGER|BD_PAGER|pub no_pager' src/` -> 0 hits, and `rg -ni 'pager' src/output/ src/format/` -> 0 hits.

**Upstream evidence**

> internal/ui/pager.go:15-36 `PagerOptions{NoPager}` + `shouldUsePager` (checks `--no-pager`, `BD_NO_PAGER`, TTY); :38-49 `getPagerCommand` (`BD_PAGER` -> `PAGER` -> `less`); :51-62 `getTerminalHeight`; :64-92 `ToPager` skips the pager when content fits the terminal and sets `LESS=-RFX` by default; registered on commands e.g. cmd/bd/list.go:392 and cmd/bd/list_proxied_server.go:304 `ui.ToPager(buf.String(), ui.PagerOptions{NoPager: in.noPager})`, flag read at cmd/bd/list_input.go:253; internal/ui/pager_test.go:65-129 covers env precedence.

</details>

<details><summary><b>No `br config validate` subcommand</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> GO's `bd config validate` is a check-only command that validates sync-related configuration and prints a diagnosis without mutating anything. LOCAL's `br config` has List/Get/Set/Delete(edit: unset)/Edit/Path and no validate arm, so an agent cannot ask the tool to check its own config before a sync. LOCAL does have `br formula validate` (src/cli/commands/formula.rs:279), which is a different subsystem and does not cover config.yaml.

**Impact**

> A pre-flight `br config validate --json` gate that a CI job or an onboarding agent can run before `br sync --flush-only` does not exist; config mistakes surface as a mid-sync failure with a less specific message.

**Local evidence**

> `rg -n 'ConfigCommands::Validate|fn execute_validate|config_validate' src/` -> 2 hits, both unrelated: src/config/mod.rs:7597 (a test named `load_config_validates_external_jsonl_before_prefix_inference`) and src/cli/commands/formula.rs:279 (`fn execute_validate(args: &FormulaValidateArgs, ...)` — the `br formula validate` subcommand). The exhaustive arm list is src/cli/commands/config.rs:218-235: `Path | Edit | List | Set | Delete | Get`. The enum itself (src/cli/mod.rs:3461-3511) has exactly List, Get, Set, Delete(alias unset), Edit, Path.

**Upstream evidence**

> cmd/bd/config.go:640-652 `var configValidateCmd = &cobra.Command{ Use: "validate", Short: "Validate sync-related configuration", ...}` with an explicit check list (federation.sovereignty in T1-T4, federation.remote set for Dolt sync, remote URL format for dolthub:// gs:// s3:// az:// file://, routing.mode in auto|maintainer|contributor|explicit) and an example invocation; registered at cmd/bd/config.go:1132 `configCmd.AddCommand(configValidateCmd)`.

</details>

<details><summary><b>Terminal/agent env surface is one variable wide: no CLICOLOR, CLICOLOR_FORCE, TERM=dumb, BD_AGENT_MODE, BD_NON_INTERACTIVE, BD_NO_EMOJI, BD_GIT_HOOK</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> GO's color/emoji/hyperlink policy honors the full cross-tool convention set (NO_COLOR, CLICOLOR=0, CLICOLOR_FORCE, TERM=dumb, FORCE_HYPERLINK, BD_GIT_HOOK=1, BD_NO_EMOJI) plus an explicit BD_AGENT_MODE=1 that switches rendering to agent-optimized mode, and a BD_NON_INTERACTIVE guard used across 19 files. LOCAL's mode detection consults only `--no-color`, `NO_COLOR`, TTY-ness, and its own `BR_OUTPUT_FORMAT`/`TOON_DEFAULT_FORMAT`. A harness that sets CLICOLOR=0 or TERM=dumb to control every child process gets no effect on `br`, and `br` emits its Unicode status icons unconditionally in text mode.

**Impact**

> `br` does not honor the conventions a CI harness or parent agent process sets for every other child tool, and has no documented way to be put into an explicitly agent-optimized rendering mode (LOCAL achieves the effect only by choosing --json). BD_GIT_HOOK-style escape-leak prevention is also absent.

**Local evidence**

> `rg -n 'CLICOLOR|BD_AGENT_MODE|BD_NON_INTERACTIVE|BD_NO_EMOJI|BD_GIT_HOOK|FORCE_HYPERLINK' src/` -> 0 hits. `rg -c 'CLICOLOR' src/` -> 0 files. The full LOCAL env surface (`rg -oN 'env = "...|env::var("...|var("...' src/`) is 27 names: BR_ACTOR, BR_AGENT_NAME, BR_CODEX_HOOK_CACHE, BR_DATABASE_PATH, BR_DOCTOR_STALE_LOCK_THRESHOLD_SECS, BR_HARNESS, BR_HISTORY_MIN_INTERVAL_SECS, BR_IGNORE_SCHEMA_SKEW, BR_INHERITED_CONTEXT, BR_MODEL, BR_OUTPUT_FORMAT, BEADS_*, NO_COLOR, COLUMNS, EDITOR, RUST_LOG, TOON_DEFAULT_FORMAT, TOON_STATS, XDG_CACHE_HOME, and the PATH/HOME/USER/VISUAL set. Mode detection is exactly src/output/context.rs:994-997 (`args.no_color || env NO_COLOR set` -> Plain; `!stdout().is_terminal()` -> Plain; else Rich). DWS: same 0 hits; DWS's env list additionally has BR_SESSION/BR_HISTORY_MAX_BYTES/GH_TOKEN but still no CLICOLOR/BD_AGENT_MODE.

**Upstream evidence**

> internal/ui/terminal.go:27-58 `ShouldUseColor` honoring BD_GIT_HOOK=1 (GH#1303, prevents OSC 11 background-query escape leakage), NO_COLOR, CLICOLOR=0, CLICOLOR_FORCE, TERM=dumb, then TTY; :60-121 `ShouldUseHyperlinks` with an OSC-8 terminal allowlist (WT_SESSION, KITTY_WINDOW_ID, WEZTERM_EXECUTABLE, KONSOLE_VERSION, DOMTERM, GHOSTTY_RESOURCES_DIR, TERM_PROGRAM, VTE_VERSION) plus FORCE_HYPERLINK; :123-134 `ShouldUseEmoji` with BD_NO_EMOJI; internal/ui/styles.go:81-88 `IsAgentMode` honoring BD_AGENT_MODE=1; internal/uimd/markdown.go:22-26 `WrapWidth` returns 0 in agent mode so bodies are emitted verbatim; BD_NON_INTERACTIVE appears across 19 files (e.g. cmd/bd/init.go, cmd/bd/bootstrap.go `isNonInteractiveInit`/`isNonInteractiveBootstrap` with precedence "explicit flag > BD_NON_INTERACTIVE env > CI env > terminal detection").

</details>

<details><summary><b>No `br config schema` — no machine-readable inventory of honored config keys, aliases, types, or defaults</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** small

**Description**

> DWS added a `br config schema` subcommand that enumerates every config key the tool honors together with its aliases, value type, and default, renderable as JSON. That is a self-describing machine contract for configuration — an agent can discover valid keys without scraping `br config list` output or reading config/mod.rs. LOCAL's `br config list` is a dump of the current merged values, which cannot distinguish an unset key with a default from a key that does not exist at all. This is the one finding where LOCAL is behind the Rust upstream rather than behind Go, so it is a straight port from DWS.

**Impact**

> An agent cannot programmatically learn the valid config key set, their aliases, or their defaults, so config authoring depends on prose docs and trial-and-error against `br config set`.

**Local evidence**

> `rg -n 'ConfigCommands::Schema|KNOWN_CONFIG_KEYS' src/` -> 0 hits. src/config/mod.rs has no exported key registry (the 8481-line module loads layers via `load_project_config`/`load_user_config`/`load_legacy_user_config` at :3583/:3592/:3610 and merges them in `ConfigLayer::merge`, but publishes no introspection table). src/cli/mod.rs:3461-3511 `enum ConfigCommands` has no `Schema` variant, and the dispatch at src/cli/commands/config.rs:218-235 handles only Path/Edit/List/Set/Delete/Get.

**Upstream evidence**

> src/cli/mod.rs:3199-3204 `/// List every config key br honors, with aliases, types, and defaults` / `Schema { #[arg(long, value_enum, default_value_t = OutputFormatBasic::Text)] format: OutputFormatBasic }` inside `enum ConfigCommands`; src/cli/commands/config.rs:220 `ConfigCommands::Schema { format } => {`; :374 `let keys = crate::config::KNOWN_CONFIG_KEYS;`; :415-423 `push_unique_config_alias` / `config_key_aliases`; src/config/mod.rs:7111 `pub const KNOWN_CONFIG_KEYS: &[ConfigKeySpec] = &[` consumed at :7265 and :7283.

</details>

<details><summary><b>Structured JSON error envelope goes to stdout, and the published schema doc contradicts the implementation</b></summary>

- **Severity:** info · **Present in:** go-only · **Effort:** small

**Description**

> LOCAL deliberately routes the structured JSON error envelope to STDOUT in --json mode so a robot reads one clean stream, where GO routes the default path to STDERR. This is a documented local decision, but the repo's own published contract for the same envelope says stderr: `br schema error` emits a schema whose doc comment states the envelope goes to stderr, so `br schema` and `br` disagree about where to read it. The envelope shape itself is richer than GO's (code/message/hint/retryable/context vs GO's error/code/hint), so this is a channel-only divergence.

**Impact**

> Bounded: both channels are single-purpose and LOCAL's stdout choice is stated in code. Reported because the machine-readable schema `br schema` publishes is wrong about its own channel, which will mislead a consumer that trusts `br schema` over observed behavior. Fixing the doc comment is cheaper than changing the channel.

**Local evidence**

> src/main.rs:1271-1281 `handle_error`: "#336: In `--json` mode, route the structured JSON error envelope to STDOUT (where success JSON already goes) so robot callers read ONE clean, parseable stream" then `println!`; the human path uses `eprintln!` at :1286. Contradicting doc: src/cli/commands/schema.rs:121 "On error, the same command writes an `ErrorEnvelope` to stderr." and :279 "On a missing id, an ErrorEnvelope is written ...", published via `br schema error` / `br schema all` (src/cli/commands/schema.rs:202,236) with no channel field on the schema (struct at :33-49 is just `{error: ErrorBody}`). Payload shape is src/error/structured.rs:498-508 `{error:{code,message,hint,retryable,context}}`.

**Upstream evidence**

> cmd/bd/errors.go:125-129 `jsonStderrError` (the default HandleErrorWithHint path, :150-156) writes to os.Stderr; cmd/bd/errors.go:131-135 `jsonStdoutError` used only by `HandleErrorRespectJSON` (:142-148); cmd/bd/errors.go:62-77 `buildJSONError` emits `{error, hint?, schema_version}` and `{schema_version, data:{error,hint}}` in envelope mode; cmd/bd/output.go:114-134 `outputJSONError` writes to os.Stderr and returns `&exitError{Code: 1}`.

</details>

### C — `concurrency-correctness` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | both-upstreams | `missing_feature` | No optimistic-concurrency control on update — concurrent agent edits silently lose data |

<details><summary><b>No optimistic-concurrency control on update — concurrent agent edits silently lose data</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> LOCAL has no compare-and-set precondition on `br update`. Two agents (or an agent and a human) that both read an issue, then both write it, will have the second write silently clobber the first with no error, no exit code, and no way for the loser to detect the loss. DWS ships a full optimistic-concurrency precondition end to end — CLI flag, MCP tool field, storage-layer enforcement inside the write transaction, and a distinct exit code — and additionally advertises the guarantee in the MCP tool description so agents know to use it. LOCAL's multi-agent claim is its stated reason for existing, so this is the gap that most directly undercuts the product's purpose.

**Impact**

> This is the lost-update race that the model-lifecycle domain's own GO-only `row_version` finding gestures at, but in its user-facing Rust form: an optional flag an agent can actually pass, enforced at the storage layer, surfaced through both the CLI and the MCP tool an agent calls. Without it, concurrent `br update` calls — the normal case in a multi-agent swarm, which is LOCAL's headline use case — are last-writer-wins with exit code 0. The MCP half matters most: LOCAL's `update_issue` tool has no way for an agent to even express the precondition, and the tool description gives the agent no reason to try. Adjacent to the model-lifecycle domain's row_version finding but distinct: that is a wire-schema field, this is a flag plus a write-path guard plus an exit code.

**Local evidence**

> `grep -rn 'if_unchanged' --include=*.rs src/ tests/` -> 0 hits. `grep -rn 'expect_updated_at' src/` -> 0 hits. `grep -rn 'lost_update' src/` -> 0 hits. `grep -rn 'UpdatePreconditionFailed' src/` -> 0 hits; LOCAL's `BeadsError` enum (src/error/mod.rs:30-205) has no precondition variant. LOCAL's 5 `optimistic` hits are unrelated (src/config/mod.rs:6935, :6963 — a startup config-cache torn-read witness). LOCAL's 23 `precondition` hits are all filesystem-scope preconditions in the doctor mutate chokepoint (src/cli/commands/doctor_subsystems/mutate.rs:13, :55, :1199; src/cli/commands/doctor.rs:10827), not issue-row concurrency. LOCAL's only concurrency-loss concept is doctor lock contention: `DoctorExitCode::ConcurrencyLost` (src/cli/commands/doctor_subsystems/exit_codes.rs, value 5), documented at exit_codes.rs:8 as "could not acquire `.write.lock` within the 5 s envelope" — a workspace-lock concern, not a read-modify-write concern.

**Upstream evidence**

> DWS src/cli/mod.rs:1338-1345: `pub if_unchanged: Option<String>` with help "Only apply this update if the issue has not changed since you read it (GitHub #500) ... If the record has moved since, nothing is written and the command exits 6 naming both timestamps, so a caller can re-read and retry instead of silently discarding the other writer's revision." Parsing: DWS src/cli/commands/update.rs:1892 and the shared surface parser :1914. Storage enforcement inside the write transaction: DWS src/storage/sqlite.rs:7565-7580, `if let Some(expected) = updates.expect_updated_at && issue.updated_at != expected { return Err(BeadsError::UpdatePreconditionFailed { id, expected, actual }) }`, with the comment "Compared as instants rather than strings ... and inside the write transaction so the answer cannot go stale between check and write." Field: src/storage/sqlite.rs:18498. MCP surface: DWS src/mcp/tools.rs:2398 declares the `if_unchanged` property, :2368 advertises "Lost updates: pass if_unchanged with the updated_at you read to make the write conditional on nobody having edited the issue since", :2110 `ensure_update_precondition` covers the paths that bypass `update_issue`, and :5269-5322 pin it with four regression tests including one asserting `if_unchanged` is a precondition and not a field update. Guard against multi-target misuse: update.rs:395 and :660 refuse `if_unchanged` with more than one id.

</details>

### C — `data-integrity-guards` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | dws-only | `missing_feature` | No guard against destructive truncation of context-accumulating fields on update |

<details><summary><b>No guard against destructive truncation of context-accumulating fields on update</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> LOCAL accepts any replacement value for `description`, `design`, `acceptance_criteria`, `notes`, `prerequisites` and `agent_context`, so an agent that pipes a truncated read, a partial heredoc, or a failed fetch into `--description` silently destroys accumulated project context with no error and no way to notice. DWS implements a three-tier retention guard that refuses the write unless `--force` is passed, and pairs it with in-place append/edit flags so a caller who legitimately wants to extend a field never has to rewrite it. LOCAL has the `--force` flag but no semantics attached to it, and only a single-valued `notes_push` in place of DWS's repeatable `append_notes`.

**Impact**

> Silent, unrecoverable loss of the context an issue accumulates — the single most valuable and most expensive-to-recreate data in the tracker. For an agent-first CLI this is a first-order failure mode: partial reads and truncated command substitutions are routine, and nothing in LOCAL's output, exit code, or error vocabulary distinguishes a 3-character description from a deliberate one. It also makes the `--force` flag that LOCAL already exposes a no-op, so callers following upstream-documented advice get no protection.

**Local evidence**

> `grep -cEi 'destructive|shorter than half' src/cli/commands/update.rs` -> 0. `grep -rn append_notes --include=*.rs src/` -> 0 hits. LOCAL `UpdateArgs` (src/cli/mod.rs) exposes `pub force: bool` and `pub notes_push: Option<String>` with no guard logic anywhere in src/cli/commands/update.rs. LOCAL's `BeadsError` enum (src/error/mod.rs:30-205) has no truncation/shrink variant; the only `PolicyViolation` variant (:200) governs *close* gates, not field rewrites.

**Upstream evidence**

> DWS src/cli/commands/update.rs:1576-1673 implements the guard — the module comment enumerates the three refusal tiers: "1. ... 2. incoming empty, or shorter than half the current length (in chars): 3. otherwise, fewer than half of the current words kept", with "Keeps at least half the length and at least half the words" passing (:1591) and "Clears the field or shrinks it below half its current length: refused" (:1601). The `--force` help text at DWS src/cli/mod.rs:1344-1352 names GitHub #467/#481 and the same half-length rule. The refusal message at update.rs:1753 points the caller at the safe alternatives: "run `br show <id>` first to read what is there, or use --append-notes / --add-acceptance to extend instead of replace." The in-place flags are DWS src/cli/mod.rs:1295 (`append_notes: Vec<String>`, repeatable), :1256 (`check_acceptance`), :1266 (`uncheck_acceptance`), :1276 (`add_acceptance`), shipped as #477/#480. Boundary regression tests: update.rs:2653 (prerequisites use the same guard), :2692 (truncation to a fragment is destructive), :2704 (clearing is destructive), :2734 (`overwrite_length_boundary_is_exactly_half`).

</details>

### C — `field-selection` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | low | dws-only | `partial` | --fields only projects CSV; it is ignored for JSON/TOON output |

<details><summary><b>--fields only projects CSV; it is ignored for JSON/TOON output</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS's `--fields` performs column projection for structured output: it parses a FieldSelection (28 allowed columns including dependency_count, dependent_count, close_reason, design, acceptance_criteria, prerequisites, created_by, estimated_minutes, source_repo) and applies it to JSON and TOON output via a streaming serializer. LOCAL's `--fields` is documented and implemented as a CSV-only picker: it feeds csv::parse_fields/format_csv in the CSV branch and is silently ignored for --json/--format toon, which always emit full records.

**Impact**

> Token-efficient agent output (the core value prop of br) cannot narrow the JSON/TOON payload: `br list --json --fields id,title,status` still returns full records with every field, so agents cannot reduce token cost via --fields.

**Local evidence**

> LOCAL --fields is only consumed in the CSV branch: src/cli/commands/list.rs:266-267 `let fields = csv::parse_fields(args.fields.as_deref()); let csv_output = csv::format_csv(&issues, &fields);` and src/cli/commands/search.rs:193 the same csv::parse_fields. The CLI doc at src/cli/mod.rs:2203-2211 literally says 'CSV fields to include'. rg -n 'FieldSelection|json_array_page' src/cli/commands/list.rs shows no projection applied on the is_json_output path.

**Upstream evidence**

> DWS src/cli/commands/list_fields.rs:14-39 ALLOWED_FIELDS (28 columns incl. design/acceptance_criteria/prerequisites/close_reason/dependency_count/dependent_count); applied to JSON at src/cli/commands/list.rs:87-90 (`if is_json_output { args.fields.map(fields::FieldSelection::parse) }`) and rendered via ctx.json_array_page/toon_with_stats (list_fields.rs:113-136). Search reuses it at src/cli/commands/search.rs:130 (`.map(FieldSelection::parse)`).

</details>

### C — `filtering` (4 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | low | go-only | `partial` | Sort keys closed/status/id/type/assignee unsupported |
| — | low | go-only | `missing_flag` | No regex/glob label matching (--label-regex, --label-pattern) |
| — | low | go-only | `missing_flag` | No filter by due_at or defer_until date range |
| — | low | go-only | `missing_flag` | No absence/empty filters (--no-labels, --empty-description) |

<details><summary><b>Sort keys closed/status/id/type/assignee unsupported</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> LOCAL's list and search validate --sort against only priority, created(_at), updated(_at) and title, and return a validation error for any other key. GO's sort supports nine keys: priority, created, updated, closed, status, id, title, type, assignee. So `br list --sort closed|status|id|type|assignee` fails where the equivalent bd works. DWS carries the same 4-key set as LOCAL, so this is a GO-only capability.

**Impact**

> Cannot sort by closure time, status, id, type, or assignee; e.g. triage 'show closed items most-recently first' or 'group by status' is impossible.

**Local evidence**

> src/cli/commands/list.rs:854-860 and src/cli/commands/search.rs:518-524 both validate_sort_key match only '"priority" | "created_at" | "created" | "updated_at" | "updated" | "title"' and error otherwise. rg -c '"closed" *=>|"status" *=>|"id" *=>|"type" *=>|"assignee" *=>' src/cli/commands/list.rs src/cli/commands/search.rs -> 0. DWS has the identical 4-key set (src/cli/commands/list.rs:884, src/cli/commands/search.rs:755).

**Upstream evidence**

> GO internal/workapi/sort.go:13-38 CompareIssuesBy switch handles priority, created, updated, closed, status, id, title, type, assignee; exposed as `Sort by field: priority, created, updated, closed, status, id, title, type, assignee` (cmd/bd/search.go:292, cmd/bd/list.go list --sort help).

</details>

<details><summary><b>No regex/glob label matching (--label-regex, --label-pattern)</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> GO supports pattern-based label selection: --label-pattern (SQL glob, e.g. 'tech-*') and --label-regex (e.g. 'tech-(debt|legacy)'). LOCAL's label matching is exact (case-insensitive equality) in both the SQL filter path and the DSL predicate, with no glob or regex option anywhere, so labels can only be matched one exact value at a time.

**Impact**

> Cannot filter by a family of labels (e.g. all tech-* debt labels) in a single query; the agent must enumerate every label value or post-filter externally.

**Local evidence**

> rg -rc 'label_regex|label_pattern|label-regex|label-pattern' src/ -> 0. DSL label predicate is exact equality: src/query/evaluator.rs:554-557 'labels' => ComparisonOp::Eq => issue.labels.iter().any(|l| l.eq_ignore_ascii_case(value)) (NotEq negates the same exact match). The storage label filter (label_filter_candidate_ids) also does exact membership. A glob like 'tech-*' would be compared literally, so it matches nothing.

**Upstream evidence**

> GO cmd/bd/list.go:414 `--label-pattern` "Filter by label glob pattern (e.g., 'tech-*' ...)" and :415 `--label-regex` "Filter by label regex pattern (e.g., 'tech-(debt|legacy)')".

</details>

<details><summary><b>No filter by due_at or defer_until date range</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> GO exposes due-date and defer-date range filters on list/search/search via --due-after/--due-before and --defer-after/--defer-before, backed by IssueFilter.DueAfter/DueBefore/DeferAfter/DeferBefore and implemented in the SQL builder. LOCAL stores and indexes due_at/defer_until but exposes no filter for them on any command, and its query DSL has no due/defer field, so the capability is unreachable. (closed-date is NOT in scope here: LOCAL's DSL does support a `closed`/`closed_at` field.)

**Impact**

> Cannot ask 'what is due this week' or 'deferred until after X' via any br filter or DSL field, despite the data being stored.

**Local evidence**

> rg -c '"due"|"defer"|"due_at"|"defer_until"' src/query/evaluator.rs -> 0 (DSL has no due/defer field). rg -c 'due_after|due-after|defer_after|defer-before|closed_after' src/cli/mod.rs src/cli/commands/search.rs -> 0 (no CLI flags; the only ListArgs date flags are created/updated before/after). The only use of defer_until in LOCAL storage is a hidden readiness gate, not a filter: src/storage/sqlite.rs:5530 `AND (defer_until IS NULL OR datetime(defer_until) <= datetime('now'))`. due_at/defer_until exist only as model fields/indexes (src/model/mod.rs:928,932; schema indexes at sqlite.rs:20383-20384).

**Upstream evidence**

> GO cmd/bd/list.go:505-508 registers --defer-after/--defer-before/--due-after/--due-before; fields IssueFilter.DueAfter/DueBefore/DeferAfter/DeferBefore in internal/types/types.go; implemented as defer_until/due_at range predicates in internal/storage/sqlbuild/filter.go.

</details>

<details><summary><b>No absence/empty filters (--no-labels, --empty-description)</b></summary>

- **Severity:** low · **Present in:** go-only · **Effort:** small

**Description**

> GO can filter for absence/emptiness directly: --no-labels (issues with no labels) and --empty-description (description NULL or empty). LOCAL has no equivalent flag and its DSL has no way to express these absence predicates: there is no has-labels/label-count field, and the description DSL comparison is substring `contains`, so `description=""` would match every issue rather than the empty ones. (This finding deliberately excludes --exclude-label/--exclude-type, which LOCAL can approximate via `NOT label=x` / `NOT type=x` in its DSL.)

**Impact**

> Cannot isolate untagged or undocumented issues for hygiene sweeps without external post-processing.

**Local evidence**

> rg -rc 'no_labels|empty_description|no-labels|empty-description' src/ -> 0. DSL has no label-count/has-labels field; the description predicate is substring-based: src/query/evaluator.rs:559-567 ComparisonOp::Eq => issue_desc.contains(&search), so an empty value matches all. LOCAL ListFilters expose no empty-description or no-labels field.

**Upstream evidence**

> GO cmd/bd/list.go:445 `--no-labels` "Filter issues with no labels" and :443 `--empty-description` "Filter issues with empty or missing description"; empty-description predicate in internal/storage/sqlbuild/filter.go:275 '(description IS NULL OR description = '')'.

</details>

### C — `graph-deps` (11 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | dws-only | `architecture_gap` | Dependency table is keyed on (issue_id, depends_on_id) only: a pair can carry exactly ONE dependency type; upstream keys on (issue_id, depends_on_id, type) |
| — | low | both-upstreams | `missing_feature` | No bulk dependency-edge import command (`br dep import <path.jsonl>`) |
| — | low | dws-only | `missing_feature` | Workflow-capacity (WIP admission control) engine with hierarchy-aware counting over parent-child edges |
| — | low | go-only | `missing_flag` | GO `dep add` bulk-wiring switches: `--file`/`-` (stdin), `--no-cycle-check`, and flag-style target entry (`-b/--blocks`, `--blocked-by`, `--depends-on`) |
| — | low | dws-only | `divergent_behavior` | Priority filter rejects ranges and comma lists: `br ready -p 0-1` errors instead of expanding |
| — | low | go-only | `missing_flag` | GO-only filter vocabulary on ready/blocked: exclude-label, label glob/regex, exclude-type, metadata-field predicates, and `ready --explain` |
| — | low | both-upstreams | `missing_flag` | `ready --brief`: compact JSON/TOON payload that strips long free-text for agent callers |
| — | low | dws-only | `missing_flag` | `gate report --to <status>`: bind a gate verdict to the specific target status it authorizes |
| — | info | go-only | `partial` | `dep tree --status` status filter and a 5x deeper default depth (50 vs 10) |
| — | info | go-only | `missing_feature` | `waits-for` fanout gate with `any-children` semantics (open after the first child closes) |
| — | info | dws-only | `missing_flag` | `epic close-eligible --transition-comment`: commit an audit comment atomically with each bulk epic close |

<details><summary><b>Dependency table is keyed on (issue_id, depends_on_id) only: a pair can carry exactly ONE dependency type; upstream keys on (issue_id, depends_on_id, type)</b></summary>

- **Severity:** critical · **Present in:** both-upstreams · **Effort:** large

**Description**

> LOCAL's `dependencies` table has PRIMARY KEY (issue_id, depends_on_id) and its add-path existence probe omits `type`, so a second relationship type between the same pair is silently rejected and the first type is kept. DWS uses a 3-column PK (schema cookie literally named `v19-typed-dependencies-exact-ddl...`) and its add-path probes `(issue_id, depends_on_id, type)`, so `blocks` + `related` + `discovered-from` edges can coexist. GO's legacy-SQLite reader contract also declares the 3-column PK, so both upstreams round-trip multi-type edges. Because the constraint is in the table DDL, this is not a missing flag — it is unfixable without a schema migration + storage-layer + JSONL-merge change, and it propagates: LOCAL's `dep remove` (src/storage/sqlite.rs:7723) deletes every row for the pair with no type filter, while DWS's remove_dependency errors out ('multiple dependency types connect X to Y; specify --type') unless a type is named (src/storage/sqlite.rs:12905-12911). LOCAL has no `dep remove --type` either (DepRemoveArgs src/cli/mod.rs:2607-2616 has only issue/depends_on), so the ambiguity guard is unreachable.

**Impact**

> A team that wants `A blocks B` and `A related-to B` simultaneously cannot express it: the second `dep add` reports "exists" and drops the new type. Worse, a `br sync --import-only` of a JSONL produced by bd/DWS that contains two edges of different types for the same pair silently collapses them to one row (the PK collision), so round-tripping a dependency graph through br is lossy. This also contradicts AGENTS.md's stated 'Schema compatibility — Database schema matches Go beads for potential cross-tool usage'. Effort is large (schema migration v20 + storage + merge + conformance).

**Local evidence**

> rg -n 'PRIMARY KEY \(issue_id, depends_on_id\)' src/storage/schema.rs -> src/storage/schema.rs:140 `PRIMARY KEY (issue_id, depends_on_id),` (2-col). Add-path existence probe omits type: src/storage/sqlite.rs:7656-7664 `SELECT 1 FROM dependencies WHERE issue_id = ?1 AND depends_on_id = ?2 LIMIT 1` then `if !existing.is_empty() { return Ok(false); }` -> `br dep add A B -t related` after `-t blocks` returns status "exists" and the type is NOT recorded. Remove path: src/storage/sqlite.rs:7723 `DELETE FROM dependencies WHERE issue_id = ?1 AND depends_on_id = ?2` (no type). `rg -n 'pub dep_type' src/cli/mod.rs` inside DepRemoveArgs (2607-2616) -> 0 hits; the only `pub dep_type` is in DepAddArgs at 2599. CHANGELOG.md:758 records only 'Backfill dependency type column' — no statement that one-type-per-pair is an intentional limitation.

**Upstream evidence**

> DWS src/storage/schema.rs:359 `PRIMARY KEY (issue_id, depends_on_id, type),` and src/storage/schema.rs:24 schema cookie `"v19-typed-dependencies-exact-ddl-version-domain-cookie-fenced"`. DWS src/storage/sqlite.rs:12618-12625 `SELECT 1 FROM dependencies WHERE issue_id = ? AND depends_on_id = ? AND type = ? LIMIT 1`. DWS src/storage/sqlite.rs:12886-12929 `remove_dependency(..., dep_type: Option<&str>, ...)` with the ambiguity guard at 12905-12911 `"multiple dependency types connect {issue_id} to {depends_on_id}; specify --type to remove one relationship"`. DWS src/cli/mod.rs:2269-2270 `/// Dependency type to remove (required when the pair has multiple types)` / `pub dep_type: Option<String>` on DepRemoveArgs. GO internal/migration/legacysqlite/reader_test.go:979 `CREATE TABLE dependencies (... PRIMARY KEY(issue_id,depends_on_id,type), ...)`.

</details>

<details><summary><b>No bulk dependency-edge import command (`br dep import <path.jsonl>`)</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS ships `br dep import <path>` as a first-class subcommand that reads newline-delimited JSON of either raw edges (`issue_id` + `depends_on_id` + optional `dep_type`) or full issue records carrying a `dependencies[]` array, and inserts edges only — it does not create or overwrite issues. LOCAL has no such subcommand: the only ways to get edges in are `br dep add` one at a time, or `br sync --import-only` / `br import`, which are whole-file issue imports. DWS's DepAddArgs is byte-identical to LOCAL's, so the capability is genuinely a separate subcommand rather than a flag LOCAL missed.

**Impact**

> Wiring a large graph from an external planner means either N `br dep add` invocations (each with its own process start, write-lock acquire and optional JSONL flush) or importing a whole issues.jsonl and letting unrelated issue records overwrite existing state. An edge-only import has no LOCAL equivalent. Effort: small-to-medium (self-contained new subcommand reusing existing read+insert paths).

**Local evidence**

> `rg -n 'DepCommands::Import|execute_dep_import|DepImportArgs|BulkDependencyInsert' /Users/tranquangdang21/Projects/beads_rust/src` -> 0 files. LOCAL DepCommands (src/cli/mod.rs, enum at 817-825) = Add/Remove/List/Tree/Cycles only. src/cli/commands/import.rs:1-3 documents the scope as 'Imports issues from JSON, CSV, or markdown files into the database. Uses existing sync infrastructure for JSONL imports via `br sync --import-only`.' — i.e. issue-level, not edge-only.

**Upstream evidence**

> DWS src/cli/mod.rs:810-816 `Dep { #[command(subcommand)] command: DepCommands, }` with `Import(DepImportArgs)` as a DepCommands variant; DWS src/cli/commands/dep.rs:41 `DepCommands::Import(args) => execute_dep_import(...)`, :132 `fn execute_dep_import`, :366 `fn read_dependency_imports`, :656 `fn dep_import`; DWS src/cli/mod.rs:2250-2258 `pub struct DepImportArgs { pub path: PathBuf, pub robot: bool }`. Parser at dep.rs:393-451 handles both the flat-edge and `dependencies[]` shapes.

</details>

<details><summary><b>Workflow-capacity (WIP admission control) engine with hierarchy-aware counting over parent-child edges</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** large

**Description**

> DWS enforces WIP limits on status transitions using the dependency hierarchy: a policy declares capacity requirements per `from -> to` transition, and occupancy is computed over candidate issues using four counting modes — `all`, `leaf_work` (an issue stops counting once a parent-child descendant is counted, so an epic doesn't double-count its active leaves), `roots` (count by highest matching parent-child ancestor), and `weighted`. Only `parent-child` edges participate; `blocks`/`related` never affect counts. Over `hard_limit` the transition is rejected with a structured `WorkflowCapacityViolation` (current / prospective / soft_limit / hard_limit / exempt / scope_key / counting_mode); crossing `soft_limit` emits a `WorkflowCapacityWarning`. DWS also ships audited per-issue exemptions (`br capacity exempt/renew/revoke/exemptions`). LOCAL has none of this: no `capacity` subcommand, no `Capacity*` types, and no capacity branch in close_policy.

**Impact**

> On LOCAL, nothing stops an operator or a swarm from starting 40 children of one epic simultaneously — the 'one work stream at a time per epic' invariant DWS encodes is simply not expressible, and there is no soft/hard WIP signal at all. This is the scheduling half of the graph domain and is entirely absent, not degraded. Effort: large (policy schema, counting engine, transition integration, evidence structs).

**Local evidence**

> `rg -n 'CapacityCommands|CapacityAdmissionRule|CapacityCountingMode|CapacityTransitionMatcher|WorkflowCapacityViolation' /Users/tranquangdang21/Projects/beeds_rust/src` -> 0 files. `rg -i -c 'capacity' src/close_policy.rs` -> 1, and that single hit is src/close_policy.rs:606 `let mut out: Vec<String> = Vec::with_capacity(source.len());` (unrelated). LOCAL `Commands` enum (src/cli/mod.rs:750-1210) has no `Capacity` variant. `rg -n 'Capacity' docs/` -> 0 product-doc hits.

**Upstream evidence**

> DWS src/cli/mod.rs:765-768 `/// Workflow capacity management: audited issue-specific exemptions (GitHub #384)` / `Capacity { #[command(subcommand)] command: CapacityCommands, }`; DWS src/cli/commands/capacity.rs:1-14 (module doc for `br capacity exempt/renew/revoke/exemptions`); DWS src/close_policy.rs:509 `pub struct CapacityAdmissionRule`, :528 `pub struct CapacityCounting`, :538 `pub enum CapacityCountingMode { All, LeafWork, Roots, Weighted }`, :604 `pub struct WorkflowCapacityViolation`, :642 `pub struct WorkflowCapacityWarning`. GO has no equivalent (`ls cmd/bd | grep -i capac` -> empty).

</details>

<details><summary><b>GO `dep add` bulk-wiring switches: `--file`/`-` (stdin), `--no-cycle-check`, and flag-style target entry (`-b/--blocks`, `--blocked-by`, `--depends-on`)</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> GO's `dep add` accepts the target as a flag as well as a positional (`-b/--blocks` for the reverse direction, `--blocked-by`/`--depends-on` as aliases), reads bulk edges from a JSONL file or stdin via `--file -`, and exposes `--no-cycle-check` to skip the per-edge cycle probe during bulk wiring (bulk `--file` adds still run one final whole-graph check before commit). LOCAL's DepAddArgs is positional-only with no bypass; DWS's DepAddArgs is byte-identical to LOCAL's, so neither has the flag, and DWS's bulk capability lives in a separate `dep import` subcommand instead (see the `dep import` finding). The no-cycle-check path is a real throughput lever for wiring hundreds of edges under concurrent agents.

**Impact**

> Bulk graph wiring on br cannot bypass the per-edge cycle probe, so seeding a large graph is O(edges x graph) inside the write lock; and a caller that prefers flags over positionals (common in generated agent scripts) has no equivalent form. Effort: small for the flags, medium for `--file` (needs a bulk insert path with one final whole-graph cycle check).

**Local evidence**

> LOCAL DepAddArgs src/cli/mod.rs:2588-2604 = `issue`, `depends_on_id`, `--type`/`-t`, `--metadata` — no file, no cycle-check bypass, no flag-style target. `rg -n 'no_cycle_check|--no-cycle-check' /Users/tranquangdang21/Projects/beads_rust/src` -> 0 files. LOCAL's per-edge cycle probe is unconditional and runs inside the write transaction: src/cli/commands/dep.rs:395-401 `storage.would_create_cycle(&issue_id, &depends_on_id, true)?`.

**Upstream evidence**

> GO cmd/bd/dep.go:1544 `depAddCmd.Flags().String("file", "", "Read dependency edges from JSONL file, or '-' for stdin")`; :1545 `depAddCmd.Flags().Bool("no-cycle-check", false, "Skip per-edge cycle checks for speed (bulk wiring); bulk --file adds still run one final whole-graph check before commit")`; :1538 `depCmd.Flags().StringP("blocks", "b", ...)`; :1542 `--blocked-by`; :1543 `--depends-on`; :1539 the parent-command-level `--no-cycle-check`; usage documented at cmd/bd/dep.go:266-269 and :285-289.

</details>

<details><summary><b>Priority filter rejects ranges and comma lists: `br ready -p 0-1` errors instead of expanding</b></summary>

- **Severity:** medium · **Present in:** both-upstreams · **Effort:** small

**Description**

> DWS routes every priority filter through one shared parser that splits each argument on ',', expands `a-b` ranges inclusively, dedupes through a BTreeSet, and rejects backwards ranges. The same parser backs list, ready, blocked and count, so `-p 0-1` means the same thing everywhere. LOCAL instead calls `Priority::from_str` per token, and `from_str` only accepts a single integer 0-4 (optionally `P`-prefixed) — so `0-1`, `0,2` and `P0-P1` all fail with InvalidPriority. LOCAL's `list` and `blocked` have the same single-value parser, so the divergence is repo-wide, not `ready`-only.

**Impact**

> Triage queries that need a band — the common 'P0 and P1 only' case — hard-fail on br with an opaque InvalidPriority error while working on DWS. This is a pure grammar gap in a filter that agents script against, so it produces hard command failures rather than subtly different results. Effort: small (port `parse_priority_filter` and call it from the four sites).

**Local evidence**

> `rg -n 'parse_priority_filter' /Users/tranquangdang21/Projects/beads_rust/src` -> 0 files. LOCAL src/cli/commands/ready.rs:407-418 `for p in priorities { parsed.push(Priority::from_str(p)?); }`; src/model/mod.rs:172-185 `match val.parse::<i32>() { Ok(p) if (0..=4).contains(&p) => Ok(Self(p)), ... Err(_) => Err(BeadsError::InvalidPriority { priority: val.to_string() }) }` — `"0-1".parse::<i32>()` is Err, so the command aborts. src/cli/commands/list.rs:537-546 uses the same `.map(|p| p.parse())`. The LOCAL `ReadyArgs.priority` doc (src/cli/mod.rs ~3100) says only 'can be repeated (0-4)'.

**Upstream evidence**

> DWS src/validation/mod.rs:38 `pub fn parse_priority_filter(values: &[String]) -> crate::error::Result<Vec<Priority>>` (splits on ',', `token.split_once('-')` expands inclusively, errors on backwards range) — doc comment at :35-37 'This is the one parser behind list, ready, blocked, and count'. DWS src/cli/commands/ready.rs:407-412 `fn parse_priorities(...) { ... Ok(Some(crate::validation::parse_priority_filter(priorities)?)) }`; DWS src/cli/mod.rs ReadyArgs.priority doc 'Filter by priority: 0-4 or P0-P4, ranges like 0-1, comma lists; repeatable'. DWS src/cli/commands/list.rs:549 'accepts single values, ranges (`0-1`), and comma lists'.

</details>

<details><summary><b>GO-only filter vocabulary on ready/blocked: exclude-label, label glob/regex, exclude-type, metadata-field predicates, and `ready --explain`</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> GO's ready/blocked accept an exclusion and pattern vocabulary LOCAL has no equivalent for: `--exclude-label`, `--label-pattern` (glob, e.g. `tech-*`), `--label-regex`, `--exclude-type`, plus generic metadata predicates `--metadata-field key=value` and `--has-metadata-key`. `ready` also takes `--offset` for result-window pagination. The most domain-relevant is `--explain`, which renders dependency-aware reasoning for why each issue is ready or blocked. GO's `blocked` additionally takes `--parent` to scope to a bead/epic subtree, which neither LOCAL nor DWS offers (LOCAL's ReadyArgs has `--parent`/`--epic`; BlockedArgs has neither). DWS's ReadyArgs is byte-identical to LOCAL's apart from `--brief`, so this is a genuine GO-only divergence.

**Impact**

> To select 'all open non-epic work that is not tech-debt', a br user must enumerate matching labels with repeated `--label` (AND) or `--label-any` (OR) and then filter client-side — `--label-any` cannot express negation, so there is no server-side way to exclude. `--explain` in particular has no br analogue at all: when the ready set looks wrong there is no command that explains which edge is holding an issue back. Effort: medium (pattern/regex filters need label-scan plumbing; --explain needs a per-issue reason walk).

**Local evidence**

> `rg -n 'exclude_label|label_pattern|label_regex|exclude_type|metadata_field|has-metadata-key' /Users/tranquangdang21/Projects/beads_rust/src` -> 0 files for all six. `has_metadata_key` appears once, at src/query/evaluator.rs:423 and :640, but only as a saved-query predicate — and at :430 it explicitly errors with 'has_metadata_key filtering requires predicate (no SQL-side filter)', so it is not a CLI filter. LOCAL BlockedArgs (src/cli/mod.rs:3153-3193) has limit/detailed/wrap/type_/priority/label/format/stats/robot and no `parent`. LOCAL ReadyArgs has no `offset`; the three LOCAL `offset` fields (src/cli/mod.rs:1401, 2141, 2305) belong to list/search, not ready.

**Upstream evidence**

> GO cmd/bd/ready.go:766 `--exclude-label`, :767 `--label-pattern` ('tech-*' matches tech-debt, tech-legacy), :768 `--label-regex`, :779 `--exclude-type`, :780 `--explain` ('Show dependency-aware reasoning for why issues are ready or blocked'), :759 `--offset`, :791 `--metadata-field`, :792 `--has-metadata-key`; GO cmd/bd/ready.go:796 `blockedCmd.Flags().String("parent", "", "Filter to descendants of this bead/epic")`, :799 blocked `--exclude-label`.

</details>

<details><summary><b>`ready --brief`: compact JSON/TOON payload that strips long free-text for agent callers</b></summary>

- **Severity:** medium · **Present in:** both-upstreams · **Effort:** small

**Description**

> Both upstreams let a caller ask for the ready set without paying for description text. DWS threads a `brief: bool` from the CLI all the way into `get_ready_issues_for_output`, which picks the summary projection for JSON/TOON; its flag doc quantifies the win (~1.2 MB -> ~110 KB on a 10k-issue tracker, descriptions being ~89% of the payload). GO has the same flag. LOCAL's `get_ready_issues_for_output` takes only `(storage, filters, sort_policy, output_format)` and always uses the full projection, so every `br ready --json` on br ships every description.

**Impact**

> On br, the documented agent workflow (`br ready --json | jq`) pays the full description payload on every poll, which is the single largest avoidable token cost in the ready loop — and br is the one whose AGENTS.md tells agents to use `--json`/`--robot` exclusively. Effort: small (the summary projection already exists; LOCAL already calls `get_ready_summary_issues_for_command_output` for Text/Csv).

**Local evidence**

> `rg -n 'pub brief' /Users/tranquangdang21/Projects/beads_rust/src` -> 0 files (the only 'brief' hits in src/ are the unrelated `format_date_brief` helper in src/cli/commands/changelog.rs:300-306). LOCAL src/cli/commands/ready.rs:342-355 `fn get_ready_issues_for_output(storage, filters, sort_policy, output_format) -> Result<Vec<Issue>>` with `OutputFormat::Json | OutputFormat::Toon => storage.get_ready_issues_for_command_output(filters, sort_policy)` — no brief branch. LOCAL `ReadyArgs` (src/cli/mod.rs:3040-3140) has no brief field.

**Upstream evidence**

> DWS src/cli/mod.rs:2773-2779 `/// Omit long free-text fields from JSON/TOON output, keeping only what is needed to choose work ...` / `#[arg(long)] pub brief: bool`; DWS src/cli/commands/ready.rs:169 `get_ready_issues_for_output(storage, &filters, sort_policy, output_format, args.brief)?`. GO cmd/bd/ready.go:784 `readyCmd.Flags().Bool("brief", false, ...)`.

</details>

<details><summary><b>`gate report --to <status>`: bind a gate verdict to the specific target status it authorizes</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** small

**Description**

> DWS's gate verdict is a state-machine edge, not just a pass/fail stamp: `--to` names the status the verdict is intended to authorize, and `resolve_gate_target` resolves it against the issue's current status and the configured per-transition gate rules (omitting it is allowed only when exactly one gated target matches). LOCAL's `gate report` records id/gate/provider/status/note with no target binding. The two gates are complementary rather than nested: LOCAL ships gate waiters (`gate add-waiter/remove-waiter/list-waiters`) that DWS lacks, so LOCAL is ahead on notification but behind on verdict semantics.

**Impact**

> On LOCAL a green `ci_green` result is an untyped fact: the same recorded pass can be read as authorizing any later transition, so multi-transition gate policies (e.g. security sign-off required for `in_progress -> review` but not for `review -> closed`) cannot be enforced precisely. Users must encode everything in the label/priority conditions of `require_if` instead. Effort: small-medium (one flag plus a resolver).

**Local evidence**

> `rg -n 'resolve_gate_target' /Users/tranquangdang21/Projects/beads_rust/src` -> 0 files. LOCAL GateReportArgs src/cli/mod.rs:2512-2538 has exactly `id`, `--gate`, `--provider`, `--status`, `--note`, `--robot` — no `to`. LOCAL src/cli/commands/gate.rs has `execute_report` (:91), `execute_list` (:159), `compute_gated_transitions` (:210) and the waiter trio (:345, :386, :427) but no target resolver.

**Upstream evidence**

> DWS src/cli/mod.rs:2081-2085 `/// Target status this verdict is intended to authorize. May be omitted only when policy has exactly one matching gated target from the issue's current status for this gate.` / `#[arg(long, value_name = "STATUS", add = ArgValueCompleter::new(status_completer))] pub to: Option<String>`; DWS src/cli/commands/gate.rs:196 `fn resolve_gate_target(workflow: &Workflow, from_status: &str, requested_to: Option<&str>, gate: &str)`.

</details>

<details><summary><b>`dep tree --status` status filter and a 5x deeper default depth (50 vs 10)</b></summary>

- **Severity:** low · **Present in:** go-only · **Effort:** small

**Description**

> LOCAL and DWS ship identical `DepTreeArgs` (issue, --direction/-d, --max-depth default 10, --format text|mermaid). GO adds a `--status` filter to the tree walk and defaults `--max-depth` to 50. On a real epic whose subtree is deeper than 10 levels, LOCAL silently truncates at depth 10 and marks the truncation, so the default view differs from GO's for the same graph. The `--status` filter has no br equivalent; a user who wants only open work in a tree must render everything and post-filter.

**Impact**

> Subtle divergence rather than a hard failure: `br dep tree <epic>` on an epic deeper than 10 levels shows less than `bd dep tree` would, and there is no way to scope a tree to a single status. Both are one-line flag additions. Effort: small.

**Local evidence**

> LOCAL DepTreeArgs src/cli/mod.rs:2652-2668 = `issue`, `--direction`/`-d` (default "down"), `--max-depth` `default_value_t = 10`, `--format` (default "text"). `rg -n 'status' src/cli/commands/dep.rs` shows no tree-level status filter (the hits at :306/:331/:347/:653/:673 are the DepActionResult/DepListItem status fields, not a filter argument).

**Upstream evidence**

> GO cmd/bd/dep.go:1553 `depTreeCmd.Flags().IntP("max-depth", "d", 50, "Maximum tree depth to display (safety limit)")`; GO cmd/bd/dep.go:1556 `depTreeCmd.Flags().String("status", "", "Filter to only show issues with this status (open, in_progress, blocked, deferred, closed)")`, consumed at cmd/bd/dep_tree.go:88 and passed into `WalkTreeRequest.Status` at cmd/bd/dep_tree.go:128.

</details>

<details><summary><b>`waits-for` fanout gate with `any-children` semantics (open after the first child closes)</b></summary>

- **Severity:** low · **Present in:** go-only · **Effort:** medium

**Description**

> GO's `waits-for` edge carries an optional `metadata.gate` discriminator, and the blocked-state derivation treats `any-children` as satisfied once ANY parent-child child of the target is closed, rather than requiring all of them. GO also exposes the choice at creation time via `bd create --waits-for-gate` (default `all-children`). LOCAL and DWS both treat every `waits-for` edge as a plain blocker with no metadata inspection, so a fanout gate configured to open on the first child stays blocked until all children close. LOCAL's only `any-children` occurrence is the formula/molecule step field, which is a different subsystem (formula templates, not dependency-edge metadata) and does not affect the blocked cache.

**Impact**

> A swarm convoy expressed as 'resume when the first worker reports' remains blocked until every worker finishes, so a fanout gate that should release early does not. Narrow blast radius: only affects projects that actually set `metadata.gate` on a `waits-for` edge, and such an edge imported from bd would be silently treated as all-children. Effort: medium (metadata read in the incremental blocked-cache path plus the full rebuild).

**Local evidence**

> `rg -n 'any-children|any_children' /Users/tranquangdang21/Projects/beads_rust/src` -> 1 file, src/formula/types.rs:199 `/// Fanout gate type: "all-children" or "any-children".` (formula step field, unrelated to the blocked cache). LOCAL's blocked computation is a flat type-set check with no metadata predicate: src/storage/sqlite.rs:5863, :5923, :6592, :7010 all use `type IN ('blocks', 'conditional-blocks', 'waits-for')` and never inspect `dependencies.metadata`. DWS is identical: `rg -n 'any-children' /tmp/beads_gap_audit/dicklesworthstone_beads_rust/src` -> 0 files.

**Upstream evidence**

> GO internal/storage/schema/migrations/0047_recompute_mixed_is_blocked.up.sql:184 and :263 `JSON_UNQUOTE(JSON_EXTRACT(d.metadata, '$.gate')) = 'any-children'` gating the `waits-for` EXISTS branch; GO cmd/bd/create.go:938 `createCmd.Flags().String("waits-for-gate", "all-children", "Gate type: all-children (wait for all) or any-children (wait for first)")`; GO internal/types/types.go:1349 `// Gate type: "all-children" (wait for all), "any-children" (wait for first)`; derivation in issueops/blockedstate.go.

</details>

<details><summary><b>`epic close-eligible --transition-comment`: commit an audit comment atomically with each bulk epic close</b></summary>

- **Severity:** low · **Present in:** dws-only · **Effort:** small

**Description**

> LOCAL and DWS agree on the two `epic` subcommands (`status`, `close-eligible`) and on `--eligible-only` / `--dry-run`, but DWS adds `--transition-comment`, which commits a message alongside every epic the bulk close touches. DWS has the same field on its `update`/`close` transition args, so this is a consistent cross-cutting transition-audit feature rather than a one-off. LOCAL's bulk epic close records the close reason but cannot attach a per-transition note in the same transaction.

**Impact**

> Closing a batch of eligible epics on br leaves no per-epic narrative in the audit trail beyond the fixed close reason, so a downstream reviewer cannot tell from the comment log why each epic was auto-closed. Cosmetic-to-minor; the events table still records each close. Effort: small.

**Local evidence**

> `rg -n 'transition_comment' /Users/tranquangdang21/Projects/beads_rust/src` -> 0 files. LOCAL EpicCloseEligibleArgs src/cli/mod.rs:2480-2484 is exactly `{ dry_run: bool }`; EpicStatusArgs src/cli/mod.rs:2472-2476 is exactly `{ eligible_only: bool }`. LOCAL src/cli/commands/epic.rs `execute_close_eligible` (:120) has no comment path.

**Upstream evidence**

> DWS src/cli/mod.rs:2038-2041 `/// New comment committed atomically with every eligible epic close.` / `#[arg(long, value_name = "COMMENT", allow_hyphen_values = true)] pub transition_comment: Option<String>` on EpicCloseEligibleArgs; the same field recurs on DWS's other transition args at src/cli/mod.rs:1298-1300, :2654, :2684, :2869.

</details>

### C — `integrations-external` (7 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | go-only | `divergent_behavior` | `br web` binds a mutating HTTP API with permissive CORS and zero authentication |
| — | low | go-only | `partial` | Federation is a file-path-only stub: registered `https://` peers are always rejected at sync time, and there is no conflict strategy |
| — | info | go-only | `missing_feature` | No external issue-tracker integrations at all (GitHub, GitLab, Jira, Linear, Notion, Azure DevOps) |
| — | low | go-only | `divergent_behavior` | `br federation add --password` takes the secret as a plaintext argv value with no prompt fallback |
| — | info | go-only | `missing_feature` | No SSE / event-push surface for live remote consumers |
| — | low | go-only | `partial` | `br web` REST API is roughly 40 percent hardcoded stubs returning empty JSON |
| — | info | go-only | `missing_feature` | No credential resolution ladder — LOCAL can only read a local key file, GO can pull credentials from env, files, or an external helper process |

<details><summary><b>`br web` binds a mutating HTTP API with permissive CORS and zero authentication</b></summary>

- **Severity:** critical · **Present in:** go-only · **Effort:** medium

**Description**

> LOCAL's `br web` (feature `web`, which IS in `default = ["web"]`) binds a real TCP listener and serves CRUD+mutating REST routes with `CorsLayer::permissive()` and no authentication, no bearer token, no Host-header check, and no guard on `--host`. `br web --host 0.0.0.0` exposes unauthenticated DELETE/PATCH/POST to the network with no warning. GO's `bd serve` makes the opposite tradeoff: it REFUSES to bind beyond loopback unless BOTH `--allow-non-loopback` AND `--auth-token-file` are supplied, and ships a DNS-rebinding Host allowlist. This is a security regression, not a cosmetic difference.

**Impact**

> Any machine on the LAN (or any web page via permissive CORS) can delete every bead, flip statuses, and add deps on a developer's machine running `br web --host 0.0.0.0`, with no credential and no audit identity beyond the hardcoded `actor = "web-ui"` (src/web/api.rs:246). A hostile page visited on the same host can reach the loopback default directly because there is no Host allowlist.

**Local evidence**

> src/web/mod.rs:174 `.layer(tower_http::cors::CorsLayer::permissive())`; src/web/mod.rs:84-155 registers GET/PATCH/DELETE on `/api/p/{project_id}/beads/{id}` and POST on status/comments/deps/archive; src/cli/mod.rs:3862-3867 `pub host: String` with `default_value = "127.0.0.1"` and NO validator; `fn bind_first_free` in src/web/mod.rs binds any parsed SocketAddr with no host policy. Exhaustiveness check: `rg -ni 'auth.token|allowed.host|allowed_host|token_file|loopback' -g '*.rs' src` -> 0 hits; `rg -ni 'auth|bearer|authorization' -g '*.rs' src/web/` -> 1 hit and it is a false positive (`comment.author` at src/web/api.rs:101).

**Upstream evidence**

> internal/httpapi/server.go:451 `return nil, fmt.Errorf("--addr %q binds beyond loopback, which requires --allow-non-loopback (and, with it, --auth-token-file)", addr)`; internal/httpapi/server.go:477 `ValidateAuthPosture(cfg.AllowNonLoopback, cfg.Auth != nil, cfg.InsecureNoAuth)`; internal/httpapi/server.go:1487-1500 `checkHost` DNS-rebinding defense ("An unauthenticated service on loopback is reachable from any browser on the host"); internal/httpapi/auth.go:45 `TokenFileAuth` (SHA-256 digest set, hot-reload, constant-time compare); internal/httpapi/server.go:1605-1623 `authorize()` + `bearerCredential`; cmd/bd/serve.go:171 `--allow-non-loopback` help: "Requires --auth-token-file, since reaching the address would otherwise be the whole authorization"; cmd/bd/serve.go:176 `--allowed-host`.

</details>

<details><summary><b>Federation is a file-path-only stub: registered `https://` peers are always rejected at sync time, and there is no conflict strategy</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** large

**Description**

> LOCAL accepts `br federation add --remote-url https://beads.example.com` without complaint, then `br federation sync` hard-fails on that same URL. There is no network transport at all — no HTTP client dependency exists in LOCAL's Cargo.toml. GO's federation is a real multi-repo sync against remote Dolt databases with peer selection, conflict strategy, credential prompting, and a `status` subcommand reporting ahead/behind and unresolved conflicts. LOCAL also has NO `status` subcommand at all.

**Impact**

> A user follows LOCAL's own `--remote-url` help text, registers an https peer successfully, and only discovers at `br federation sync` that no network transport exists. The whole multi-repo story reduces to copying a JSONL file to a local path, with no conflict detection, no ahead/behind reporting, and no status command. LOCAL's docs also mis-classify this as a br-exclusive feature: docs/BD_VS_BR.md:75 and :87 both list Federation as `bd`-absent, which is wrong — GO ships a 14 KB cmd/bd/federation.go.

**Local evidence**

> src/cli/commands/federation.rs:379 `"unsupported protocol in remote_url: '{}'; use file:// or a bare path"`; src/cli/commands/federation.rs:32 the enum variant is literally documented `/// Sync with a peer (stub).`; src/cli/commands/federation.rs:22-37 `FederationCommand` has only Add/List/Remove/Sync/Info (no Status); `rg -ni 'strategy|conflict' src/cli/commands/federation.rs` -> 0 hits; `rg -n 'reqwest|ureq|curl|isahc' Cargo.toml` -> 0 hits (no HTTP client dependency at all); the `remote_url` doc at federation.rs:43 promises "Remote URL (e.g., https://beads.example.com)" that the code then refuses.

**Upstream evidence**

> cmd/bd/federation.go:25-57 `federationCmd` "Manage peer-to-peer federation between other workspaces ... Requires the Dolt storage backend"; cmd/bd/federation.go:36-56 `federationSyncCmd` with `--peer` and `--strategy ours|theirs`; cmd/bd/federation.go:58-70 `federationStatusCmd` "Show federation sync status ... Commits ahead/behind each peer ... Whether there are unresolved conflicts"; cmd/bd/federation.go:129-138 flags `--peer`, `--strategy`, `-u/--user`, `-p/--password` ("prompted if --user set without --password"), `--sovereignty` T1-T4.

</details>

<details><summary><b>No external issue-tracker integrations at all (GitHub, GitLab, Jira, Linear, Notion, Azure DevOps)</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** large

**Description**

> GO has a pluggable tracker subsystem with six first-party adapters, a plugin-style `IssueTracker` interface, a 53 KB bidirectional sync engine (pull/push/conflict-detect/conflict-resolve/dependency materialization), per-tracker FieldMappers, and first-class CLI commands. LOCAL has only the inert `external_ref: Option<String>` model field. DWS is identical to LOCAL here (its `jira` hits are all test fixtures / a close_policy string literal), so this is a go-only gap.

**Impact**

> LOCAL cannot round-trip an issue to a real tracker, cannot detect a tracker-side edit, and cannot materialize cross-tracker dependencies. Severity is set to high (not info) because LOCAL's own rationale is factually obsolete: docs/BD_VS_BR.md:9 justifies the omission with "Agents use MCP / APIs directly; `bd` shell-outs are brittle" — but GO no longer shells out; it uses native in-process HTTP clients (internal/linear/client.go 41 KB with OAuth at oauth.go, internal/gitlab/client.go 22 KB) behind a typed interface. The stated reason no longer describes what GO actually does.

**Local evidence**

> `rg -ni 'gitlab' -g '*.rs' src` -> 0 hits. No adapter module exists: `rg -ni 'jira|linear|notion' -g '*.rs' src` yields only doc/test strings (src/util/id.rs:1360 `ext:github#repo-456`, src/close_policy.rs:2381 `"Captured in tracker:ABC-123"`). No CLI subcommand: full Commands enum dump (src/cli/mod.rs:753-1098) contains no Track/SyncJira/SyncLinear/SyncAdo/SyncGitHub/SyncNotion. The only integration point is the DB column `pub external_ref: Option<String>` at src/model/mod.rs:936 — never read or written by any network code.

**Upstream evidence**

> internal/tracker/tracker.go:11-58 `IssueTracker` interface (FetchIssues/CreateIssue/UpdateIssue/FieldMapper/IsExternalRef/BuildExternalRef) + `BatchPushTracker`/`BatchPushDryRunner`/`PullStatsProvider` optional capabilities; internal/tracker/registry.go:19-43 `Register/Get/List/NewTracker`; internal/tracker/engine.go:149 `Sync`, :259 `DetectConflicts`, :327 `doPull`, :935 `doPush`, :1265 `resolveConflicts`; internal/tracker/types.go:75-77 `Pull bool / Push bool`; adapters: internal/github/tracker.go, internal/gitlab/tracker.go, internal/jira/tracker.go, internal/linear/tracker.go (50 KB cmd), internal/notion/tracker.go, internal/ado/tracker.go. CLI: cmd/bd/jira.go:17/45/70 (`jira`, `sync`, `status`), cmd/bd/linear.go:28/96/162/176 (`linear`, `sync`, `status`, `teams`), cmd/bd/github.go:30/44/60/70 (`github`, `sync`, `status`, `repos`).

</details>

<details><summary><b>`br federation add --password` takes the secret as a plaintext argv value with no prompt fallback</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> LOCAL accepts a federation peer password only as a command-line argument and immediately encrypts it at rest. There is no interactive prompt and no env-var or file source, so the secret is exposed in shell history, in `/proc/<pid>/cmdline` for the process lifetime, and in any process listing. GO's equivalent flag is documented as prompting when the password is not supplied, keeping the secret off argv.

**Impact**

> A user who sets `--user` without `--password` on GO gets a secure prompt; on LOCAL there is no such path, so the only way to authenticate is to put the secret in argv where every local user and every process lister can read it. Note the at-rest handling is fine (AES-256-GCM, src/util/credentials.rs:24) — the exposure is argv-only. Severity is medium rather than high because the federation transport is itself currently unreachable over a network (see the federation finding).

**Local evidence**

> src/cli/commands/federation.rs:41-42 `#[arg(long)] pub password: Option<String>` (no prompt machinery); `cmd_add` reads `args.password` directly and passes it to `CredentialKey::encrypt_password`; `rg -ni 'rpassword|read_password|prompt' src/cli/commands/federation.rs` -> 0 hits; `rg -ni 'rpassword|read_password' -g '*.rs' src` returns no password-prompt helper for federation.

**Upstream evidence**

> cmd/bd/federation.go:137 `federationAddPeerCmd.Flags().StringVarP(&federationPassword, "password", "p", "", "SQL password (prompted if --user set without --password)")`.

</details>

<details><summary><b>No SSE / event-push surface for live remote consumers</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> GO provides a push sibling to its event poll: a `text/event-stream` endpoint with `id:` sequence framing, browser-native `Last-Event-ID` resume, a hard cap on concurrent streams, and a same-read-path guarantee (it calls the identical paged journal read the poller uses, so retention semantics cannot drift). LOCAL has no streaming surface: its web UI's activity endpoint is a hardcoded empty stub, and its MCP server has no subscription primitive.

**Impact**

> Any dashboard, bot, or editor plugin that wants live issue state must poll. A poller holds nothing between requests but cannot react faster than its interval; GO's stream delivers within the push latency. LOCAL's web UI activity view therefore shows nothing at all.

**Local evidence**

> src/web/mod.rs:125-127 routes `/api/p/{project_id}/activity` to `axum::routing::get(api::stub_empty_activity)`; src/web/api.rs:820 defines `stub_empty_activity`. `rg -ni 'event-stream|ServerSent|sse|Last-Event-ID' -g '*.rs' src` -> 0 hits. LOCAL's event log is append-only and readable only by polling (src/storage/events.rs).

**Upstream evidence**

> internal/httpapi/routes.go:711 `pattern: "/v0/beads/events:watch"`; internal/httpapi/routes.go:151 notes the row is "coupled to bypassSemaphore"; internal/httpapi/events_watch.go:18-28 "The events journal, PUSHED ... held open as text/event-stream ... `Last-Event-ID` ... is the same value the `since` parameter takes, so a dropped stream resumes exactly where a poller would have"; events_watch.go:131 `const lastEventIDHeader = "Last-Event-ID"`; events_watch.go:133 `func handleWatchEvents`; events_watch.go:37-40 `eventsWatchBatch` + documented hard cap on concurrently held streams.

</details>

<details><summary><b>`br web` REST API is roughly 40 percent hardcoded stubs returning empty JSON</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** large

**Description**

> LOCAL's web server registers 18 route groups, but 10 of them are wired to stub handlers that return a constant `{}` or `[]` regardless of request. Affected user-visible surfaces: gate, assist, human-assist, insights, activity, gamification, attachments (upload + path PUT), publish, board order, filesystem browse, and self-update check/run. A client cannot distinguish "not implemented" from "empty result" — a stub POST returns HTTP 201 CREATED, so callers treat a no-op as a success. GO implements every one of these as real handlers.

**Impact**

> The web UI silently no-ops on gate decisions, attachments, publish, and update runs while returning success codes, so a UI or script that acts on a 201 believes it changed something. There is also no 501/NotImplemented signal to let a client degrade correctly.

**Local evidence**

> src/web/api.rs:803 `stub_json`, :807 `stub_created` (returns `(StatusCode::CREATED, Json(json!({})))`), :811 `stub_assist`, :820 `stub_empty_activity`, :824 `stub_empty_orders`, :828 `stub_insights`, :843 `stub_gamification`, :853 `stub_fs`, :862 `stub_update_check` — 9 `fn stub_*` handlers, none reading request state. Routed at src/web/mod.rs:109,113,117,122,126,130,135,139,144,149,155,166,170,172.

**Upstream evidence**

> internal/httpapi/problem.go (80 KB), internal/httpapi/tree.go, internal/httpapi/sweep.go, internal/httpapi/settings.go + settings_write_test.go, internal/httpapi/release.go + release_test.go, internal/httpapi/stats.go, internal/httpapi/blocking.go, internal/httpapi/query.go, internal/httpapi/reads.go, internal/httpapi/edges.go, internal/httpapi/related.go, internal/httpapi/pinning.go, internal/httpapi/metadata_cas.go — one real handler per surface, each with a matching `*_test.go`.

</details>

<details><summary><b>No credential resolution ladder — LOCAL can only read a local key file, GO can pull credentials from env, files, or an external helper process</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> GO has a backend- and issuer-neutral credential resolver that walks an ordered chain of sources (env var, file, external command) and fails closed on a configured-but-broken source. The built-in command rung implements the vendor-neutral `credential_process` idiom shared by kubectl ExecCredential, AWS, and the git credential helper, so a protected remote DB can be opened with a Vault-issued or workload-identity credential. LOCAL has only local AES-256-GCM encryption of credentials already in hand, plus an INI parser. (LOCAL's at-rest encryption is arguably stronger; the missing piece is OBTAINING the credential from an external authority.)

**Impact**

> LOCAL cannot authenticate to a shared or protected remote store using a workload identity, a short-lived token, or a vault helper — only a literal secret the user already possesses. Short-lived/rotating credential workflows, which GO supports by construction, are not expressible.

**Local evidence**

> src/util/credentials.rs:1-30 documents its entire scope: "AES-256-GCM encryption for federation peer credentials, with a 32-byte random key stored at `<beads_dir>/.beads-credential-key`" + an "INI-style credentials file parser (as used by Go beads) for reading `~/.config/beads/credentials`". `rg -ni 'credential_process|ExecCredential|credential.helper|ResolveLadder' -g '*.rs' src` -> 0 hits. There is no TTL/expiry concept for a credential anywhere.

**Upstream evidence**

> internal/creds/creds.go:1-10 package doc ("backend- and issuer-neutral ... a vendor-neutral credential-process idiom (kubectl ExecCredential / AWS credential_process / git credential helper)"); creds.go:19-35 `Kind` (KindSecret/KindIdentity) and `Credential{Value, Username, Kind, Expiry, Source}`; creds.go:41-51 `Source` interface; creds.go:57-70 `ResolveLadder` ("fails closed: any source error stops the walk ... a configured-but-broken helper can never silently downgrade to a lower rung"); internal/creds/command.go (6.4 KB) the command source.

</details>

### C — `mcp-agent-surface` (12 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | low | dws-only | `divergent_behavior` | MCP tool errors are raised as JSON-RPC errors instead of isError tool results with structured content |
| — | low | both-upstreams | `missing_flag` | MCP cannot write acceptance_criteria, a first-class field LOCAL already stores, gates on, and exposes via CLI |
| — | medium | both-upstreams | `missing_test` | No MCP protocol-level integration tests; the tool/resource/prompt surface and error contract are unverified end-to-end |
| — | medium | dws-only | `missing_flag` | close_issue MCP schema cannot set agent_name/harness/model, so a shared server cannot attribute closes per agent |
| — | medium | dws-only | `missing_feature` | No if_unchanged optimistic-concurrency precondition on update_issue (lost-update protection) |
| — | low | dws-only | `architecture_gap` | br serve has no graceful shutdown: SIGINT/SIGTERM cannot interrupt it, risking WAL loss |
| — | low | dws-only | `architecture_gap` | MCP auto-flush ignores the user's configured history/snapshot policy, unlike the CLI sync path |
| — | low | go-only | `partial` | Ready and blocked views are unfilterable and parameterless; no MCP tool can answer a scoped ready-work query |
| — | low | go-only | `missing_feature` | No response-side token-efficiency controls: no result compaction, field selection, or description truncation |
| — | low | dws-only | `divergent_behavior` | Resource URI space diverges: beads://issues/{id} (LOCAL) vs beads://issue/{id} (DWS) |
| — | low | go-only | `missing_feature` | No runtime workspace switching: br serve binds one .beads directory permanently, with no equivalent of GO's context tool |
| — | low | go-only | `missing_feature` | No MCP tool-surface introspection and no admin/diagnostics surface (discover_tools, get_tool_info, admin) |

<details><summary><b>MCP tool errors are raised as JSON-RPC errors instead of isError tool results with structured content</b></summary>

- **Severity:** critical · **Present in:** dws-only · **Effort:** large

**Description**

> All 7 LOCAL tools return `McpResult<Vec<Content>>`, so every domain error (issue-not-found, validation, placeholder-ID, policy refusal) propagates as a *raised* JSON-RPC error. DWS routes all 7 through a `final_tool_result()` wrapper that converts the error into a successful tool result carrying `is_error: true` plus `structured_content: {code, kind, message, data}`. DWS's own code comment names the reason: tool refusals belong in the tool result where clients can inspect the typed cause, because 'JSON-RPC errors are reserved for protocol failures; the framework deliberately sanitizes internal protocol errors.' In LOCAL the well-formed payload IS built (`McpError::with_data(mcp_code, structured.message, data)` at src/mcp/tools.rs:357) but is delivered through the channel the framework sanitizes, so the structured fields do not survive to the client. GO has the same weakness as LOCAL (its admin path does `raise ValueError`, server.py:1418), so this is a DWS-only gap.

**Impact**

> An MCP client talking to LOCAL cannot machine-read an error code. Distinguishing 'issue not found' from a malformed-argument bug from a close-policy refusal requires substring matching on prose. Every tool invocation that legitimately fails (a very common case in agent loops) returns a protocol-level error, which many clients surface as a transport fault and some retry blindly. LOCAL's `suggested_tool_calls` recovery hints (tools.rs:80, :101) are built into the error and are lost the same way.

**Local evidence**

> `grep -c 'McpResult<Vec<Content>>' src/mcp/tools.rs` -> 7 (every `fn call` at lines 1285, 1518, 1849, 2168, 2404, 2778, 2966). `grep -n 'FinalCallToolResult\|ResultMeta\|CompleteResult\|ContentBlock\|structured_content\|is_error' src/mcp/*.rs` -> 0 hits across mod.rs/tools.rs/resources.rs/prompts.rs. LOCAL never emits `isError`. LOCAL does build rich payloads (src/mcp/tools.rs:357 `McpError::with_data(mcp_code, structured.message, data)`) but returns them as `Err(...)`. No MCP protocol test asserts the wire error contract: `find tests/ -iname '*mcp*'` -> only tests/artifacts/perf/ directories; `grep -rln fastmcp tests/` -> 0 hits.

**Upstream evidence**

> DWS src/mcp/tools.rs:1470-1498 `final_tool_result(result: McpResult<Vec<Content>>) -> McpResult<CompleteResult<FinalCallToolResult>>`, with the doc comment at :1467-1469; error branch sets `is_error: true, structured_content: Some(error)` at :1490-1494; success branch `is_error: false, structured_content: None` at :1473-1486; returns `Ok(CompleteResult::new(payload, ResultMeta::empty()))` at :1497. Payload shape from `mcp_error_json` at :1453-1460 -> `{code: i32::from(err.code), kind: format!("{:?}", err.code), message, data}`. All 7 `fn call` signatures return `McpResult<CompleteResult<FinalCallToolResult>>` (:1219, :1547, :1884, :2350, :2753, :3186, :3442). Wire-level proof: DWS tests/e2e_mcp_protocol.rs:319-322 asserts `response["result"]["isError"] == true` and reads `response["result"]["structuredContent"]`; :342-346 asserts `result.get("isError")` and `result.get("structuredContent")`.

</details>

<details><summary><b>MCP cannot write acceptance_criteria, a first-class field LOCAL already stores, gates on, and exposes via CLI</b></summary>

- **Severity:** high · **Present in:** both-upstreams · **Effort:** small

**Description**

> LOCAL's data model, storage layer, and CLI all carry `acceptance_criteria`, but the MCP `create_issue` and `update_issue` input schemas omit it, so an MCP agent has no way to author the field. This is a broken local workflow, not merely a missing feature: LOCAL ships a close policy gate `require_acceptance_criteria_satisfied` that rejects closes while the issue body has unchecked `- [ ]` items, so an agent that cannot set the field can still be blocked by the gate. GO also exposes it (as `acceptance` on create, `acceptance_criteria` on update), so this is a both-upstreams gap.

**Impact**

> MCP is the primary agent surface for LOCAL, so in practice `acceptance_criteria` is unreachable for MCP-driven work: the only way to set it is a human at a shell. Any agent using LOCAL over MCP cannot author acceptance criteria, and if the close policy gate is enabled it will hit close refusals it has no way to resolve. LOCAL is strictly worse here than a `br update --acceptance` one-liner on the same binary.

**Local evidence**

> `grep -c acceptance_criteria src/mcp/tools.rs` -> 0; also `grep -c prerequisites src/mcp/tools.rs` -> 0, `check_acceptance`/`uncheck_acceptance`/`add_acceptance` -> 0 each. The field itself is present three layers down: model at src/model/mod.rs:874 `pub acceptance_criteria: Option<String>`; storage at src/storage/sqlite.rs:10961 `pub acceptance_criteria: Option<Option<String>>` on `IssueUpdate`; CLI at src/cli/mod.rs:1456-1458 `#[arg(long, visible_alias = "acceptance")] pub acceptance_criteria: Option<String>`. The gate that consumes it: src/close_policy.rs:89 `pub require_acceptance_criteria_satisfied: ToggleGate` and :1194-1206 `evaluate_acceptance_criteria`. LOCAL MCP create_issue properties are exactly {title, description, type, priority, assignee, labels, parent, issues}; update_issue exactly {id, title, description, status, priority, type, assignee, owner, due_at, defer_until, estimated_minutes, external_ref, labels_add, labels_remove, comment, updates}.

**Upstream evidence**

> DWS exposes it on both tools: update_issue schema at src/mcp/tools.rs:2357-2759 contains `acceptance_criteria`, `prerequisites`, `check_acceptance`, `uncheck_acceptance`, `add_acceptance`; create_issue schema at :1891-2357 contains `acceptance_criteria` and `prerequisites`; DWS's `UPDATE_FIELD_KEYS` const at :32-47 includes `acceptance_criteria` and `prerequisites`; DWS's `IssueUpdate` has both at src/storage/sqlite.rs:18437-18438. DWS asserts it end-to-end at tests/e2e_mcp_protocol.rs:492 `exercise_acceptance_over_mcp`. GO: integrations/beads-mcp/src/beads_mcp/server.py:1090 `acceptance: str | None = None` on `create`, tools.py:508/539 `acceptance_criteria` on `update`, models.py:182/202.

</details>

<details><summary><b>No MCP protocol-level integration tests; the tool/resource/prompt surface and error contract are unverified end-to-end</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> LOCAL's MCP surface is covered only by inline unit tests that call handler functions directly. Nothing spawns `br serve` and speaks JSON-RPC to it, so the discovery surface (`tools/list`, `resources/list`, `prompts/list`), the resource URI template, and the tool error contract are all untested. DWS has a 3069-line protocol harness that exercises exactly these, which is what makes its other MCP guarantees (isError, structuredContent, acceptance-over-MCP, shutdown) trustworthy rather than merely asserted in comments. LOCAL also has zero inline tests for prompts.rs.

**Impact**

> The exact things this audit found wrong in LOCAL are the things its test suite cannot catch: the plural-vs-singular resource URI is only observable over the wire, and the isError/structuredContent contract is only observable over the wire. With no protocol test, a future fastmcp bump that changes registration or error semantics (see the URI-overlap risk in coverage_notes) would ship silently. LOCAL also cannot regression-test its advertised 7 tools / 12 resources / 4 prompts against AGENTS.md and docs/CLI_REFERENCE.md, which are hand-maintained and already disagree with DWS.

**Local evidence**

> `find tests/ -iname '*mcp*'` -> only tests/artifacts/perf/beads-perf-* directories and one summary.md, no .rs test files. `grep -rln 'fastmcp\|mcp::' tests/` -> 0 hits. Inline-only coverage: `grep -c '#\[test\]'` -> src/mcp/tools.rs:52, src/mcp/resources.rs:10, src/mcp/mod.rs:7, src/mcp/prompts.rs:0 (total 69, all unit-level; zero for prompts). The tests at src/mcp/mod.rs:299-301 construct a `BeadsState` literal and call handler methods, never the transport.

**Upstream evidence**

> DWS tests/e2e_mcp_protocol.rs (3069 lines) spawns the real binary over stdio with a hand-rolled JSON-RPC client: `McpClient::spawn` at :184, `send` :229, `request_frame` :243, `receive_response` :252, `read_resource` :299, `call_tool` :336, `tool_error` :309. `assert_discovery_surface` at :419 asserts `tools/list` (:425), `resources/list` (:446) and `prompts/list` (:469); `mcp_fixed_resources_and_issue_template_are_all_reachable` at :650; plus tests/e2e_mcp_shutdown.rs (184 lines).

</details>

<details><summary><b>close_issue MCP schema cannot set agent_name/harness/model, so a shared server cannot attribute closes per agent</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** small

**Description**

> A single `br serve` process can only record one actor for every mutation: `--actor` is read once at startup and baked into `BeadsState`. DWS instead accepts per-call `agent_name`, `harness`, and `model` on `close_issue` (and on `update_issue`), so one shared MCP server can attribute each mutation to the specific agent, harness, and model that made it. LOCAL's own `CloseArgs` struct already carries all three fields and LOCAL's `show_issue_json` already reads them back — only the MCP write schema is missing them, so the read side advertises a field the write side cannot populate.

**Impact**

> The audit trail cannot distinguish which agent closed what. On a shared `br serve` (exactly the swarm topology AGENTS.md prescribes, where many agents connect to one server) every mutation is stamped with the single `--actor` value, so 'which agent/model closed this bead' is unanswerable from the event log. LOCAL also cannot write a `transition_comment` atomically with the close, so satisfying a configured transition-comment requirement needs a separate write-combining call that is not atomic with the close.

**Local evidence**

> `grep -n 'actor' src/mcp/tools.rs` -> all mutation attribution goes through `state.actor` (e.g. :1625, :1638, :1649, :1654, :1660). `ServeArgs` at src/mcp/mod.rs:686-690 has exactly one field, `#[arg(long, default_value = "mcp")] pub actor: String`. LOCAL close_issue input properties (src/mcp/tools.rs:2354-2404) are exactly {id, ids, reason} — no agent_name/harness/model. Yet LOCAL's `CloseArgs` at src/cli/commands/close.rs:38-43 already declares `agent_name` (':40), `harness` (':42'), `model` (':43'), and `show_issue_json` output keys include actor, agent_name, harness, model — so the read path exposes attribution the MCP write path cannot set.

**Upstream evidence**

> DWS src/mcp/tools.rs close_issue schema at :2780-2822 adds `transition_comment` (:2770), `agent_name` (:2820 'Agent attribution for the close event'), `harness` (:2821), `model` (:2822); call site :2823-2825 `agent_name: optional_str_arg(&args, "agent_name")?, harness: ..., model: ...` populating `CloseArgs`. DWS `CloseArgs` src/cli/commands/close.rs:34 additionally carries `transition_comment` ('New comment committed atomically with the close transition'), which LOCAL has nowhere (`grep -rn transition_comment src/` -> 0 hits). DWS update_issue schema :2357-2759 also carries agent_name/harness/model. GO has no agent attribution in its MCP layer (`grep -n 'agent_name\|harness' .../server.py` -> 0).

</details>

<details><summary><b>No if_unchanged optimistic-concurrency precondition on update_issue (lost-update protection)</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> `update_issue` offers no compare-and-set guard: the write is unconditional, so a second agent that read the issue earlier silently clobbers the first agent's edits. DWS adds an `if_unchanged` input that makes the write conditional on the `updated_at` the caller last read, and advertises it in the tool description as the answer to lost updates. The field is entirely absent from LOCAL, not merely unwired from MCP.

**Impact**

> beads_rust is explicitly built for multi-agent swarms, and AGENTS.md documents concurrent agents claiming and editing the same beads. Without a precondition, last-writer-wins is the only outcome: an agent that read an issue, spent a long tool-use turn reasoning, then wrote a partial update will erase every intervening edit by another agent, and the audit log will show two clean successes with no conflict signal. This is a data-loss path, not a usability wart.

**Local evidence**

> `grep -rn 'if_unchanged' src/ | wc -l` -> 0. Zero hits across the whole LOCAL source tree, so this is neither exposed over MCP nor available on the CLI. LOCAL's update_issue has no `force` and no precondition of any kind; its only extra safety affordance is the `updates` batch array.

**Upstream evidence**

> DWS src/mcp/tools.rs:2398 declares the `if_unchanged` schema property; :2368 advertises it in the tool description ('Lost updates: pass if_unchanged with the updated_at you read to make the write conditional on nobody having edited the issue since'); :973-975 wires it via `updates.expect_updated_at = ...parse_if_unchanged_surface(...)`; :2110 comment 'Enforce `if_unchanged` on the update paths that never call `update_issue`'; dedicated regression test `update_issue_schema_advertises_the_if_unchanged_precondition` at :5269. Also available on the CLI at src/cli/mod.rs:1341. E2E proof: DWS tests/e2e_mcp_protocol.rs:743 `exercise_if_unchanged_over_mcp`.

</details>

<details><summary><b>br serve has no graceful shutdown: SIGINT/SIGTERM cannot interrupt it, risking WAL loss</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> LOCAL's `run_serve` ends in `server.run_stdio()`, which blocks until transport EOF and returns `()`. There is no cancellation plumbing, no `ensure_not_shutting_down()` guard on tool entry, and no path for a signal to unwind the stack. DWS replaces the API with `run_transport_returning_with_cx`, spawns a watcher that translates `crate::shutdown::is_requested()` into `Cx.set_cancel_requested(true)`, and adds a per-call `ensure_not_shutting_down()` refusal. The motivation is explicit in the DWS comment: it lets `br serve` return through `main` and run every destructor, specifically 'WAL flush on drop, #270'. Root cause is the pinned framework version: LOCAL resolves fastmcp-rust 0.3.2, DWS pins =0.10.0, and the `modern` module plus the Cx-aware transport entry point only exist in the newer line.

**Impact**

> On LOCAL an operator cannot stop `br serve` with Ctrl-C and get a clean shutdown. SIGINT does not reach the serve loop, so the process is typically SIGKILLed or left waiting on stdin EOF, skipping destructors and risking an unflushed SQLite WAL plus a stale `.write.lock`. For a server holding a workspace-wide write lock and serving a swarm, that is a durability and availability problem on every restart.

**Local evidence**

> src/mcp/mod.rs:781 `server.run_stdio();` is the last statement of `run_serve` and returns `()`; the legacy builder is used at :732 `fastmcp_rust::Server::new("br", ...)`. `grep -n 'ensure_not_shutting_down\|shutdown::is_requested\|set_cancel_requested\|run_transport_returning_with_cx\|Cx' src/mcp/mod.rs` -> 0 hits: no shutdown guard, no cancellation wiring. Dependency floor is src/../Cargo.toml:111 `fastmcp-rust = { version = "0.3.1", optional = true }`, resolved to 0.3.2 in Cargo.lock. `find tests/ -iname '*mcp*'` returns no test file, so no coverage of signal handling. CHANGELOG.md:247 confirms 'Updated fastmcp-rust and its FastMCP crate family to 0.3.1'.

**Upstream evidence**

> DWS src/mcp/mod.rs:1638 `fastmcp_rust::modern::ServerBuilder::new(...)`; :1694-1700 builds a Cx, spawns a watcher thread polling `crate::shutdown::is_requested()` and calling `watcher_cx.set_cancel_requested(true)`; :1702-1704 `server.run_transport_returning_with_cx(&serve_cx, StdioTransport::stdio())` returning a Result so the error maps to `BeadsError::Config`. Guard at :62-71 `ensure_not_shutting_down_with(...)` / `pub(super) fn ensure_not_shutting_down()`, called at the top of every tool `call` (e.g. src/mcp/tools.rs:2821, :3442, :3481). Comment at :1689-1693 states the WAL-flush-on-drop rationale (#270). Tests: DWS tests/e2e_mcp_shutdown.rs:123 `serve_sigint_returns_through_main_and_preserves_reopenable_db` sends SIGINT, asserts the process exits, then reopens the DB and runs `br sync status`; DWS Cargo.toml:130 pins `fastmcp-rust = { version = "=0.10.0" }`.

</details>

<details><summary><b>MCP auto-flush ignores the user's configured history/snapshot policy, unlike the CLI sync path</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** small

**Description**

> Every MCP mutation in LOCAL triggers a JSONL auto-flush, and that flush is called with no history configuration, so it always uses defaults. DWS resolves the user's `.br_history` policy once at server start, stores it on `BeadsState`, and passes it to every auto-flush. The gap is specifically in the MCP path: LOCAL does have the config resolver and does use it for CLI sync, so the MCP server is the one surface that ignores a policy the user explicitly set.

**Impact**

> A user who disabled snapshot history gets snapshots anyway once any agent mutates an issue through MCP, silently consuming disk and retaining data they asked not to retain. A user who tuned the retention policy has that policy ignored on the MCP path while the CLI honors it, so the same workspace behaves differently depending on who triggers the flush.

**Local evidence**

> src/mcp/mod.rs:256-265 `pub struct BeadsState` has exactly seven fields (db_path, beads_dir, jsonl_path, write_lock_timeout_ms, allow_external_jsonl, actor, issue_prefix) plus a private read_snapshot_cache — no history field. src/mcp/mod.rs:430-437 `flush_dirty_storage` calls `crate::sync::auto_flush(storage, &self.beads_dir, &self.jsonl_path, self.allow_external_jsonl)` with four arguments. The callee src/sync/mod.rs:3612-3617 `pub fn auto_flush(storage, beads_dir, jsonl_path, allow_external_jsonl)` accepts no history parameter at all. `grep -n 'history\|HistoryConfig' src/mcp/mod.rs` -> 0 hits. Meanwhile the resolver does exist locally — src/config/mod.rs:2774 `pub fn resolved_history_config(&self)` — and the CLI honors it at src/cli/commands/sync.rs:572 `history_config: open_result.resolved_history_config()`. `run_serve` (src/mcp/mod.rs:698-731) never calls it.

**Upstream evidence**

> DWS src/mcp/mod.rs:370-373 `pub history: crate::sync::history::HistoryConfig` on BeadsState, documented '.br_history policy resolved once at server start from the merged' config; resolved at :1603-1608 via `crate::config::resolved_history_config`; stored into the state at :1634; and passed to every auto-flush at :664 `self.history.clone()` as the fifth argument to `crate::sync::auto_flush`. Regression test at :1078-1146 `with_mutation_auto_flush_honors_resolved_history_config` (cited as GitHub #484 at :1078) asserts that with history disabled no `.br_history` directory is created and with it enabled exactly one snapshot is taken. Resolver at DWS src/config/mod.rs:5079 `pub fn resolved_history_config(&self)`.

</details>

<details><summary><b>Ready and blocked views are unfilterable and parameterless; no MCP tool can answer a scoped ready-work query</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> LOCAL exposes ready and blocked work only as two fixed `beads://` resources whose read handlers take no URI parameters — no filters, no limit, no field selection — and `list_issues` has no readiness semantics at all (its filter set is status/type/priority/assignee/labels/title/search, with no unblocked, no ready, and no unassigned predicate). So an agent cannot ask the single most common triage question — 'ready, unassigned, P0, label backend' — in one call; it must fetch the whole ready set, fetch the whole blocked set, and filter client-side, paying full context for two unfiltered reads. GO answers that in one call. DWS has the same limitation as LOCAL, so this is a GO-only gap.

**Impact**

> The core agent workflow of the product — pick ready work and claim it — is the workflow LOCAL's MCP surface supports worst. An agent must pull the entire unfiltered ready and blocked sets into context and do the selection itself, which both wastes tokens and pushes filtering logic (including OR-label and unassigned logic) into the model where it is done unreliably. It also cannot cap the read, since the resources accept no limit.

**Local evidence**

> src/mcp/resources.rs:457-478 `ReadyIssuesResource`: it is a plain `Resource` (not a `ResourceTemplate`) with `uri: "beads://issues/ready"` and a `read(&self, _ctx)` handler that takes no URI or query parameters and returns whatever `mcp_ready_issues(&self.0, &storage)` yields; its own description says 'Use list_issues for filtered queries' — but list_issues has no readiness filter to fall back on. `BlockedIssuesResource` is the same shape at :510. `list_issues` full property set, extracted from src/mcp/tools.rs:1190-1470, is {status, type, priority, assignee, labels, title, search, include_closed, limit, sort, queries} — verified identical to DWS's set at src/mcp/tools.rs:1225-1552, confirming neither Rust server grew a ready/unassigned predicate. LOCAL's CLI does have such filters, which sharpens the gap: the MCP surface is the constrained one.

**Upstream evidence**

> GO integrations/beads-mcp/src/beads_mcp/server.py:866-900 `@mcp.tool(name="ready", ...)` `ready_work(limit=10, priority, issue_type, assignee, labels, labels_any, unassigned=False, sort_policy, workspace_root, brief=False, fields=None, max_description_length=None)` — a filterable, limitable, field-selectable ready query in one call, described as 'Returns minimal format for context efficiency'. The blocked equivalent is `beads_blocked` (tools.py:685) surfaced as the `blocked` tool at server.py:1338-1339, and `list` at server.py:949 additionally branches on `status='blocked'` to return BlockedIssue records with blocked_by detail. `labels_any` (OR semantics) and `sort_policy` have no LOCAL equivalent in any MCP path.

</details>

<details><summary><b>No response-side token-efficiency controls: no result compaction, field selection, or description truncation</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> LOCAL returns full issue summaries for every result and lets the caller set `limit` up to 500, with no server-side shaping at all: no field projection, no compact representation, and no automatic truncation of large result sets. GO treats context budget as a first-class concern — it projects issues to a minimal shape, honors a caller-selected field subset, truncates long descriptions, and automatically collapses oversized result sets into a preview envelope with a recovery hint. Neither LOCAL nor DWS implements any of this, so an agent that asks for 500 issues pays for 500 fully-detailed issues of context.

**Impact**

> This is the single largest recurring context cost in agent use of LOCAL. A routine `list_issues(limit=200)` or an unfiltered `list_issues()` on a mature repo returns 200-500 complete issue records into the model's context, and there is no parameter the agent can pass to ask for less. Combined with the absence of any compaction, a swarm running several agents against one server pays this per call, per turn. The `limit` cap of 500 is a ceiling, not a mitigation.

**Local evidence**

> `grep -rniE 'compact|max_tokens|token_budget|truncat' src/mcp/` -> 0 hits across mod.rs/tools.rs/resources.rs/prompts.rs. `grep -n 'response_format|"format"|brief|minimal|compact' src/mcp/tools.rs` -> 0 hits. `list_issues` has no `fields`, `brief`, or `response_format` property; its full property set is {status, type, priority, assignee, labels, title, search, include_closed, limit, sort, queries}, and `limit` is documented 'Max issues to return (default 50, max 500)' at src/mcp/tools.rs:1240. The list response envelope carries only {count, issues} (plus {index, query, ok, error} for batches) — no compacted/total_count/preview/hint. The only defense is a `McpReadSnapshotCache` (src/mcp/mod.rs:268-278) which caches reads, it does not shrink them.

**Upstream evidence**

> GO integrations/beads-mcp/src/beads_mcp/server.py:6 docstring 'Result compaction for large queries (>20 issues)'; :88-116 loads COMPACTION_THRESHOLD (BEADS_MCP_COMPACTION_THRESHOLD, default 20) and PREVIEW_COUNT (BEADS_MCP_PREVIEW_COUNT) with validation; :930 `minimal_issues = [_to_minimal(issue) for issue in issues]`; :925-928 field selection via `_filter_fields` when a `fields` argument is given; :933-947 returns `CompactedResult(compacted=True, total_count, preview, preview_count, hint='Use show(issue_id) for full details.')` once over threshold; :857-858 `_truncate_description`; same pattern repeated for the blocked path at :1003-1015. The `ready` tool (server.py:866-900) carries `brief` ('return only {id, title, status} (~97% smaller)'), `fields`, and `max_description_length` parameters. DWS also has none of it: `grep -rniE 'compact|max_tokens|token_budget|truncat' src/mcp/` -> 0 hits.

</details>

<details><summary><b>Resource URI space diverges: beads://issues/{id} (LOCAL) vs beads://issue/{id} (DWS)</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** small

**Description**

> The individual-issue resource template is plural in LOCAL and singular in DWS, and both projects document their own spelling in AGENTS.md and CLI_REFERENCE.md. A client configured against either spelling fails against the other, so the two Rust servers are not drop-in swappable and any shared MCP client config, doc, or agent prompt is pinned to one. DWS did not rename arbitrarily: a code comment immediately above its IssueResource registration explains that 'fastmcp rejects overlapping exact/template registrations' and that individual issues therefore use the singular namespace while collections keep `issues/`. LOCAL keeps the plural form and registers IssueResource second, before the five `beads://issues/*` collection resources.

**Impact**

> A beads:// URI written against DWS's AGENTS.md returns not-found on LOCAL and vice versa, so an agent that learned one server's URI space breaks on the other with no capability signal. LOCAL's choice also places a `{id}` template in the same prefix as five exact collection URIs, which is the configuration DWS explicitly restructured to avoid; see coverage_notes for the unverified runtime-shadowing risk this creates.

**Local evidence**

> src/mcp/resources.rs:239 `uri: "beads://issues/{id}"`, :255 `uri_template: "beads://issues/{id}"`, :267 description 'Provide an issue ID via the URI template: beads://issues/{id}'. Documented as plural in two places: AGENTS.md:473 and docs/CLI_REFERENCE.md:1356. Registration order in src/mcp/mod.rs:757 `.resource(resources::IssueResource::new(state.clone()))` — second, ahead of ReadyIssues/BlockedIssues/InProgress/Deferred/Bottlenecks.

**Upstream evidence**

> DWS src/mcp/resources.rs:241 `uri: "beads://issue/{id}"`, :257 `uri_template: "beads://issue/{id}"`. Documented singular at DWS AGENTS.md:555. Rationale in DWS src/mcp/mod.rs:1679-1680: 'fastmcp rejects overlapping exact/template registrations. Individual issues use the singular namespace; collections retain issues/.' — and DWS correspondingly moves `IssueResource` to last (:1682), after all five `beads://issues/*` collections, with the comment '// Resources (12)' still at :1670 counting it separately.

</details>

<details><summary><b>No runtime workspace switching: br serve binds one .beads directory permanently, with no equivalent of GO's context tool</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** large

**Description**

> LOCAL's MCP server resolves its workspace once, before the transport starts, and then `BeadsState` is immutable — no tool can retarget the server at a different project. GO exposes a `context` tool that sets, shows, or initializes the workspace root, and the choice persists across subsequent tool calls, so one MCP server can be driven across multiple projects by the agent itself. The practical consequence is that an agent working across a monorepo of many beads workspaces must launch a separate `br serve` process per project, whereas GO can retarget in-process.

**Impact**

> An agent that must triage or update beads in several workspaces cannot use one MCP connection; it needs one `br serve` per project and cannot multiplex across them in a single conversation. For the cross-repo sync scenarios beads is designed around (content-addressed dedup across repos, `external:<project>:<capability>` dependency refs — which LOCAL does support at src/mcp/tools.rs:2726), this makes cross-workspace work awkward exactly where the data model says it matters.

**Local evidence**

> src/mcp/mod.rs:699 `let beads_dir = config::discover_beads_dir_with_cli(overrides)?;` is evaluated once at the top of `run_serve` before any transport exists; the resolved `beads_dir`/`db_path`/`jsonl_path` are moved into the `Arc<BeadsState>` at :724-732, and `BeadsState` (:256-265) holds them as immutable public fields with no setter. `run_serve` is reachable only via the `serve` subcommand with a single `--actor` flag (src/mcp/mod.rs:686-690). `grep -n 'workspace\|retarget\|switch' src/mcp/*.rs` -> no workspace-switching affordance; docs/CLI_REFERENCE.md:1343-1346 lists exactly one option (`--actor`) and confirms the stdio, single-workspace model.

**Upstream evidence**

> GO integrations/beads-mcp/src/beads_mcp/server.py:628-675 `@mcp.tool(name="context", ...)` with actions set/show/init; :628-633 description 'Manage workspace context for beads operations. Actions: set / show / init'; :649-654 infers `set` when `workspace_root` is supplied else `show`; :665-675 `init` delegates to `beads_init(prefix=...)`; :677+ `_context_set` resolves the repo root and stores it in module-level `_workspace_context['BEADS_WORKING_DIR']` and `os.environ`, explicitly to persist 'across MCP tool calls'. Supporting machinery in tools.py:215 `_resolve_workspace_root`, :146 `_find_beads_db_in_tree`, plus a per-call `workspace_root` override parameter threaded through the other tools (e.g. `ready` at server.py:882). GO's design makes this possible because its MCP server is a subprocess client of `bd` (bd_client.py) rather than an in-process storage owner.

</details>

<details><summary><b>No MCP tool-surface introspection and no admin/diagnostics surface (discover_tools, get_tool_info, admin)</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** large

**Description**

> GO's MCP server exposes an operational tool group with no analogue in LOCAL: two meta tools that let an agent cheaply enumerate and inspect the server's own vocabulary, and an `admin` tool that exposes database health checks, orphan repair, migration inspection, and test-pollution detection. LOCAL has all of the underlying capability as CLI subcommands — `br doctor` alone is 768KB of source — but none of it is reachable over MCP, and the 7-tool ceiling leaves no room. DWS matches LOCAL here, so these three names are GO-only.

**Impact**

> Two costs. (1) Discovery: an agent must load seven full input schemas to learn the vocabulary, where GO spends ~500 bytes. (2) Operations: when an MCP-driven session hits a data problem — orphaned dependency references, duplicate or polluting test issues, a half-applied migration — the agent has no way to diagnose or repair it, because `br doctor` is CLI-only. The agent must abandon the MCP session, shell out, and lose the conversation context, and it may not be able to repair at all if it lacks shell access.

**Local evidence**

> `grep -rniE 'discover_tool|get_tool_info|list_tools|capabilit' src/mcp/` -> only three incidental hits, none an introspection facility: src/mcp/resources.rs:1202 and src/mcp/tools.rs:3091 are the string 'external:missing:capability' in dependency-error payloads, and src/mcp/tools.rs:2726 is a dependency-target description. No MCP tool lists the server's own tools, and no MCP tool runs diagnostics. The full LOCAL tool set is the fixed seven registered at src/mcp/mod.rs:753-759 and enumerated in AGENTS.md:471-472 and docs/CLI_REFERENCE.md:1354-1355; an agent that wants to know what LOCAL can do has no cheap way to find out beyond loading all seven full schemas.

**Upstream evidence**

> GO integrations/beads-mcp/src/beads_mcp/server.py:364-382 `@mcp.tool(name="discover_tools")` returning `{tools: _TOOL_CATALOG, count, hint}`, documented 'Context savings: ~500 bytes vs ~10-50k for full schemas'; :385+ `@mcp.tool(name="get_tool_info")` taking `tool_name` and returning full parameters and usage examples; :1373-1420 `@mcp.tool(name="admin")` dispatching validate / repair / schema / debug / migration / pollution, delegating to `beads_validate` (tools.py:758), `beads_repair_deps` (tools.py:724), `beads_inspect_migration` (tools.py:699), `beads_get_schema_info` (tools.py:714) and `beads_detect_pollution` (tools.py:741). The corresponding helpers all exist in tools.py.

</details>

### C — `model-lifecycle` (7 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | dws-only | `missing_feature` | No in-place acceptance-criteria checklist editing (`--check-acceptance` / `--uncheck-acceptance` / `--add-acceptance`) |
| — | medium | go-only | `missing_flag` | No conditional compare-and-swap update: `--if-assignee` / `--if-status` absent |
| — | low | dws-only | `missing_feature` | No per-issue `prerequisites` checklist field or `transition_prerequisites_incomplete` close gate |
| — | low | go-only | `missing_feature` | No first-class claim release command (`bd unclaim`) |
| — | low | both-upstreams | `missing_feature` | No optimistic-concurrency token on the issue row (`row_version` / `revision`) |
| — | low | go-only | `missing_feature` | No transactional multi-operation batch runner (`bd batch`) |
| — | info | go-only | `missing_feature` | No close-as-duplicate / close-as-superseded convenience commands (`bd duplicate --of`, `bd supersede --with`) |

<details><summary><b>No in-place acceptance-criteria checklist editing (`--check-acceptance` / `--uncheck-acceptance` / `--add-acceptance`)</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS ships a 746-line byte-preserving checklist engine over the `acceptance_criteria` markdown column plus three in-place CLI flags. Items are addressed by 1-based index or a unique case-insensitive text selector, every edit rewrites only the marker character (so the rest of the field, including trailing bytes and fenced code, is untouched), and the call returns a structured `AcceptanceCriteriaOutput` (items, checked_count, total, remaining, checked, unchecked, added) in both plain and JSON output. Because the edit cannot destroy content, it deliberately bypasses DWS's whole-field-replacement guard without weakening it. LOCAL stores `acceptance_criteria` as an opaque `Option<String>` and can only rewrite the entire field via `--acceptance-criteria`. Note the two sides are not equivalent: LOCAL *does* implement the read-side gate (`require_acceptance_criteria_satisfied`, unchecked-box scanning) — it is specifically the edit/authoring surface that is missing.

**Impact**

> LOCAL can enforce 'all acceptance boxes must be ticked' but an agent cannot satisfy that gate without re-sending the entire markdown field, which is exactly the fragile operation DWS engineered around. Every criteria update risks clobbering concurrent edits.

**Local evidence**

> `rg -n 'check_acceptance|uncheck_acceptance|add_acceptance|check-acceptance|uncheck-acceptance|add-acceptance' src/ tests/` -> 0 hits. `rg -n 'AcceptanceItem|AcceptanceChecklist|AcceptanceSelector|AcceptanceEdit|AcceptanceCriteriaOutput|plan_acceptance_edit' src/ tests/` -> 0 hits. `find src -type d -name 'acceptance*'` -> no such directory. LOCAL's 179 `acceptance_criteria` references are all whole-column storage/format/validation plumbing (src/close_policy.rs:89,989,1194-1230; src/model/mod.rs; src/storage/schema.rs; src/format/*) with no parser or editor.

**Upstream evidence**

> DWS src/model/acceptance.rs:1-13 (module contract: byte-preserving edits, fence handling), :24 `pub struct AcceptanceItem`, :44 `pub struct AcceptanceChecklist`, :131 `pub fn plan_acceptance_edit`, :182 `pub struct AcceptanceCriteriaOutput`; + src/model/acceptance/fence_tests.rs (188 lines). CLI: src/cli/mod.rs:1256 `pub check_acceptance: Vec<String>`, :1266 `pub uncheck_acceptance: Vec<String>`, :1276 `pub add_acceptance: Vec<String>`. Wiring: src/cli/commands/update.rs:15 (import), :178-196 `plan_acceptance_edits`, :839-859 (per-issue application, deliberately outside the overwrite guard).

</details>

<details><summary><b>No conditional compare-and-swap update: `--if-assignee` / `--if-status` absent</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** medium

**Description**

> GO's `bd update` accepts `--if-assignee` and `--if-status`: the write is applied only if the issue's current assignee/status still equals the supplied value, otherwise nothing is written and bd exits with a dedicated code 13 (distinct from the generic failure code 1), so a caller can distinguish 'guard failed' from 'the write errored'. The flags are mutually exclusive with `--claim` and with `--force`. LOCAL has no conditional-update surface whatsoever — its only concurrency guard is the claim-time `expect_unassigned` / `claim_exclusive` pair, which applies solely to taking a claim and does nothing for release, transfer, or arbitrary field updates.

**Impact**

> Lost-update / clobber hazard: a supervisor or peer agent performing a handover (`br update <id> --assignee new`) has no atomic way to assert it still holds the issue. Any check-then-write sequence races, and the failure is silent data loss rather than a typed refusal.

**Local evidence**

> `rg -n 'if_assignee|if-assignee|if_status|if-status' src/ tests/` -> 0 hits. The closest LOCAL mechanism is claim-scoped: src/storage/sqlite.rs:10997 `pub expect_unassigned: bool`, :10999 `pub claim_exclusive: bool`, consumed at src/storage/sqlite.rs:2735, 3108-3128. No conditional guard exists on the update path, and LOCAL `br update` has no way to say 'change X only while assignee is still Y'.

**Upstream evidence**

> GO cmd/bd/update.go:997-998 flag definitions — `updateCmd.Flags().String("if-assignee", "", "...a mismatch writes nothing and exits 13 (vs 1 for other failures). Requires a field update; cannot combine with --claim")` and the parallel `--if-status`; :1004 `updateCmd.MarkFlagsMutuallyExclusive("force", "if-assignee")`; runtime plumbing at :940-945. Behaviour pinned by cmd/bd/update_conditional_proxied_test.go:49-101 and cmd/bd/assign_fence_embedded_test.go:169-178.

</details>

<details><summary><b>No per-issue `prerequisites` checklist field or `transition_prerequisites_incomplete` close gate</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS models a second, distinct per-issue checklist — `prerequisites` — separate from both `acceptance_criteria` and dependency edges, described in the source as 'what must be true before this work may transition'. It is a first-class persisted column with its own migration, is settable on `br create --prerequisites` and `br update --prerequisites`, is covered by the same destructive-rewrite guard as description/design/notes, and drives a close-policy gate `transition_prerequisites_incomplete` that reuses the same AcceptanceChecklist parser. LOCAL has no such field: its 17 `prerequisite` hits are all about dependency-edge semantics (a parent epic's blocked state propagating to children, deferred dependents, etc.) and the word never names a column.

**Impact**

> LOCAL collapses 'what must be true before starting' into dependency edges, which cannot express a per-issue checklist that is neither a blocker edge nor a delivery criterion — the exact middle band DWS's transition gate targets.

**Local evidence**

> `rg -n 'prerequisite' src/` -> 17 hits, all dependency-graph prose or tests (src/storage/sqlite.rs:333,6001-6041,17970-18028; src/cli/commands/close.rs:192,832-835,1781-1802) — none is a field or column. `rg -n 'prerequisites' src/storage/schema.rs` -> 0 hits. `rg -n 'prerequisites TEXT' src/ tests/ docs/` -> 0 hits. No `prerequisites` key appears in the LOCAL Issue struct (src/model/mod.rs:853-1010).

**Upstream evidence**

> DWS src/model/mod.rs:488 `pub prerequisites: Option<String>,` (documented at :484-487 as 'Per-issue prerequisite checklist, separate from acceptance criteria and dependency edges. Policy decides which transitions require it.'). CLI: src/cli/mod.rs:1133 (create `--prerequisites`), :1240 (update `--prerequisites`, 'Replace the prerequisite checklist (empty string clears)'), :1344 (grouped into the destructive-replacement guard). Policy: src/close_policy.rs:1279 `fn evaluate_transition_prerequisites(` emitting gate `transition_prerequisites_incomplete` at :1302, parsing via `AcceptanceChecklist::parse` at :1286. Schema: src/storage/schema.rs:295 `prerequisites TEXT NOT NULL DEFAULT ''`, migration at :5021.

</details>

<details><summary><b>No first-class claim release command (`bd unclaim`)</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> GO ships `bd unclaim [id...]`, a purpose-built operation that clears the assignee and resets status to open in one step, takes a `--reason`, refuses to release another actor's live claim without `--force`, and supports `--if-assignee` for an atomic compare-and-swap release (mutually exclusive with --force, since they encode contradictory intent). LOCAL has no such command: releasing a claim requires a hand-assembled `br update <id> --status open --assignee ''` that must also satisfy LOCAL's close/dependency policy guards, with no dedicated audit reason and no holder check.

**Impact**

> Abandoning a claim in LOCAL is a multi-flag compound update that is easy to get wrong and cannot be made holder-safe. Combined with finding 1 (no lease), LOCAL has neither automatic nor safe-manual claim recovery.

**Local evidence**

> `rg -n 'unclaim' src/` -> 0 hits. `pub enum Commands` (src/cli/mod.rs:751-1093) has no Unclaim variant. LOCAL's release is only implicit: `pub assignee: Option<String>` on update (src/cli/mod.rs:1233) plus a status write, and the only mentions of releasing a claim are advisory strings in src/coordination.rs:651-660.

**Upstream evidence**

> GO cmd/bd/unclaim.go:13 `var unclaimCmd = &cobra.Command{` (Use "unclaim [id...]"); :28-30 documenting the `--if-assignee` atomic-CAS contract; :141-143 flag definitions — `unclaimCmd.Flags().StringP("reason", "r", ...)`, `.Bool("force", ...)`, `.String("if-assignee", "", "Only release if still assigned to this assignee (atomic compare-and-swap; exits nonzero without changing the issue when the holder differs)")`. Behaviour pinned by cmd/bd/unclaim_conditional_parity_test.go and cmd/bd/unclaim_test.go:58-64.

</details>

<details><summary><b>No optimistic-concurrency token on the issue row (`row_version` / `revision`)</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** large

**Description**

> GO stamps every issue with a random, non-zero `row_lock` cell that the engine rewrites on every status/ownership-mutating write, surfaced to Go callers as an equality-only `RowVersion` and to external/guarded clients as a decimal-string `revision` on the detail-view DTO. A client reads the issue, mutates it elsewhere, and can then detect that the row changed underneath it. GO is explicit that coverage is partial and that the complete change-detection key must combine RowVersion with `updated_at`, status, and the label set. LOCAL has no such token: its only change indicator is `updated_at`, so a stale read cannot be distinguished from an unchanged one, and `br` exposes no revision field on the wire.

**Impact**

> Any client doing read-modify-write over br's API (MCP, web, or a shell script) has no way to detect that the row moved between its read and its write, which is the primitive that would make finding 2 (conditional CAS) implementable.

**Local evidence**

> `rg -n 'row_version|RowVersion|optimistic_concurrency' src/ tests/` -> 0 hits. `rg -n 'revision' src/` -> 3 hits, all unrelated (src/policy.rs:500-501,701 `policy_revision`; src/cli/commands/reflect.rs:304 resolving a *git* revision). The LOCAL Issue struct (src/model/mod.rs:853-1010) has no revision/token field, and no `br schema` output advertises one.

**Upstream evidence**

> GO internal/types/types.go:99 `RowVersion int64 \`json:"-"\`` with the 20-line doc comment at :70-98 defining it as 'an opaque optimistic-concurrency token ... EQUALITY-ONLY ... a change signals the row was mutated since you read it', noting it is `json:"-"` because row_lock is random per write, and documenting that it must be combined with `updated_at`, status, and labels for complete detection. Projected to clients as `revision` via IssueDetails/RevisionToken.

</details>

<details><summary><b>No transactional multi-operation batch runner (`bd batch`)</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** large

**Description**

> GO's `bd batch` reads a line-oriented command grammar (`close`, `update`, `create`, `dep add`, `dep remove`) from stdin or `-f/--file` and executes the whole script inside one database transaction: on any error the entire batch rolls back and exits non-zero naming the failing line, otherwise it commits once. It exists specifically to collapse N `bd` process invocations (which each pay a full connection + commit) into one transaction and one commit. LOCAL has no `batch` subcommand. LOCAL *does* accept many IDs on a single command (`pub ids: Vec<String>` at src/cli/mod.rs:1442, 1579, 2964, 3016, 3252) and its MCP `update_issues` tool has batch semantics — but that is per-item partial-success (one write lock, per-item ok/error, 'Partial failures do not fail the whole batch', src/mcp/tools.rs:1251), which is the opposite failure model from GO's all-or-nothing rollback.

**Impact**

> Shell-orchestrated bulk maintenance in LOCAL costs N process spawns and N commits, and there is no way to get atomic multi-command behaviour. LOCAL's only batch semantics are best-effort partial success, so a caller cannot rely on 'either all of these land or none do'.

**Local evidence**

> `rg -n 'Batch|"batch"' src/cli/mod.rs` -> 0 hits; `ls src/cli/commands/batch.rs` -> No such file or directory. All 36 `\bBatch\b` hits in src/ are unrelated: DB write batching (src/storage/sqlite.rs:306,3474,9905), MCP batch tool descriptions (src/mcp/tools.rs:1200,1251,1480,1782), sync witness planning (src/sync/witness.rs:165), and test names. No cross-command, all-or-nothing write path exists.

**Upstream evidence**

> GO cmd/bd/batch.go:30 `var batchCmd = &cobra.Command{`; :36 `Short: "Run multiple write operations in a single database transaction"`; :40-41 "All operations execute inside a single dolt transaction: on any error the whole batch is rolled back, otherwise it is committed with one DOLT_COMMIT"; :62-63 "an unforced refusal rolls back EVERY operation in the batch"; :168-170 "One transaction, one commit message, whole-batch rollback on the first [error]". Grammar enumerated at :47-56. Paired with cmd/bd/batch_parity_test.go and cmd/bd/batch_test.go.

</details>

<details><summary><b>No close-as-duplicate / close-as-superseded convenience commands (`bd duplicate --of`, `bd supersede --with`)</b></summary>

- **Severity:** info · **Present in:** go-only · **Effort:** small

**Description**

> GO's `bd duplicate <id> --of <canonical>` and `bd supersede <id> --with <new>` are one-shot commands that create the `duplicates` / `supersedes` dependency edge and automatically close the source issue with a reference to the canonical/replacement. LOCAL has both edge types in its DependencyType enum (strictly richer than GO's in coverage — it also carries Attests, Tracks, Until, CausedBy, Validates, DelegatedFrom plus a Custom variant) and both reach the MCP dependency tool (src/mcp/tools.rs:570,588), but there is no CLI command that performs the edge-plus-close pairing. A LOCAL user must run `br dep add` and then `br close` as two separate steps. This is a documented deliberate exclusion, so it is reported at info severity per the audit rules.

**Impact**

> Impact is limited: the underlying graph edges and MCP path exist, so the capability is reachable — only the atomic convenience wrapper is missing. LOCAL's own porting scope document records this as a deliberate v1 exclusion, so it is not a regression.

**Local evidence**

> `rg -n 'as_duplicate|as-duplicate|as_superseded|as-superseded|duplicate_of' src/cli/mod.rs src/cli/commands/close.rs` -> 0 hits. LOCAL src/model/mod.rs:331-334 defines `Duplicates` and `Supersedes` DependencyType variants and :370,398,452 wire the string forms. The close error path only *suggests* the manual pairing: src/close_policy.rs:1298 "close-as-superseded with a duplicate_of edge before closing {issue_id}". No `Duplicate`/`Supersede` variant in `pub enum Commands` (src/cli/mod.rs:751-1093).

**Upstream evidence**

> GO cmd/bd/duplicate.go:16-25 `var duplicateCmd` (`Use: "duplicate <id> --of <canonical>"`, Short 'Mark an issue as a duplicate of another') with Long at :19-24 stating 'The duplicate issue is automatically closed with a reference to the canonical'; :28-35 `var supersedeCmd` (`Use: "supersede <id> --with <new>"`, same auto-close contract). Edge types: internal/types/types.go:1261 `DepDuplicates`, :1262 `DepSupersedes`.

</details>

### C — `observability-ops` (10 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | dws-only | `missing_feature` | br doctor --selftest: no end-to-end binary self-test on the host |
| — | low | go-only | `missing_feature` | No OpenTelemetry export: no storage-operation metrics, no per-command traces |
| — | low | go-only | `missing_feature` | No br events command: no queryable/tailable/exportable mutation journal |
| — | low | dws-only | `missing_feature` | br doctor --bundle: incident-evidence bundle not implemented (LOCAL's own doc tracks it) |
| — | medium | dws-only | `missing_feature` | br doctor migrate-schema: no receipt-bound, reversible schema migration lifecycle |
| — | low | dws-only | `missing_output_mode` | No engine/on-disk state block in br info --json or br doctor --json |
| — | info | go-only | `missing_feature` | No bd metrics command: no user-facing anonymous-usage metrics control plane |
| — | low | dws-only | `missing_feature` | Four DWS structured error codes are missing from LOCAL's taxonomy |
| — | low | dws-only | `missing_feature` | Four doctor detectors present in DWS are absent from LOCAL |
| — | low | go-only | `missing_flag` | No config-persisted per-check doctor suppression (doctor.suppress.*) |

<details><summary><b>br doctor --selftest: no end-to-end binary self-test on the host</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS adds `br doctor --selftest`, which spawns the currently-installed executable through a full issue lifecycle inside a throwaway temp workspace and emits a `br.doctor.selftest.v1` receipt: platform facts (os/arch/family/br_version/executable path), filesystem facts (case-sensitivity, renameat2/RENAME_NOREPLACE support classification as supported/unsupported/replaced/unknown), storage-engine facts, and a per-step receipt list. Its purpose is to catch host-level breakage that unit tests miss (Windows panics, WSL2 DrvFS renameat2 refusal, glibc floor) by exercising exactly the binary the user installed. LOCAL has no selftest flag, no selftest subsystem module, and no receipt schema. DWS's module doc names the specific shipped-binary bugs the check exists for (#438/#439 Windows, #419 WSL2 DrvFS, #444 glibc floor), so this is a proven-necessary capability, not speculative.

**Impact**

> A user or packager on a platform where br is broken has no supported way to prove the installed binary works end to end before filing an incident. Support has to reproduce the user's platform manually, and partial failures (e.g. SQLite silently falling back from renameat2 to a witness-checked path) are invisible until they corrupt data.

**Local evidence**

> rg -c 'selftest' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits; rg -c 'rename_noreplace' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits; rg -c 'br.doctor.selftest' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits. LOCAL's doctor_subsystems/mod.rs declares exactly 6 modules (capabilities_doctor, exit_codes, mutate, refuse_gates, run_dir, surface) — no selftest.rs. LOCAL's DoctorArgs (src/cli/mod.rs:3606-3704) has 10 flags; no --selftest/--selftest-dir/--keep.

**Upstream evidence**

> src/cli/commands/doctor_subsystems/selftest.rs:1-11 (module doc naming GH #438/#439 Windows panics, #419 WSL2 DrvFS renameat2, #444 glibc floor); selftest.rs:25 SCHEMA_VERSION = "br.doctor.selftest.v1"; selftest.rs:28-46 SelftestReceipt {schema_version, ok, elapsed_ms, workspace, kept, platform, fs, engine, steps}; selftest.rs:48-70 PlatformFacts + FilesystemFacts (case_sensitive, rename_noreplace: supported|unsupported|replaced|unknown); src/cli/mod.rs:3427-3446 (--selftest with conflicts_with_all = ["repair","repair_indexes","robot_triage","quick"], plus --selftest-dir and --keep).

</details>

<details><summary><b>No OpenTelemetry export: no storage-operation metrics, no per-command traces</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** large

**Description**

> GO ships a full opt-in OpenTelemetry integration (OTLP metrics + traces + logs, console exporters, VictoriaMetrics/Grafana reference stack) that instruments the storage layer with a counter, a duration histogram, an error counter, and an issue-count gauge, plus a root `bd.command.<name>` span per CLI invocation with scrubbed args. LOCAL has zero OpenTelemetry code: no exporter, no instrument, no `BR_OTEL_ENABLED`-style gate, no OTLP endpoint env vars. `br doctor --json` and `br sync --status --json` are LOCAL's only machine-readable telemetry, and both are point-in-time snapshots, not time-series metrics a Prometheus/VictoriaMetrics scraper can consume. DWS (the Rust upstream) also has no OTEL, so this is a GO-only capability that neither Rust port has taken.

**Impact**

> Operators running br in CI or a service fleet have no way to get per-command latency, error rate, or throughput into a metrics backend. The only quantitative surfaces are br doctor --json / br sync --status --json snapshots, which a scraper cannot trend. Every operational question ("is close getting slower?", "which command is failing?") has to be answered by hand from logs.

**Local evidence**

> rg -ci 'otel|OpenTelemetry|OTLP|tracer|BD_OTEL|victoriametrics|grafana' /Users/tranquangdang21/Projects/beads_rust/src /Users/tranquangdang21/Projects/beads_rust/Cargo.toml -> 0 hits for every pattern. `rg -ni 'telemetry|otel|prometheus|metrics' /Users/tranquangdang21/Projects/beads_rust/Cargo.toml` -> 0 hits (no OTEL/tracing-export dependency at all). LOCAL's only 'metrics' hits are the MCP `beads://graph/health` resource (src/mcp/resources.rs:773,844) and unrelated prose.

**Upstream evidence**

> internal/telemetry/telemetry.go:1-60 (package doc: BD_OTEL_ENABLED master switch, OTEL_EXPORTER_OTLP_METRICS_ENDPOINT, OTEL_EXPORTER_OTLP_LOGS_ENDPOINT, OTEL_TRACES_EXPORTER, VictoriaMetrics/Grafana reference stack); internal/telemetry/storage.go:46-56 (Int64Counter "bd.storage.operations", Float64Histogram "bd.storage.operation.duration" in ms, Int64Counter "bd.storage.errors", Int64Gauge "bd.issue.count"); internal/telemetry/storage.go:40 WrapStorage returns the store unchanged when telemetry is disabled (zero-overhead off path); cmd/bd/command_telemetry.go:22-60 (initTelemetry + startCommandSpan emitting `bd.command.<name>` with bd.command/bd.version/bd.args attributes, args scrubbed); internal/telemetry/ has 37 decorator files (commenter.go, deleter.go, ready_counter.go, graph_counter.go, sweeper.go, ...) each wrapping one storage operation.

</details>

<details><summary><b>No br events command: no queryable/tailable/exportable mutation journal</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** large

**Description**

> GO ships `bd events` with three subcommands — tail (--since seq, --limit, --follow streaming new records as they commit at 1s poll cadence), export (--limit), and prune (--before seq) — over a durable, append-only, gapless-seq-ordered events journal (bd_events_journal). Every committed mutation (create, update, close, reopen, delete, claim, dep add/remove, label add/remove, comment) is journaled in the SAME transaction as the change, with a counter-assigned gapless `seq` that is never reused or reset, a UTC `ts` stamped inside the committing transaction, the `op`, the `issue_id`, the `actor`, and the full post-mutation issue state (literal JSON null on delete). It is opt-in via `bd config set events-journal true` or BD_EVENTS_JOURNAL=1, and retention is automatic (7 days / 100k rows by default, with floors and an auto-prune switch). The journal record shape is a single published contract shared by the CLI and the HTTP API precisely to prevent two encoders of the same row from drifting. LOCAL has no `events` subcommand at all. Its nearest surfaces are `br history` (which is JSONL snapshot backup/restore, not a mutation journal) and the MCP resource beads://events/recent (a bounded recent-events read, no seq cursor, no follow, no export). DWS also lacks it.

**Impact**

> Scripts and integrations cannot replay a workspace's exact mutation history. An agent debugging 'how did this bead get into this state' has no seq-ordered feed to tail or export — only the issue's own comment/event projection, which loses cross-issue ordering and does not capture raw DML. External mirrors built on br have no stable incremental cursor to poll.

**Local evidence**

> rg -n 'Events' /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs -> 0 hits (no Events subcommand). `ls src/cli/commands/ | rg -i event` -> no events.rs. LOCAL's `rg -n 'seq' src/storage/events.rs` returns only one hit and it is a test name (src/storage/events.rs:804 test_multiple_event_types_sequence) — there is no gapless seq counter. LOCAL's only events readers: src/mcp/resources.rs:652-684 (beads://events/recent) and src/mcp/tools.rs:2910, both bounded recent reads with no --since/--follow/--before cursor. LOCAL's HistoryCommands (src/cli/mod.rs:3558-3582) are List/Diff/Restore/Prune over .br_history JSONL snapshots — a different artifact from a mutation journal.

**Upstream evidence**

> cmd/bd/events.go:33-90 (eventsCmd doc: append-only seq-ordered record of every committed issue mutation, written in the same transaction, OFF by default, retention floors events-journal-retain-days/-rows at 7 days/100k); cmd/bd/events.go:200-204 (tail flags --since/--limit/--follow, export --limit, prune --before); cmd/bd/events.go:206-209 (AddCommand wiring); cmd/bd/events.go:276 runEventsTail, :319 runEventsPrune; internal/eventsjournal/record.go:33-75 (Record struct: Seq gapless/never reused, TS RFC3339 normalized at the read seam, Issue always present and literally null on delete, Dep/Comment absent rather than null, and the module doc explaining the two-encoder drift this package exists to prevent); internal/eventsjournal/activation.go:1-20 (activation applied by store FACTORIES so doctor --fix repairs are journaled too); internal/eventsjournal/autoprune.go:61 AutoPruneConfigKey = "events-journal-auto-prune".

</details>

<details><summary><b>br doctor --bundle: incident-evidence bundle not implemented (LOCAL's own doc tracks it)</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS ships `br doctor --bundle <out.tar.gz>`, a one-command incident-evidence capture. The gzip'd tar contains a manifest.json (schema, br version, engine block, per-command exit codes), doctor.json, doctor-repair-dry-run.json, health.json, sync-status.json, where.json, config-list.txt, version.txt, a listings.json (names/sizes/mtimes under .beads/, .br_recovery/, .br_history/), a db-family.json (presence/size/mtime/SHA-256 of beads.db, -wal, -shm, -journal, cert and namespace files), a db-dump.json (metadata table, sqlite_master, recent events via a read-only connection), and copies of metadata.json/config.yaml. Database bytes and issues.jsonl are opt-in via --include-db / --include-jsonl; otherwise only their SHA-256 and size travel. All text members have e-mail addresses rewritten to <redacted-email>. Crucially, LOCAL's own docs/reliability/HEALTH_CONTRACT.md prescribes exactly this capture command under its 'Evidence Bundle (Incident Capture)' section and then states it does not exist — the upstream closed LOCAL's own tracked TODO.

**Impact**

> Every field failure in LOCAL requires the user to hand-assemble 11 artifacts across 3 directories, hash them by hand, and redact e-mail addresses by hand before they can be attached to a bug report. The prescribed redaction step is the one most likely to be skipped, so incident reports can leak personal data. This is a tracked LOCAL TODO that the upstream closed.

**Local evidence**

> rg -c -- '--bundle' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits; rg -c 'include-db' -> 0; rg -c 'include_jsonl' -> 0; rg -c 'BUNDLE_SCHEMA' -> 0; rg -c 'redacted-email' -> 0. LOCAL has no bundle.rs in doctor_subsystems/. The gap is self-documented: docs/reliability/HEALTH_CONTRACT.md:139-143 lists the 11 artifacts a reporter must assemble and then prints the capture command `br doctor --bundle /tmp/incident-$(date +%Y%m%d-%H%M%S).tar.gz` followed by "(Not yet implemented - tracked for future work.)" — that is the only 'not yet implemented' note in LOCAL's whole docs/ tree.

**Upstream evidence**

> src/cli/commands/doctor_subsystems/bundle.rs:1-25 (module doc enumerating every bundle member, the --include-db/--include-jsonl gating, and the e-mail redaction rule); bundle.rs:57 BUNDLE_SCHEMA = "br.doctor.bundle.v1"; src/cli/mod.rs:3448-3478 (--bundle with conflicts_with_all, --include-db, --include-jsonl, --bundle-events N default 200); docs/reliability/HEALTH_CONTRACT.md:157-172 in DWS replaces the '(Not yet implemented)' note with the live command plus a member-by-member table.

</details>

<details><summary><b>br doctor migrate-schema: no receipt-bound, reversible schema migration lifecycle</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** large

**Description**

> DWS adds a `br doctor migrate-schema` subcommand group with four verbs — plan, apply, undo, recover — implementing an explicit, receipt-bound schema migration lifecycle. `plan` observes the complete logical DB plus the raw SQLite file family and emits a deterministic token over the logical state. `apply` recomputes that plan under database-family write authority, refuses semantic drift, writes a verified recovery bundle of the then-current raw family, and runs only the reviewed migration steps in one BEGIN IMMEDIATE transaction; after commit it checkpoints, rebuilds indexes, rewrites pages, closes the writer, and requires a clean all-row integrity result from a fresh connection. `undo` verifies the live logical state is still the exact applied state, quarantines every current family member, and restores every pre-migration byte without deleting anything. Each stage has its own JSON receipt schema (plan/prepared/commit_ready/applied/failed/undo/recovery). LOCAL has no such subcommand and no schema_migration module — the ordinary storage open is the only migration path, and there is no operator-facing, reversible, drift-checked migration.

**Impact**

> A schema-version boundary crossing in LOCAL is a one-way door: there is no plan token to review before the change, no drift detection that would refuse a migration against unexpected state, and no undo that can restore pre-migration bytes. Recovery falls back to doctor --repair (rebuild from JSONL), which is strictly more destructive. The file is 275KB — this is the single largest subsystem DWS has that LOCAL does not.

**Local evidence**

> rg -c 'migrate-schema' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits; rg -c 'migrate_schema' -> 0 hits. LOCAL's DoctorSubcommand (src/cli/mod.rs:3708-3725) has exactly 6 variants: Capabilities, RobotDocs, Health, Ls, Undo, Explain — no MigrateSchema. LOCAL's doctor_subsystems/mod.rs has no schema_migration module. No `br.doctor.schema_migration.*` receipt schema exists in LOCAL.

**Upstream evidence**

> src/cli/commands/doctor_subsystems/schema_migration.rs:1-22 (module doc: ordinary opens never cross a schema-version boundary; this is the sole operator-facing reviewed-migration path, and the three properties it guarantees — plan token over logical state, drift refusal, verified recovery bundle, post-commit all-row integrity); schema_migration.rs:57-63 (PLAN_SCHEMA, PREPARED_SCHEMA, COMMIT_READY_SCHEMA, APPLIED_SCHEMA, FAILED_SCHEMA, UNDO_SCHEMA plus recovery.v1); src/cli/mod.rs:3503-3505 (MigrateSchema variant in DoctorSubcommand); src/cli/mod.rs:3568-3620 (DoctorMigrateSchemaArgs + Recover/Plan/Apply/Undo command enum and their arg structs).

</details>

<details><summary><b>No engine/on-disk state block in br info --json or br doctor --json</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS reports an `engine` block from both `br info --json` and `br doctor --json`: the storage engine name, its crate and version from Cargo.lock at build time, the database path, an inventory of the engine's sidecar files (-wal, -shm, -wal-cert, -wal-cert-head, -ns, -journal, -fsqlite-ns-gate, -fsqlite-ns-use) with byte size and age, a live probe of the sole-opener lease file (a non-blocking exclusive lock attempt that reports whether some open connection holds it), and a listing of recovery artifacts found up to 5 directories deep. It is strictly observational — files are only stat'ed and the lease probe takes and immediately releases the lock. DWS's module doc explains the motivation: the August 2026 corruption program required this to be assembled by hand during incident reports. LOCAL's `br info --json` has no engine field (its serialization struct contains only schema, projection, and similar blocks) and LOCAL has zero occurrences of sole_opener or the sidecar-inventory vocabulary.

**Impact**

> A br user filing a corruption incident cannot produce the engine/version/sidecar-inventory/lease-holder facts that the diagnosis actually turns on. LOCAL's `br doctor --json` output cannot answer 'which engine is this binary, how old is the -wal, is another process holding the opener lease' — the same manual assembly DWS built the block to eliminate.

**Local evidence**

> rg -ci 'sole_opener' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits. `rg -in 'engine|sidecar|fsqlite' src/cli/commands/info.rs` returns only the three fsqlite import lines (info.rs:10-12) — there is no engine struct, no EngineBlock field, no engine_block() call. LOCAL's recovery_artifacts name does appear (45 hits) but LOCAL reports a different concept (aged-quarantine check) with no sidecar inventory, no lease probe, and no engine version.

**Upstream evidence**

> src/cli/commands/doctor_subsystems/engine.rs:1-10 (module doc: the engine block reports which storage engine this binary was built against plus the on-disk state incident reports for the August 2026 corruption program had to assemble by hand); engine.rs:14-22 SIDECAR_SUFFIXES (8 suffixes) and :24 RECOVERY_ARTIFACT_LIMIT = 50; engine.rs:39-60 EngineBlock {name, crate, version, database, sidecars, sole_opener_lease, recovery_artifacts} and EngineFile {file, bytes, age_s}; wiring: src/cli/commands/info.rs:4 (import), :74 (engine: EngineBlock field), :173 (engine: engine_block(...)); src/cli/commands/doctor.rs:6 (import), :4189-4236 (ENGINE_BLOCK thread-local accessor and serialization), :13609 (set_engine_block(engine_block(...)) at the end of the doctor run).

</details>

<details><summary><b>No bd metrics command: no user-facing anonymous-usage metrics control plane</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> GO ships a `bd metrics` command group — metrics / metrics on / metrics off / metrics example — that lets a user inspect and change anonymous usage-metrics settings without hand-editing config or setting env vars. The subsystem reports only which commands run plus the bd version and OS platform, never issues, paths, remotes, identity, or user text. It has a file-backed event queue under ~/.beads/eventsData, a detached background flusher spawned after each command (guarded by BD_IS_FLUSHER against recursion, plus a flush-due marker to avoid respawn storms), a prune path for leftover queues from a previously-enabled config, a cached machine-ID probe resolved only on the enabled path, and standard opt-out env vars BD_DISABLE_METRICS and DO_NOT_TRACK. The opt-out is deliberately the one command that emits no event, with a comment explaining that recording a 'metrics-off' event would queue telemetry the user just declined and could flush it if they re-enable later. LOCAL has no metrics command, no telemetry subsystem, and no BR_DISABLE_METRICS/DO_NOT_TRACK handling. Note: LOCAL ships self_update against GitHub releases, so it is not offline-only, but it has no usage-reporting surface of any kind.

**Impact**

> Two-sided. Upstream's maintainers lose the usage signal that decides what to polish next; LOCAL users have no control plane because there is nothing to control. This is the one gap in the audit where 'port it' is a product decision rather than a parity fix — adding phone-home telemetry to a tool whose stated design is non-invasive and agent-trusted should be an explicit call, not a default.

**Local evidence**

> rg -n 'Metrics' /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs -> 0 hits (no metrics subcommand). rg -ci 'telemetry|anonymous usage|BD_METRICS|BR_METRICS|opt.out|machine_id' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits for every pattern. There is no user-facing analytics opt-out because there is no analytics — and I found no LOCAL doc stating this is a deliberate decision (see coverage_notes).

**Upstream evidence**

> cmd/bd/metrics.go:19-35 (metricsCmd doc + RunE), :49-68 (metricsOnCmd writing user-global config), :76-95 (metricsOffCmd with the 'opt-out is intentionally the one command that emits no cli_command event' comment), :99-118 (metricsExampleCmd + AddCommand wiring); internal/metrics/metrics.go:12-25 (EnvDisableMetrics BD_DISABLE_METRICS, EnvDisableEventFlush BD_DISABLE_EVENT_FLUSH, EnvDoNotTrack DO_NOT_TRACK, DefaultEndpoint, queue dir ~/.beads/eventsData); metrics.go:49-84 Init() with the NullEmitter when disabled and the distinct-ID resolved only on the enabled path; metrics.go:105-127 CloseAndFlush with a 500ms bounded close; internal/metrics/spawn.go:18 EnvIsFlusher = "BD_IS_FLUSHER", :45 shouldSpawnFlusher, :76 flusherDue, :107 hasQueuedEvents, :164 MaybeSpawnFlusher; cmd/bd/send_metrics.go:9-20 the hidden flush-only child.

</details>

<details><summary><b>Four DWS structured error codes are missing from LOCAL's taxonomy</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** small

**Description**

> The ErrorCode enum is the machine-readable error taxonomy that drives deterministic exit codes for scripts and agents. DWS carries four codes that LOCAL does not: UpdatePreconditionFailed (optimistic-concurrency/CAS failure — an update whose precondition no longer holds), ShuttingDown (the process is in graceful-shutdown and refused new work), CloseIncomplete (a close that could not be completed, e.g. a flush or dependency check did not finish), and WorkflowCapacityExceeded (a workflow is at its configured capacity). Each maps to its own exit code, so in LOCAL these conditions either collapse into a generic DatabaseError/InternalError with a non-specific exit code, or surface as untyped strings. The gap runs both ways in a small way — LOCAL carries SchemaSkew (structured.rs:56) which DWS dropped — but that is a LOCAL extra, not a missing capability, and the net set difference is four DWS codes.

**Impact**

> Scripts and agents keying on `error.code` cannot distinguish a lost-update race (UpdatePreconditionFailed) from a generic database error, cannot detect that a close was attempted but incomplete, and cannot recognise a capacity rejection. Each collapses to a coarser code with a different exit code, so a retry policy keyed on the fine-grained code either retries when it should not or gives up when it should retry.

**Local evidence**

> rg -c '^\s*(UpdatePreconditionFailed|ShuttingDown|CloseIncomplete|WorkflowCapacityExceeded),' /Users/tranquangdang21/Projects/beads_rust/src/error/structured.rs -> 0 hits for each of the four. LOCAL does carry `SchemaSkew` at src/error/structured.rs:56 (a LOCAL-only code DWS does not have), so the two taxonomies have genuinely diverged rather than LOCAL being a strict prefix. LOCAL's ErrorCode spans structured.rs:48-136 across database, issue, validation, dependency, jsonl/sync, config, io, and policy families.

**Upstream evidence**

> src/error/structured.rs:104 UpdatePreconditionFailed,; :128 ShuttingDown,, :132 CloseIncomplete,, :138 WorkflowCapacityExceeded, (all within the same enum that LOCAL's is a fork of; LOCAL's enum is 42KB, DWS's is 72KB, and src/error/mod.rs is 14KB vs DWS's 27KB).

</details>

<details><summary><b>Four doctor detectors present in DWS are absent from LOCAL</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** medium

**Description**

> The authoritative detector registry is the DETECTOR_ROWS table in doctor_subsystems/capabilities_doctor.rs, which `br doctor capabilities --format json` publishes to agents. DWS registers 58 detectors; LOCAL registers 54. The four DWS-only detectors are: config.unknown_keys (warn, fast_path) — every leaf key in .beads/config.yaml that no br code path reads, so typos and dead config are surfaced instead of silently ignored; permissions.db_sidecars (warn, fast_path) — the permissions of the SQLite sidecar family; db.namespace_identity (error, fast_path) — the storage engine's on-disk namespace identity, an error-severity state check; and db.foreign_recovery_debris (info, fast_path) — files and directories in .beads/ that look like another tool's or another install's recovery debris. All four are on the --quick fast path, so their absence is felt in the CI/pre-commit mode too, not just full doctor. LOCAL has zero occurrences of any of the four names.

**Impact**

> A typo'd or stale key in .beads/config.yaml is silently ignored in LOCAL but reported by DWS; a mis-permissioned -wal/-shm sidecar and namespace-identity divergence go unreported. Because all four are fast_path, `br doctor --quick` — the mode docs recommend for pre-commit and CI — is strictly weaker in LOCAL than in DWS.

**Local evidence**

> Extracted DETECTOR_ROWS first-column ids from both files: LOCAL = 54 unique ids, DWS = 58; `comm -13` yields exactly {config.unknown_keys, db.foreign_recovery_debris, db.namespace_identity, permissions.db_sidecars} and `comm -23` (LOCAL-only) is empty. Direct search: rg -c 'config.unknown_keys|db.namespace_identity|db.foreign_recovery_debris|permissions.db_sidecars' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits for each of the four.

**Upstream evidence**

> src/cli/commands/doctor_subsystems/capabilities_doctor.rs:259 ("config.unknown_keys", "configs", "warn", true); :278 ("permissions.db_sidecars", "permissions", "warn", true); :311 ("db.namespace_identity", "state_files", "error", true); :320 ("db.foreign_recovery_debris", "state_files", "info", true); :489 maps fixer filter_ids onto permissions.db_sidecars. Implementations: src/cli/commands/doctor.rs:9529-9576 check_config_unknown_keys (walks every leaf key in .beads/config.yaml and reports those no code path reads); doctor.rs:2769-2814 inspect_namespace_identity / check_namespace_identity; doctor.rs:3010-3144 is_foreign_recovery_debris_dir_name / _file_name / scan_foreign_recovery_debris / check_foreign_recovery_debris; doctor.rs:977 pairs the unknown-keys finding with the fm-configs-unknown-keys fixer id.

</details>

<details><summary><b>No config-persisted per-check doctor suppression (doctor.suppress.*)</b></summary>

- **Severity:** low · **Present in:** go-only · **Effort:** small

**Description**

> GO's doctor reads a `doctor.suppress.<slug>` config namespace (e.g. doctor.suppress.git-hooks = true) and drops those checks' findings from ordinary `bd doctor` runs, persisted in the workspace. It ships a name-to-slug converter so a human check name ('Git Hooks') maps to its config key ('git-hooks'). LOCAL has no such namespace: `br doctor --only` and `--skip` only gate which FIXERS run under `--repair` — they do not suppress a finding in a plain read-only `br doctor` run, and they are not persisted between invocations. The only suppression in LOCAL is the internal `is_quick_suppressed_doctor_check()` helper (src/cli/commands/doctor.rs:316), which hard-codes which checks `--quick` drops and is not operator-configurable. So an operator with a check that is permanently, knowingly failing in their environment (a legacy artifact, an unusual filesystem) has no supported way to quiet it in LOCAL, and must re-filter its output on every run.

**Impact**

> A permanently-known-failing check stays in every plain `br doctor` and every `--robot-triage` output in LOCAL, so agents re-triage a finding the operator has already accepted, and CI logs carry the same noise forever. GO lets the operator record the decision once in config.

**Local evidence**

> rg -in 'doctor\.suppress|suppress.*check|ignore.*check|disabled.*check' /Users/tranquangdang21/Projects/beads_rust/src -> 0 relevant hits (only unrelated gitignore.beads_inner_present and acceptance_criteria_ignores_unchecked_boxes matches). The sole suppression mechanism is the private helper at src/cli/commands/doctor.rs:316 `fn is_quick_suppressed_doctor_check(name: &str) -> bool`, consumed at doctor.rs:11366, which is a hard-coded allow-list for --quick only. LOCAL's --only/--skip (src/cli/mod.rs:3660-3680) are documented as --repair-only fixer gates and are not persisted.

**Upstream evidence**

> cmd/bd/doctor/suppress.go:13-14 (SuppressConfigPrefix = "doctor.suppress."; users set keys like doctor.suppress.git-hooks = true); :16-31 GetSuppressedChecks; :33-45 GetSuppressedChecksWithStore (shared store, GH#2636); :47-64 getSuppressedChecksFromStore (reads GetAllConfig, takes keys with the prefix whose value is "true"); :67-77 CheckNameToSlug converting a human check name to its config-friendly slug.

</details>

### C — `pagination` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | low | dws-only | `partial` | search JSON output omits total/has_more and silently truncates at --limit |

<details><summary><b>search JSON output omits total/has_more and silently truncates at --limit</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** small

**Description**

> LOCAL's `search` JSON/TOON output is a bare array (early_ctx.json_array) and truncates the result vector at --limit with no has_more/total signal, so a caller cannot tell whether more matches exist beyond the page. DWS's search returns a page envelope with `has_more` and also emits an explicit truncation note. Notably LOCAL's own `list --json` DOES compute total/has_more, so this is a search-specific inconsistency. GO search is also a bare array, so this is DWS-only.

**Impact**

> Agent pagination is unreliable: `br search --json --limit N` gives no way to know to request the next page, and LOCAL's list path having the envelope makes the search omission an inconsistency.

**Local evidence**

> rg -c 'has_more|total' src/cli/commands/search.rs -> 0 (search.rs never emits them). src/cli/commands/search.rs:145-151 truncates in place `if ... issues.len() > limit { issues.truncate(limit); }` and returns a bare `Vec<Issue>`; JSON output at 173-178 calls `early_ctx.json_array(...)` (bare array), not `json_array_page`. By contrast LOCAL list DOES use the page envelope (src/cli/commands/list.rs:104-187 computes total and has_more). GO confirmed bare too: cmd/bd/search.go has 0 hits for has_more/total and emits outputJSON(issuesWithCounts) at line 301.

**Upstream evidence**

> DWS src/cli/commands/search.rs:153-161 computes `let has_more = limit > 0 && issues.len() > limit;` and returns a `SearchPage{ issues, limit, offset, has_more, selection }`; the page envelope with has_more is rendered at src/cli/commands/search.rs:287-295 (`SelectedSearchResults{ ..., has_more }`), plus a truncation note via `emit_search_truncation_note` (src/cli/commands/search.rs:442).

</details>

### C — `query-dsl` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | low | go-only | `divergent_behavior` | Query DSL lowercases the metadata.<key> suffix, making mixed-case metadata keys unqueryable |

<details><summary><b>Query DSL lowercases the metadata.<key> suffix, making mixed-case metadata keys unqueryable</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> Both parsers lowercase the field name, but GO deliberately preserves the case of the `metadata.<key>` suffix because metadata keys are case-sensitive JSON keys; lowercasing them makes mixed-case keys silently unqueryable. LOCAL lowercases the entire field including the key suffix, so `metadata.Sprint=Q1` becomes `metadata.sprint` and the JSON lookup `v.get("sprint")` misses a key stored as `Sprint`. This is a correctness divergence in the shared query DSL.

**Impact**

> Cannot query or existence-check metadata whose keys are mixed-case (e.g. metadata.Sprint, metadata.jira.Sprint) — a common convention. Matches silently return empty rather than erroring.

**Local evidence**

> src/query/parser.rs:158 `let field = self.current.value.to_lowercase();` lowercases the whole field with no metadata exception. Downstream, src/query/evaluator.rs:648-650 does `let key = &field["metadata.".len()..]; ... v.get(key)` and src/query/evaluator.rs:433-436 pushes `format!("{key}={value}")` for the SQL path — both consume the already-lowercased key. No metadata case-preservation exists anywhere in LOCAL (rg -c 'metadata\.' in src/query/parser.rs shows only the generic starts_with check in evaluator).

**Upstream evidence**

> GO internal/query/parser.go:239-246 — comment 'metadata.<key> field keeps its original case: metadata keys are case-sensitive JSON keys ... so lowercasing it would make mixed-case keys silently unqueryable', then `field := strings.ToLower(...); if strings.HasPrefix(field, "metadata.") { field = "metadata." + p.current.Value[len("metadata."):] }`. Regression test internal/query/query_test.go:716 'metadata.Sprint=Q1 preserves key case' and invariant test TestMetadataKeysAreQueryable (internal/query/query_test.go:764).

</details>

### C — `robustness-portability` (1 finding)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | dws-only | `divergent_behavior` | Text-mode output aborts with SIGABRT on a closed pipe (no SIGPIPE handling) |

<details><summary><b>Text-mode output aborts with SIGABRT on a closed pipe (no SIGPIPE handling)</b></summary>

- **Severity:** critical · **Present in:** dws-only · **Effort:** small

**Description**

> `br`'s Plain/text output path uses bare `println!`, which panics on EPIPE. Combined with the release profile's `panic = "abort"`, a downstream reader that closes early (`br list | head`) turns into SIGABRT plus a core dump / macOS crash report instead of the conventional clean Unix-filter exit. LOCAL installs signal handlers for SIGINT/SIGTERM/SIGHUP only and never touches SIGPIPE, so nothing intercepts the panic. LOCAL's BrokenPipe handling exists but is wired exclusively to the JSON/TOON serializer, leaving the text path — which is exactly the path a non-TTY pipe selects — unguarded. DWS fixed this as GitHub #434 by restoring SIGPIPE to SIG_DFL at startup. This is the single most common shell pipeline an operator or agent runs against a CLI, and LOCAL's own AGENTS.md cites `br list | head -1` as a real invocation.

**Impact**

> Any `br <text-output> | head` / `| less` early-exit pipeline, on any Unix host, terminates with exit 134 and writes a crash report or core file. Because non-TTY stdout routes to `OutputMode::Plain`, this is precisely the piped path. Shell pipelines that use `set -o pipefail` (the default in many CI and agent harnesses) will additionally report the SIGABRT as a hard failure, and the crash artifact pollutes the workspace. The fix does not require relaxing LOCAL's `forbid(unsafe_code)`: LOCAL already owns the correct contract for the structured path, so the text writer can be routed through a `Write` impl that swallows `ErrorKind::BrokenPipe` exactly as `is_broken_pipe_serialization_error` does — a small, precedent-backed change plus an e2e test.

**Local evidence**

> `grep -rn SIGPIPE --include=*.rs src/ tests/` -> 0 hits. `src/shutdown.rs:79-82` registers only `signal_hook::consts::{SIGHUP, SIGINT, SIGTERM}`. `src/output/context.rs:1080` is the Plain branch of `print_line`: `OutputMode::Plain => println!("{content}")`; the same unguarded pattern repeats at :1351, :1397, :1409, :1415. The BrokenPipe guard that does exist is JSON-only: `src/output/context.rs:808 fn is_broken_pipe_serialization_error(err: &serde_json::Error)` is reached solely from `record_output_serialization_failure` (:54-55) and `report_serialization_error` (:1092-1093) — both on the serde_json serializer path. `Cargo.toml:151` sets `panic = "abort"`; `src/lib.rs:22` sets `#![forbid(unsafe_code)]`. No `tests/*broken*pipe*` file exists.

**Upstream evidence**

> DWS CHANGELOG.md:1450-1468, section 'Die like a Unix filter, not with a core dump (GitHub #434, commit 80ee8690)': "br list | head (any text-mode output into a closed pipe) previously aborted with SIGABRT: Rust ignores SIGPIPE, bare println! panics on EPIPE, and the panic=\"abort\" profile turned that into an abort with a macOS crash report. Text-mode commands now restore SIGPIPE to SIG_DFL at startup and die silently by signal 13 like every other Unix filter." The same entry records that the robot contracts keep a deliberate exit-0-on-EPIPE contract and that `br serve` keeps treating EPIPE as a hard error, and notes the restore is one of three sanctioned unsafe_code carve-outs (DWS Cargo.toml:238-244 sets `unsafe_code = "deny"` with `#[allow(unsafe_code)]` exceptions, where LOCAL uses `forbid`). Regression test: DWS tests/e2e_broken_pipe.rs (`#![cfg(unix)]`, asserts `SIGPIPE = 13` and `SIGABRT = 6`, and drives a pipe whose read end is dropped before the child starts).

</details>

### C — `search` (2 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | medium | dws-only | `missing_feature` | Full-text search does not include comment bodies |
| — | info | go-only | `divergent_behavior` | Default text search does not include external_ref |

<details><summary><b>Full-text search does not include comment bodies</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> DWS extends the search needle predicate to match comment text via an `issues.id IN (SELECT comments.issue_id FROM comments WHERE instr(lower(comments.text), ?) > 0)` clause (ported as beads_rust#416: 'agent workflows put durable handoffs and decisions in comments, so a comment-only token must still be findable'). LOCAL's three search code paths match only title, description and id, so any token that appears solely in a comment is unsearchable. GO likewise does not search comments, so this is a DWS-only capability.

**Impact**

> Agent-first workflows stash durable handoffs/decisions in comments, but `br search <token>` returns nothing for a token that only appears there. Cross-reference comments by ID still works, so this is a discoverability gap, not data loss.

**Local evidence**

> LOCAL search WHERE is only 3 fields: src/storage/sqlite.rs:5132, 5246, 5276 all read "AND (instr(lower(title), ?) > 0 OR instr(lower(description), ?) > 0 OR instr(lower(id), ?) > 0)". A targeted search of the whole LOCAL search region (lines 5100-5290) for 'comments|comment' returned 0 hits, and none of LOCAL's `FROM comments`/`JOIN comments` uses (sqlite.rs:3250,8706,8736,8793,9695,9718,10166) are in the search path. Confirmed LOCAL has no comment-body search.

**Upstream evidence**

> DWS src/storage/sqlite.rs:1645-1649 SEARCH_NEEDLE_PREDICATE (and 1658-1662 SEARCH_COUNT_NEEDLE_PREDICATE) add `OR issues.id IN (SELECT comments.issue_id FROM comments WHERE instr(lower(comments.text), ?) > 0)`; documented at src/storage/sqlite.rs:1582-1583 (beads_rust#416). Regression test src/cli/commands/search.rs:1077 `test_search_matches_comment_bodies` asserts a token existing only in a comment body is returned.

</details>

<details><summary><b>Default text search does not include external_ref</b></summary>

- **Severity:** low · **Present in:** go-only · **Effort:** small

**Description**

> GO's default search predicate also matches external_ref (when the query looks like an issue ID, the clause is id/title/external_ref), so searching a tracker key can find the linked issue. LOCAL's text search never includes external_ref, and DWS's does not either; this is a GO-only behavior.

**Impact**

> Searching an external ticket key (e.g. a JIRA id) will not surface the linked issue unless it also matches title/description. Niche and only active for id-like queries.

**Local evidence**

> LOCAL search WHERE is title/description/id only (src/storage/sqlite.rs:5132, 5246, 5276); rg -c 'instr(lower(external_ref|external_ref.*instr' src/storage/sqlite.rs -> 0. LOCAL has an exact --external-ref filter flag but it is not consulted by free-text search.

**Upstream evidence**

> GO internal/storage/sqlbuild/filter.go:71 default text search when LooksLikeIssueID(query): '(id = ? OR id LIKE ? OR LOWER(title) LIKE ? OR LOWER(external_ref) LIKE ?)'.

</details>

### C — `storage-durability` (4 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | high | dws-only | `missing_feature` | No detection or recovery for poisoned/corrupt WAL index (-shm); every mutating command fails with an undiagnosable "database is busy" |
| — | medium | go-only | `missing_feature` | No MIGRATION-FREEZE marker: an operator cannot quiesce a workspace to stop writes during a torn upgrade or manual repair |
| — | low | dws-only | `missing_feature` | No database-inode write authority: SQLite's own file lock is the only writer serialization, so two hard-link aliases of one database can write concurrently |
| — | info | go-only | `architecture_gap` | No pluggable storage-backend registry: the Storage trait exists but cannot be swapped at runtime, and no conformance suite proves a substitute behaves like the reference |

<details><summary><b>No detection or recovery for poisoned/corrupt WAL index (-shm); every mutating command fails with an undiagnosable "database is busy"</b></summary>

- **Severity:** critical · **Present in:** both-upstreams · **Effort:** large

**Description**

> DWS ships a 36KB src/franken_sync/wal_index.rs that detects and recovers the exact GitHub #507 crash signature: a valid checksummed WAL header sitting beside a WAL-index (-shm) that is present but zero-page/initialized-poison. probe() reads the first 32 WAL bytes and 96 SHM bytes and confirms the poison via poisoned_headers(); a read-only open returns an advisory diagnosis only. The writable identity-bound open quarantines that one derived-cache file under BOTH the engine namespace lock and SQLite's own main-file lock range (open_regular() refuses symlinks via O_NOFOLLOW, hard links via nlink != 1, and non-regular files), then reopens — main/WAL/journal/certificate bytes are never rewritten or removed. The doctor recovery workflow layers a complete backup, a private rehearsal on a copy, protected-byte comparison and logical attestation on top, and only then admits the engine. DWS ALSO runs this automatically before startup classifies the real pending receipt, so a poisoned index is repaired transparently on the next mutating command. Critically, the remediation text explicitly warns the operator not to delete the WAL and not to rebuild from JSONL, because committed records may exist ONLY in the WAL. LOCAL has no probe, no poison classification, no quarantine, no rehearsal, and no equivalent remediation: it reports the misleading "database is busy" forever while `br doctor` stays green because integrity_check is org-only in SQLite.

**Impact**

> A workspace that hits the #507 crash signature is permanently wedged on the LOCAL: every mutating command returns "database is busy", the application-level retry loop (src/storage/sqlite.rs:1495-1530, 8 attempts x 50ms..6400ms jittered backoff) burns ~12.7s per command before failing, and `br doctor` reports no problem because the poison lives in the derived index that SQLite's org-only PRAGMA integrity_check never inspects. The operator has no signal telling them the WAL still holds committed data, so the natural reflexes — rebuild from JSONL, or delete the -wal — both silently destroy committed issues. DWS turns this silent data-loss trap into a detected condition with an auto-repair on next startup and a remediation string that explicitly forbids both destructive reflexes.

**Local evidence**

> rg -n "poisoned_index_present|wal_index_needs_recovery|recover_wal_index_for_startup|wal_index::" -g '*.rs' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits. Confirmed by explicit sweep: poisoned_index_present=0, wal_index_needs_recovery=0, recover_wal_index_for_startup=0. DWS has the same three symbols with 3 hits each. LOCAL's closest analog is detect-only: src/cli/commands/doctor.rs:1784 inspect_database_sidecars() reports db.sidecars via FM_WAL_SHM_SIDECAR_ORPHAN (doctor.rs:701), but its only branches are (a) sidecar present while main DB is not a regular file, (b) sidecar is not a regular file, (c) WAL-without-SHM (doctor.rs:1812-1821, treated as the expected frankensqlite operating state), (d) SHM-without-WAL (doctor.rs:1824). A zero-page -shm BESIDE a valid non-empty WAL matches none of these, so db.sidecars returns Ok. No `br doctor migrate-schema recover` subcommand exists (rg "migrate-schema|MigrateSchema" -> 0 hits). No rehearsal / private-copy recovery path (rg "rehears" -> 0 hits; doctor.rs has emit_recovery_audit_record at :412 but only for the existing schema-migration path).

**Upstream evidence**

> DWS: src/franken_sync/wal_index.rs:1 module doc ("Narrow containment for the initialized, zero-page WAL index in GitHub #507"); :149 poisoned_headers(wal:&[u8;32], shm:&[u8;96]) checks the zero fields (shm[14..24] and shm[32..40] all zero); :162-170 probe() reads both prefixes and returns the verdict; :172 pub fn poisoned_index_present; :44-60 open_regular() refuses symlinks (O_NOFOLLOW/O_CLOEXEC), non-regular files, and hard links (nlink != 1 on unix); :75 DATABASE lock offset. Startup gate: src/main.rs:126 calls wal_index_needs_recovery(&paths.db_path) and on true src/main.rs:129-140 takes blocking_database_family_write_lock_with_timeout then calls recover_wal_index_for_startup. Recovery fn: src/cli/commands/doctor_subsystems/schema_migration.rs:376 wal_index_needs_recovery, :387 recover_wal_index_for_startup (verifies authority sha256 + verify_database_authority, requires a verified sole opener, then recover_engine_admission_with_lease). Quarantine on open: src/franken_sync.rs:292 wal_index::quarantine_poisoned_index(&path, identity). Doctor diagnosis + remediation: src/cli/commands/doctor.rs:1249 poisoned_index_present -> DatabaseAdmissionHint::PoisonedWalIndex, and :1258-1268 remediation text naming `br doctor migrate-schema recover` and warning "Do not run generic br doctor --repair or delete the WAL: committed records may exist only in WAL." Runtime warning: src/franken_sync/wal_index.rs:176 warn_if_poisoned emits diagnostic=WAL_INDEX_POISONED; wired at franken_sync.rs:165, :307, :586 and prepared.rs:67. Crash-injection test harness: src/franken_sync/wal_index.rs:441 CRASH_STAGE_ENV=BR_TEST_507_CRASH_STAGE. GO has no equivalent (its backend is Dolt, not SQLite WAL).

</details>

<details><summary><b>No MIGRATION-FREEZE marker: an operator cannot quiesce a workspace to stop writes during a torn upgrade or manual repair</b></summary>

- **Severity:** high · **Present in:** go-only · **Effort:** medium

**Description**

> GO ships a plain-file write-freeze marker (internal/migration/freeze.go, GitHub dc-6jaq) that an operator or external orchestration tool creates to stop all writes against a workspace during a migration, and removes to resume. bd only ever READS it, before a write command runs, so a human typing `bd create`/`bd update` mid-migration cannot slip a write past whatever quiesced the workspace. The marker is looked up in a start directory AND every ancestor up to the filesystem root, so one marker at the top of a multi-repo tree freezes every workspace beneath it; BD_MIGRATION_FREEZE_FILE overrides with an authoritative single path and doubles as the opt-out. Reads are size-capped at 64KB because "the ancestor walk can reach directories bd does not control" and a freeze must not become a multi-gigabyte allocation. The payload carries Operator, Reason and Timestamp. The gate is enforced in two shapes: a refusing CheckReadonly() that every mutating command calls (cmd/bd/errors.go:201, wired at close.go:52, create.go:55, memory.go:260, compact_proxied_server.go:23), and a print-nothing migrationFreezeActive()/migrationFreezeActiveFor() probe that lets diagnosis paths keep running while frozen but skip their own maintenance writes (doctor.go:592 skips writes when migrationFreezeActiveFor(beadsDir)). bd doctor deliberately has no freeze override (doctor.go:381-383). LOCAL has no such marker: nothing can externally stop an agent or a human from writing to a workspace that is mid-migration.

**Impact**

> On LOCAL there is no way for an operator to put a workspace into a known-quiesced state. During a schema migration, a torn-upgrade repair, or a forensic recovery, any concurrently running br process — a human at a terminal, a cron job, or another swarm agent — can write through the exact window the operator is trying to protect, re-corrupting the state being repaired. LOCAL's own doctor docs make the hazard concrete: doctor.rs:585-592 in GO exists because writes "land in the doctor target" during a freeze; LOCAL has no equivalent switch, and its config rebuild path (src/config/mod.rs:1746-1798) mutates the live database via VACUUM/REINDEX/VACUUM INTO with no external veto an operator can apply.

**Local evidence**

> rg -ni "MIGRATION-FREEZE|migration.freeze|freeze_marker" -g '*.rs' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits. rg -rn "MIGRATION-FREEZE" docs *.md -> 0 hits. LOCAL's only read-only concept is a process-level DisplayMode value (src/write_combining.rs:70 ReadOnly, used for write-combining dispatch at :873, :907, :1575), not an externally settable gate; the readonly references in doctor.rs:750/:7107 and doctor_subsystems/mutate.rs:95 are filesystem-permission observations, not a write veto. No freeze marker is documented as an intentional omission in CHANGELOG.md, docs/operations/UPGRADE_LOG.md, docs/reliability/HEALTH_CONTRACT.md or docs/BD_VS_BR.md.

**Upstream evidence**

> GO: internal/migration/freeze.go:1-9 package doc — "a human typing 'bd create'/'bd update' mid-migration cannot slip a write past whatever quiesced the workspace (dc-6jaq)"; :22 FileName = "MIGRATION-FREEZE" searched in a start directory and every ancestor to the filesystem root; :26 EnvFreezeFile = "BD_MIGRATION_FREEZE_FILE" as authoritative override and opt-out; :36 maxMarkerSize = 64<<10 with the rationale that the ancestor walk can reach directories bd does not control; :43 Info{Operator, Reason, Timestamp}. Gate: cmd/bd/errors.go:201 CheckReadonly calls migrationFreezeError and exits ExitMigrationFrozen; :341 migrationFreezeGateFor / :348 migrationFreezeErrorFor / :365 migrationFreezeActiveFor; :212-224 freezeSearchRoots adds the resolved workspace so BEADS_DIR / -C / cron callers walk the right tree. Call sites: cmd/bd/create.go:55, close.go:52, memory.go:260, compact_proxied_server.go:23, compact_dolt_proxied_server.go:16. Doctor: cmd/bd/doctor.go:381-383 "There is deliberately no doctor-side override. A migration freeze is exactly..."; :592 `if !readonlyMode && !migrationFreezeActiveFor(beadsDir)` skips doctor's own writes while still reporting. CHANGELOG.md:655 records the feature and dc-6jaq.

</details>

<details><summary><b>No database-inode write authority: SQLite's own file lock is the only writer serialization, so two hard-link aliases of one database can write concurrently</b></summary>

- **Severity:** high · **Present in:** both-upstreams · **Effort:** large

**Description**

> DWS cannot just flock() the database file: src/sync/db_inode_lock.rs:8-56 documents that flock(LOCK_EX) and POSIX fcntl record locks share one kernel lock table on macOS/BSD and conflict EVEN WITHIN A SINGLE PROCESS, which made every subsequent frankensqlite fcntl(F_SETLK) fail with EAGAIN ("database is busy" on a freshly initialised workspace), and that Windows LockFileEx is mandatory and collides with the engine's own lock-byte ranges. Its fix (GitHub #412) is a one-byte range lock at DATABASE_INODE_LOCK_OFFSET = i64::MAX-1 (db_inode_lock.rs:75) using an open-file-description lock (fcntl F_OFD_SETLK) on unix and LockFileEx at the same offset on Windows — an offset beyond any real database size, so it never overlaps SQLite's 0x40000000..0x40000200 lock range. That lock is carried by DatabaseFamilyWriteLock, bound to the database by a canonical-path SHA-256 (sync/mod.rs:1714 database_write_authority_sha256) verified via verify_database_authority (sync/mod.rs:1227), and paired with DatabaseOpenerLease (registered at sqlite.rs:2489/:2658) which refuses recovery unless a verified sole opener holds the lease. The mechanism is selected by ExclusiveLockMechanism::DatabaseInode vs ::LockSidecar (sync/mod.rs:288-295) precisely so sidecar locks and engine-locking inodes are treated differently. LOCAL's only cross-process write serialization is a .write.lock sidecar flock keyed on beads_dir, with no binding to the database identity, no sole-opener enforcement, and no hard-link aliasing defense.

**Impact**

> Two br processes operating on hard-link aliases of the same physical beads.db (e.g. a mirrored checkout path, a bind mount, or a symlink-free hard-link farm) each pass LOCAL's .write.lock test — the locks live on different inodes because the lock file is resolved under beads_dir — and then contend inside the engine, where the only remaining guard is a 12.7s application-level retry against SQLITE_BUSY. Under sustained contention LOCAL can also hit the frankensqlite busy-handler hot-spin problem its own comments document (src/storage/sqlite.rs:36-41, busy_timeout deliberately set to 0 because "frankensqlite's busy_timeout implementation uses a [hot-spinning] handler that starves the competing writer"). DWS makes this class of bug structurally impossible by locking the database inode itself at an offset the engine never touches, which is precisely the fix upstream needed to keep macOS/Windows users off a permanently-busy workspace.

**Local evidence**

> rg -n "DatabaseFamilyWriteLock|DatabaseOpenerLease|authority_sha|database_write_authority|blocking_database_family" -g '*.rs' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits. rg -n "db_inode_lock|OFD|ofd" -g '*.rs' src -> 0 hits. rg -n "hard.?link|nlink|file_identity|same_file" -g '*.rs' src -> 0 functional hits (only unrelated comments about unlinking ephemeral files at sqlite.rs:362, :1128 and about Link in the doctor fixer at doctor_subsystems/mutate.rs:1319). The entire public lock surface is three functions: src/sync/mod.rs:58 blocking_write_lock, :68 blocking_write_lock_with_timeout, :149 try_sync_lock — all keyed on beads_dir.join(".write.lock") / ".sync.lock". No identity check, no offset, no OFD, no opener lease.

**Upstream evidence**

> DWS: src/sync/db_inode_lock.rs:1-56 module doc for #412 (flock/fcntl conflict on macOS/BSD and in-process; LockFileEx mandatory-lock breakage on Windows; Linux-only invisibility); :75 pub const DATABASE_INODE_LOCK_OFFSET: i64 = i64::MAX - 1; :81 impl crate::franken_sync::wal_index::RecoveryLock; src/sync/mod.rs:288-295 enum ExclusiveLockMechanism { LockSidecar, DatabaseInode } with the DatabaseInode doc; :1714 pub fn database_write_authority_sha256; :1723 pub fn blocking_database_family_write_lock_with_timeout; :471/:586 authority_path_sha256; :1227 verify_database_authority. Opener-lease enforcement: src/storage/sqlite.rs:2489 and :2658 DatabaseOpenerLease::register(path), and schema_migration.rs:415-424 recover_engine_admission_with_lease which errors with "engine recovery requires a verified sole opener; close peer br processes and retry" unless lease.try_exclusive() succeeds. Authority rebind during WAL-index recovery: schema_migration.rs:391-395 rejects recovery when database_write_authority_sha256(db_path) != authority.authority_path_sha256(). GO independently has a shared/exclusive flock family (internal/lockfile/lock_shared_unix.go, lock_unix.go) that LOCAL's lock surface also lacks.

</details>

<details><summary><b>No pluggable storage-backend registry: the Storage trait exists but cannot be swapped at runtime, and no conformance suite proves a substitute behaves like the reference</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** large

**Description**

> GO exports a public out-of-tree backend seam: backend/backend.go type-aliases storage.DoltStorage and the Backend struct so an external Go module literally implements the in-tree engine interface, then calls backend.Register("name", Backend{Open, OpenReadOnly}) at process init. internal/storage/backends/backends.go guards registration (panics on empty name, the reserved name "dolt", or a missing Open/OpenReadOnly) and stores the registry plus the backendnames set that config validation reads under one RWMutex, updating both together so they cannot diverge. Crucially GO also ships backend/conformance — a test suite a third-party backend runs against itself, with RunAll covering the portable raw surface and RunUnsupportedContract covering the seven capability sub-interfaces (VersionControl, HistoryViewer, RemoteStore, SyncStore, FederationStore, CompactionStore, FastStatisticsStore) the suite does not call. That is the durability-relevant part: it means a substitute backend's transaction and commit semantics are proven equivalent rather than assumed. LOCAL declares a Storage trait explicitly intended to permit substitution, but the CLI binds to the concrete SqliteStorage type, and the only dynamic-dispatch instantiation is a test. There is no registry, no name-based selection, no conformance harness, and no second production backend.

**Impact**

> Two consequences, one durable and one structural. Durable: LOCAL cannot adopt an alternative engine (a different SQLite build, an embedded KV store, a server-backed store) for durability reasons — a crashing or busy-susceptible workspace has no escape hatch other than JSONL export and manual re-import, with no way to prove a replacement preserves commit and transaction semantics. Structural: the trait's documented substitutability claim is not exercised by the CLI, so the abstraction is currently aspirational; a reviewer reading trait_.rs:3-4 would reasonably expect a seam that does not exist at the dispatch layer. Severity is medium rather than high because LOCAL's JSONL round-trip is a complete, tested escape hatch, and because no concrete durability defect follows from the seam's absence — it is a capability and verifiability gap, not a bug.

**Local evidence**

> rg -ni "register_backend|BackendRegistry|dyn Storage|storage_backend" -g '*.rs' /Users/tranquangdang21/Projects/beads_rust/src -> 2 hits, both documentation/benign: src/storage/trait_.rs:17 ("For cases that need dynamic dispatch (e.g., mock/test backends), use `Box<dyn Storage>`") and :633 (a unit test doing `let store: Box<dyn Storage> = Box::new(InMemoryStorage::new()...)`). The trait doc at src/storage/trait_.rs:3-4 claims it "allows alternative backends (SQLite, in-memory for tests, etc.) to be substituted without changing the command layer", but src/cli/mod.rs binds &mut SqliteStorage directly. rg -ni "dolt|postgres|pgsql|libsql|duckdb|sqlx" -g '*.rs' src -> 0 functional hits (only the string "bd-dolt" appearing as an example ID prefix at cli/mod.rs:3286 and storage/sqlite.rs:3323). No backendname selection, no conformance test package.

**Upstream evidence**

> GO: backend/backend.go:1-62 package doc describing the out-of-tree contract and the minimal example; :76 pub type DoltStorage = storage.DoltStorage; :140 type Backend = backends.Backend; :147 func Register(name string, backend Backend); :163 func Registered(name string) bool; :171 WorkspaceIsBeadsDir. internal/storage/backends/backends.go:1-23 package doc ("OSS Beads registers no alternate backend. A downstream distribution adds a registrant package and blank-imports it"); :45 Backend{Open, OpenReadOnly, WorkspaceIsBeadsDir}; :48-52 `var (mu sync.RWMutex; registry = make(map[string]Backend))`; :53-60 Register panics on empty name, reserved "dolt", or nil Open/OpenReadOnly. Conformance suite: backend/conformance/ (referenced from backend.go:36-41 and :46-52), with the two explicitly non-interchangeable halves documented. Capability sub-interfaces proved only by allowlist: internal/storage/storage.go:639 FastStatisticsStore, :648 DoltStorage, plus VersionControl / HistoryViewer / RemoteStore / SyncStore / FederationStore / CompactionStore.

</details>

### C — `sync-jsonl` (9 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | low | go-only | `divergent_behavior` | `br federation sync` clobbers the peer's JSONL before reading it; no real remotes, no conflict strategy, no multi-peer |
| — | low | dws-only | `missing_flag` | DWS lossless/reviewed reconcile modes (--reconcile, --reconcile-additive, --apply + --expect-plan-sha256) have no LOCAL equivalent |
| — | low | dws-only | `architecture_gap` | DWS three-layer write authority (workspace lock + canonical-path sidecar + SQLite-safe inode OFD lock) absent; LOCAL holds one unbound .write.lock |
| — | low | go-only | `missing_feature` | Persistent memories (`br remember`) never travel through LOCAL's JSONL sync; GO round-trips them as `_type: "memory"` records |
| — | info | go-only | `architecture_gap` | DWS federation sync is a real pull-then-push against Dolt remotes; LOCAL's federation remote is a local file path only |
| — | low | go-only | `missing_flag` | GO export record-type scoping (--all, --include-infra, --scrub, --include-memories, --exclude-owner) absent; LOCAL export filters issue rows only |
| — | low | go-only | `missing_flag` | GO's `bd import --dry-run` classification preview and `--dedup` title-dedup have no LOCAL equivalent |
| — | low | dws-only | `divergent_behavior` | DWS freezes one tombstone-retention cutoff per export; LOCAL re-evaluates age per issue against the wall clock |
| — | info | dws-only | `partial` | DWS emits a hash-bound export publication receipt with a retained recovery path; LOCAL records only a chunked witness |

<details><summary><b>`br federation sync` clobbers the peer's JSONL before reading it; no real remotes, no conflict strategy, no multi-peer</b></summary>

- **Severity:** critical · **Present in:** go-only · **Effort:** large

**Description**

> LOCAL's `federation sync` is a two-step stub with the steps in the destructive order. Step 1 exports the local DB over the peer's file with force:true and allow_external_jsonl:true; Step 2 then 'imports the peer's JSONL' — but by then the peer file contains only local content, so nothing is ever pulled and the peer's divergent history is destroyed on disk. There is no pull-then-push, no conflict detection, and no strategy switch. The CLI even labels it 'Sync with a peer (stub)'. It also rejects every non-file protocol, so an https:// or ssh:// peer added via `federation add` (whose own help says 'Remote URL (e.g., https://beads.example.com)') is accepted at add time and then hard-fails at sync time. And it bypasses LOCAL's own path allowlist: it calls export_to_jsonl/import_from_jsonl directly with allow_external_jsonl:true against an operator-supplied remote_url, whereas the normal sync path validates through validate_sync_path_with_external.

**Impact**

> Any operator who registers a second machine/workspace as a federation peer and runs `br federation sync` silently overwrites the peer's issues.jsonl with the local clone's content and reports success ('Exported: N issues / Imported: N issues / Last sync: <ts>'). The peer's unique issues are gone from that file with no conflict report, no backup, and no non-zero exit. Because the same command also writes to an arbitrary operator-supplied path with the external-path guard disabled, it is also the one sync surface in LOCAL that is not covered by the docs/SYNC_SAFETY.md allowlist model.

**Local evidence**

> src/cli/commands/federation.rs:390 '// Step 1: Export local DB to the peer path' followed by sync::export_to_jsonl(&storage, &sync_path, &export_config) at :406-410 with export_config force:true (:396), allow_external_jsonl:true (:399); then :412 '// Step 2: Import peer's JSONL into local DB' calling sync::import_from_jsonl(&mut storage, &sync_path, ...) at :425 with force_upsert:true (:420) and allow_external_jsonl:true (:421). Protocol gate at :377-381 rejects any remote_url containing '://' that is not file:// with 'unsupported protocol in remote_url'. FederationSyncArgs at src/cli/commands/federation.rs:65-69 has exactly one field, `pub name: String` — no --peer-optional, no --strategy. rg -c 'strategy' src/cli/commands/federation.rs -> 0 hits. Contrast the enforced allowlist LOCAL uses elsewhere: src/sync/mod.rs:4577-4585 calls validate_sync_path_with_external(input_path, beads_dir, config.allow_external_jsonl) at the top of import_from_jsonl.

**Upstream evidence**

> GO cmd/bd/federation.go:38-52 long help: 'Pull from and push to peer towns. Without --peer, syncs with all configured peers. With --peer, syncs only with the specified peer. Handles merge conflicts using the configured strategy: --strategy ours Keep local changes on conflict / --strategy theirs Accept remote changes on conflict. If no strategy is specified and conflicts occur, the sync will pause and report which tables have conflicts for manual resolution.' Flag at cmd/bd/federation.go:130 `federationSyncCmd.Flags().StringVar(&federationStrategy, "strategy", "", "Conflict resolution strategy (ours|theirs)")`; validation at cmd/bd/federation.go:168-169; delegation at cmd/bd/federation.go:198 `result, err := ds.Sync(ctx, peer, federationStrategy)`; conflict reporting at cmd/bd/federation.go:222. GO's main `bd sync` (cmd/bd/sync.go:650-719) is a separate full federation loop (pull -> positively detect merge conflicts -> recompute is_blocked -> bounded-retry push) with machine-readable exit codes 0/1/2/3/4.

</details>

<details><summary><b>DWS lossless/reviewed reconcile modes (--reconcile, --reconcile-additive, --apply + --expect-plan-sha256) have no LOCAL equivalent</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** large

**Description**

> LOCAL's only JSONL->DB paths are `sync --import-only` (content-hash collision detection + updated_at comparison, may be combined with --rebuild which physically deletes DB rows absent from the JSONL) and `sync --merge` (whole-record 3-way merge, four strategies, one side always wins wholesale). DWS adds a distinct family of modes: `--reconcile` classifies every row against full issue state rather than the cached content hash and applies creates + timestamp-newer updates in place, never resetting tables, never deleting issues, never writing events or JSONL, and preserving all audit history; `--reconcile-additive` is a read-only-by-default lossless planner that keys on exact issue IDs, keeps every DB-only row and every audit event, and refuses to do content-hash identity merges or physical deletes; and `--apply --expect-plan-sha256 <token>` commits such a plan through a crash-recoverable publication saga, refusing to mutate if the source, DB, operation, resolution set, or expected post-state now yields a different plan. `--migrate-source-repo-path` normalizes source_repo_path atomically under the same review gate, and `--resolve-source-id` resolves one reviewed scalar conflict in favour of JSONL without replacing relations.

**Impact**

> On a workspace where the JSONL was hand-edited, restored from an older snapshot, or produced by a different tool, LOCAL offers no way to bring it in without either (a) --import-only, which keys identity off the cached content hash and can therefore mis-pair or skip rows, or (b) --rebuild, which physically deletes every DB row absent from the JSONL and drops the corresponding audit history. DWS's user can preview a hash-bound plan, read it, and commit it only if the inputs are byte-identical to what they reviewed. LOCAL's operator has to choose between a lossy destructive rebuild and an unreviewed import, with no plan/receipt artifact either way.

**Local evidence**

> rg -n 'reconcile_additive|migrate_source_repo_path|expect_plan_sha256|resolve_source_id|AdditiveReconcile|plan_additive_reconcile|plan_sync_reconcile|apply_sync_reconcile|plan_reviewed_additive_reconcile|ReconcilePlan' /Users/tranquangdang21/Projects/beads_rust/src /Users/tranquangdang21/Projects/beads_rust/tests -> 0 hits for reconcile_additive, migrate_source_repo_path, expect_plan_sha256, resolve_source_id, AdditiveReconcile, plan_additive_reconcile, plan_sync_reconcile, apply_sync_reconcile, plan_reviewed_additive_reconcile, ReconcilePlan. The only 'reconcile' hits in LOCAL src/ are English prose (src/sync/mod.rs:2808,2814; src/config/mod.rs:2692; src/storage/sqlite.rs:732; doctor.rs:11303-11321). LOCAL SyncArgs is src/cli/mod.rs:3332-3458 and its entire flag set is: --flush-only, --import-only, --merge, --status, --witness, --witness-chunk-lines, --witness-parallelism, --export-parallelism, --force/-f, --force-db, --force-jsonl, --allow-external-jsonl, --manifest, --error-policy, --orphans, --rename-prefix, --rebuild, --robot. LOCAL's --rebuild is the opposite of lossless: src/cli/mod.rs:3447-3451 'Rebuild the database from JSONL (removes orphaned DB entries) ... This ensures the DB exactly matches the JSONL source of truth.'

**Upstream evidence**

> DWS src/cli/mod.rs:2994-3002 `--reconcile` ('Classifies every JSONL row against full issue state instead of the cached content hash ... Never resets tables, never deletes issues, never writes events or JSONL, and preserves all audit history'); :3004-3013 `--dry-run`; :3033-3040 `--reconcile-additive` ('read-only by default ... never performs content-hash identity merges, physical deletes, base snapshot writes, or JSONL writes'); :3042-3050 `--migrate-source-repo-path`; :3052-3057 `--apply`; :3059-3065 `--expect-plan-sha256`; :3067-3076 `--resolve-source-id`. Implementations: src/sync/mod.rs:7590 plan_additive_reconcile, :5058 plan_reviewed_additive_reconcile, :5123 apply_reviewed_additive_reconcile, :15557 plan_sync_reconcile, :15684 apply_sync_reconcile, :5367 AdditiveReconcilePlan, :4743 AdditiveReconcileReceipt, :4734 AdditiveContentHashRepairWitness, :4647 AdditiveConflictRelationDiffWitness. Mode validation at src/cli/commands/sync.rs:795-911. DWS CHANGELOG.md:1083 'Additive sync --reconcile gained hash-bound dry-run/apply receipts'.

</details>

<details><summary><b>DWS three-layer write authority (workspace lock + canonical-path sidecar + SQLite-safe inode OFD lock) absent; LOCAL holds one unbound .write.lock</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** large

**Description**

> LOCAL serializes writers with a single advisory flock on .beads/.write.lock plus a non-blocking .sync.lock. The lock is bound to nothing: it records no canonical path, no device/inode identity, and nothing re-verifies that the DB or JSONL it was taken for is still the same file. DWS replaces this with a composite three-layer authority — workspace lock for single-workspace serialization, a canonical-path-derived sidecar that survives atomic replacement and pre-database existence, and a one-byte OFD lock on the database inode at an offset that provably never overlaps SQLite's lock-byte range — plus continuous re-verification (verify_jsonl_authority / verify_database_authority) that turns 'the file I locked got swapped underneath me' into a hard error. DWS's own header explains why the naive flock-on-database approach was catastrophically wrong on macOS/Windows (flock and POSIX fcntl locks share one kernel lock table, so locking the DB inode made every frankensqlite F_SETLK fail with 'database is busy').

**Impact**

> br's whole multi-clone model is a JSONL file that gets atomically replaced by git checkout/pull/merge while other br processes may be running. Under LOCAL, if a git operation replaces issues.jsonl (or the DB) between the moment a writer takes .write.lock and the moment it renames its temp file into place, the still-held lock describes a file that no longer exists. Two writers can each believe they hold the exclusive lock and each publish a different generation, so the last rename wins and the other writer's work is lost with no error. DWS converts that exact race into a SyncConflict error. LOCAL users get a lost-update with a success exit code; DWS users get a diagnosable failure.

**Local evidence**

> rg -c 'DatabaseFamilyWriteLock|JsonlFamilyWriteLock|DatabaseOpenerLease|canonical_database_authority_path|verify_jsonl_authority|verify_database_authority|verify_route|authority_paths_equivalent|PinnedJsonlName|canonical_database_authority_key|F_OFD_SETLK|SyncConflict-variant' across /Users/tranquangdang21/Projects/beads_rust/src and tests -> 0 hits for every one of DatabaseFamilyWriteLock, JsonlFamilyWriteLock, DatabaseOpenerLease, canonical_database_authority_path, verify_jsonl_authority, verify_database_authority, verify_route, authority_paths_equivalent, PinnedJsonlName, canonical_database_authority_key, F_OFD_SETLK. (BeadsError::SyncConflict does exist locally, 12 hits, but is used for unrelated conflict-marker errors.) The entire local write-authority surface is src/sync/mod.rs:58-123 blocking_write_lock (plain File::try_lock on .write.lock) and src/sync/mod.rs:149-166 try_sync_lock (plain File::try_lock on .sync.lock). src/sync/path.rs validates symlink escape (src/sync/path.rs:74-75 PathValidation::SymlinkEscape, :152-169, :390-403) but nothing re-checks the route while a mutation is in flight.

**Upstream evidence**

> DWS src/sync/mod.rs:440-457 `pub struct DatabaseFamilyWriteLock` with doc 'Composite advisory authority for every mutation of one database family. The workspace lock preserves existing single-workspace serialization, the canonical-path sidecar serializes independent workspaces before a database exists or across atomic replacement, and the database-inode lock unifies hard-link aliases once the file exists.' src/sync/mod.rs:460-479 `pub struct JsonlFamilyWriteLock` with doc 'Unlike a SQLite database, JSONL export intentionally renames a fresh inode over the destination. Locking the destination inode would therefore become stale at every successful flush.' Re-verification at src/sync/mod.rs:521-540 verify_jsonl_authority ('Canonical JSONL path changed while its write authority was held'), src/sync/mod.rs:1227 verify_database_authority, src/sync/mod.rs:1358 canonical_database_authority_path, src/sync/mod.rs:1453-1475 DatabaseOpenerLease / DatabaseOpenerExclusiveHold, src/sync/mod.rs:1651 blocking_jsonl_family_write_lock_with_timeout, src/sync/mod.rs:1723 blocking_database_family_write_lock_with_timeout. Inode lock rationale and the macOS/Windows breakage of the old approach: src/sync/db_inode_lock.rs:1-70, offset constant at DATABASE_INODE_LOCK_OFFSET chosen away from SQLite's 0x4000_0000..0x4000_0200 range.

</details>

<details><summary><b>Persistent memories (`br remember`) never travel through LOCAL's JSONL sync; GO round-trips them as `_type: "memory"` records</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> LOCAL ported `br remember/memories/recall/forget` and stores memories in the SQLite config table under `kv.memory.<key>` — deliberately the same keyspace GO uses. But LOCAL's JSONL export emits issue rows only, and its import only parses issue rows, so memories are per-clone SQLite state that never crosses the git boundary. GO's export writes a `_type: "memory"` record per key (sorted for determinism) when memories are enabled, and `bd import` writes them back into the same config prefix. The result is that a LOCAL clone and a GO clone sharing one issues.jsonl disagree about the memory set, and two LOCAL clones sharing one repo silently diverge.

**Impact**

> Agent memory is one of the artifacts br is specifically built to carry between machines, and in LOCAL it is the one artifact that cannot travel. A team that commits `.beads/issues.jsonl` gets issues shared but every `br remember` written on one machine stays invisible on all others, with no warning and no way to move it through the supported sync path except a manual SQL dump.

**Local evidence**

> src/cli/commands/memory.rs:13 `const MEMORY_KEY_PREFIX: &str = "kv.memory.";` with module doc at :3-5 'Memories are stored in the config table with `kv.memory.<key>` prefix, matching Go bd's kvkeys.MemoryConfigKeyPrefix pattern.' But: `rg -n 'memory|kv\.memory' src/sync/mod.rs src/cli/commands/export.rs src/cli/commands/sync.rs` filtered to exclude SqliteStorage::open_memory()/memory() -> 0 hits. LOCAL's ExportConfig/ExportResult/import_from_jsonl (src/sync/mod.rs:312-529, :4568-4660) have no memory concept; src/cli/commands/export.rs iterates `storage.list_issues(&filters)` and writes one line per Issue. LOCAL's own docs/JSONL_COMPATIBILITY.md row for `waiters` shows the same class of loss was previously catalogued, but there is no row for memories.

**Upstream evidence**

> GO cmd/bd/export_auto.go:802-833 writes the memory block: `if includeMemories { ... fullPrefix := kvPrefix + memoryPrefix ... sort.Strings(memKeys) ... record := map[string]string{"_type": "memory", "key": userKey, "value": v} ... memoryCount++ }` (comment at :802 area; sort rationale 'for deterministic output order (GH#3474)'). Same in the explicit export path at cmd/bd/export.go:291. Import side at cmd/bd/import.go:357-366: `for _, mem := range memories { storageKey := kvPrefix + memoryPrefix + mem.Key; ... store.SetConfig(ctx, storageKey, mem.Value); result.Memories++ }`, and dry-run reports `result.Memories = len(memories)`. Flag: cmd/bd/export.go:71 `exportCmd.Flags().BoolVar(&exportIncludeMemories, "include-memories", false, "Include persistent memories (from 'bd remember') in the export")`.

</details>

<details><summary><b>DWS federation sync is a real pull-then-push against Dolt remotes; LOCAL's federation remote is a local file path only</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** large

**Description**

> Beyond the destructive ordering already reported separately, LOCAL's federation has no transport. `federation add` accepts and stores an https:// remote_url (its own help says 'Remote URL (e.g., https://beads.example.com)'), but `federation sync` rejects every non-file scheme at runtime, so the accepted configuration is unusable and only a co-located filesystem path or file:// URL works. GO's federation sync drives a real store-level Sync against named peers, with a config.yaml-configured remote, username/password, sovereignty tiers, and an explicit ours/theirs conflict strategy or a conflict report with no strategy. GO's top-level `bd sync` is a second, independent federation loop with bounded push-race retries, positive merge-conflict detection, and machine-branchable exit codes 0/1/2/3/4.

**Impact**

> In LOCAL, `br federation` can only reconcile a LOCAL clone against a JSONL file that already sits on the same filesystem, and it does so destructively (see the separate federation finding). There is no supported path to sync two machines, and the config surface (username, password, sovereignty tier) records intent that the sync path silently ignores rather than reporting as unsupported.

**Local evidence**

> src/cli/commands/federation.rs:377-381 rejects any remote_url containing '://' that does not start with file://, returning 'unsupported protocol in remote_url: {}; use file:// or a bare path'. The add path at :40-56 stores remote_url verbatim (and even advertises https in the doc comment at :44), and the table schema at :559-570 has username/password_encrypted/sovereignty/last_sync columns that the sync path never uses. rg -c 'dolthub|dolt_remote|git\+|ssh://' src/cli/commands/federation.rs -> 0. LOCAL has no equivalent of a Dolt-style remote at all: its only remote-ish commands are federation and the read-only git probe in `sync --status` (src/cli/commands/sync.rs:94-142, deliberately read-only).

**Upstream evidence**

> GO cmd/bd/federation.go:198 `result, err := ds.Sync(ctx, peer, federationStrategy)` with peer from `--peer` or all configured peers (help at :38-52), credentialed (federationUser/federationPassword at cmd/bd/federation.go:20-21) and strategy-validated at :168-169. Main `bd sync`: cmd/bd/sync.go:650-713 documents the four-step loop and the exit-code contract ('0 synced / 1 error / 2 merge conflict — halted / 3 retries exhausted / 4 the dirty working set is stuck'), flags at :716-719 (`--remote`, `--attempts`, `--yes` to adopt a Dolt remote derived from git origin, `--no-adopt`), remote resolution in cmd/bd/sync_remote.go:14-30 (sync.remote, then deprecated sync.git-remote) and credential redaction at :90-120.

</details>

<details><summary><b>GO export record-type scoping (--all, --include-infra, --scrub, --include-memories, --exclude-owner) absent; LOCAL export filters issue rows only</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> LOCAL's `br export` takes only row-level issue filters (status/type/assignee/owner/pinned/mol_type/id) and always exports every issue record in scope. GO's `bd export` additionally scopes *which record classes* reach the git-tracked JSONL: --all (infra, templates, gates, memories), --include-infra (infrastructure beads such as agents/roles/messages), --scrub (drop test/pollution records), --include-memories, and --exclude-owner (repeatable, also read from export.exclude_owners config) which drops issues created by a given identity. The last one is the materially different capability: it lets a per-agent or per-contractor identity keep its issues out of a shared committed JSONL, and GO re-applies the same exclusion on the incremental export path so a config-excluded owner's later edit cannot leak into the committed file.

**Impact**

> Every issue br stores lands in `.beads/issues.jsonl`, which is by design committed and pushed. LOCAL gives an operator no supported way to keep one identity's issues, its test fixtures, or its infrastructure beads out of that shared file — only hand-editing after the fact, which the next auto-flush reverts. GO also closes the incremental-export leak that a naive port would reintroduce.

**Local evidence**

> rg -c 'include_infra|include-infra|exclude_owner|exclude-owner|include_memories|include-memories' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits each. `rg -c 'retention-days'` -> 0, 'retention_days' -> 43 (that one is the tombstone-retention knob in sync/mod.rs, unrelated to export scoping). LOCAL ExportFilterArgs at src/cli/mod.rs:2230-2260+ is entirely row-selection; note its `all: bool` means 'all statuses', not GO's 'all record types'. src/cli/commands/export.rs builds ExportConfig at the JSONL branch with a fixed record set (no infra/template/scrub/owner gates) and writes `for issue in &issues { serde_json::to_string(issue) }` at :124-133. LOCAL's docs/BD_VS_BR.md and docs/JSONL_COMPATIBILITY.md list no record-scoping flag.

**Upstream evidence**

> GO cmd/bd/export.go:67-75: `--all` 'Include all records (infra, templates, gates, memories)'; `--include-infra` 'Include infrastructure beads (agents, roles, messages)'; `--scrub` 'Exclude test/pollution records'; `--include-memories` 'Include persistent memories (from bd remember) in the export'; `--exclude-owner` 'Exclude issues created by this identity (repeatable; also reads export.exclude_owners config)'; `--verbose` 'Print filtered issue count when owners are excluded'. Full-export filter at cmd/bd/export_auto.go:679 buildAutoExportFilter / :717 exportToFile. Incremental path re-applies the owner exclusion as an explicit safety net at cmd/bd/export_auto.go:1280-1284 ('Owner-exclusion safety net, mirroring exportToFile's: without this, a config-excluded owner's issue that changes would leak into the git-committed JSONL via the incremental path even though the full-export path always excludes it (be-shbed)'). Tests: cmd/bd/export_exclude_owner_test.go, cmd/bd/export_source_wisp_subset_test.go.

</details>

<details><summary><b>GO's `bd import --dry-run` classification preview and `--dedup` title-dedup have no LOCAL equivalent</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** medium

**Description**

> GO's import has two flags LOCAL's import has no analogue for. `--dry-run` parses the input, applies dedup, then classifies every row against the live store and renders a plan (created/updated/skipped counts, memories, source) without mutating anything; LOCAL's import always writes. `--dedup` is a distinct dedup axis from the content-hash dedup both tools share: it drops incoming lines whose *title* matches an existing open issue, which is what stops a bulk paste of re-filed tickets from duplicating the backlog. LOCAL's ImportArgs carries input/format/rename_prefix/force/strict and nothing else.

**Impact**

> An operator restoring a snapshot or importing a vendor's issue list with `br import` has no way to see what will change before committing to it, and no way to suppress title-level duplicates. The only previews LOCAL offers are `--strict` (abort on first failure) and the sync-level `--rebuild` (which previews nothing and deletes DB rows). Combined with `br import` auto-flushing to JSONL at the end (src/cli/commands/import.rs `storage_ctx.flush_no_db_if_dirty()?`), a bad import is written to the git-tracked JSONL before anyone can inspect it.

**Local evidence**

> LOCAL ImportArgs at src/cli/mod.rs:1629-1649 has exactly five fields: input (:1632), format (:1636), rename_prefix (:1640), force (:1644), strict (:1648). rg -n 'dry_run|dry-run' /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/import.rs -> 0 hits. rg -n 'dedup' /Users/tranquangdang21/Projects/beads_rust/src/cli/ -> only Vec::dedup() calls in unrelated commands (src/cli/commands/delete.rs:774, orphans.rs:454, config.rs:1260, graph.rs:795, blocked.rs:171) and two doctor doc comments — no import-title-dedup flag or filter anywhere.

**Upstream evidence**

> GO cmd/bd/import.go:113-116: `importCmd.Flags().BoolVar(&importDryRun, "dry-run", false, "Show what would be imported without importing")`; `importCmd.Flags().BoolVar(&importDedup, "dedup", false, "Skip lines whose title matches an existing open issue")`; `importCmd.Flags().BoolVar(&importAllowStale, "allow-stale", false, "Import rows even when older than the local issue (required to restore an older snapshot)")`. Implementation at cmd/bd/import.go:336-356: dedup runs first (`issues, dedupHits = filterDuplicatesByTitle(ctx, store, issues)`), then `if importDryRun { result.Skipped = dedupHits; classification, err := classifyDryRunImport(ctx, store, issues, importAllowStale); ... return renderImportDryRun(result, len(memories), source, dedupHits) }` — no store mutation. Also cmd/bd/import_proxied_server.go:55 for the server path.

</details>

<details><summary><b>DWS freezes one tombstone-retention cutoff per export; LOCAL re-evaluates age per issue against the wall clock</b></summary>

- **Severity:** low · **Present in:** dws-only · **Effort:** small

**Description**

> With `--retention-days N`, LOCAL decides tombstone expiry independently for each issue at the moment that issue is serialized, using `Issue::is_expired_tombstone(retention_days)` — now minus deleted_at vs N days. Because LOCAL's export is parallel (up to 64 workers via --export-parallelism) and a large export can take seconds, a tombstone that falls due mid-run can be included in one worker's output and excluded by another's, producing a JSONL whose tombstone set depends on scheduling rather than on the data. DWS captures a single `export_as_of` timestamp for the whole logical export and evaluates every issue against that frozen value, so the same export is deterministic regardless of duration or worker count.

**Impact**

> Narrow and only bites at the exact retention boundary, but it is a real determinism defect in the artifact that gets committed: two runs of `br sync --flush-only --retention-days 30` on an unchanged database can produce different JSONL, and the difference is invisible in the reported counts. It also means the exported tombstone set cannot be reproduced from the export's own inputs.

**Local evidence**

> src/sync/mod.rs:1915-1916, :2218-2220, :2273-2275 and :3050 all call `issue.is_expired_tombstone(retention_days)` with no as-of argument; the function is src/model/mod.rs:1313-1333 and computes from `deleted_at` against the current time. The export driver is parallel: src/sync/mod.rs:2192-2275 prepares entries across workers (`max_parallel_workers`, default cap 64 at src/sync/mod.rs:48, and the --export-parallelism flag at src/cli/mod.rs:3397-3403 'Used by --flush-only and merge export writeback'). There is no frozen-clock parameter anywhere in LOCAL's ExportConfig (src/sync/mod.rs:312-340).

**Upstream evidence**

> DWS src/sync/mod.rs:3310-3312 ExportConfig carries 'Retention period for tombstones in days' plus a 'Frozen tombstone-retention cutoff for this logical export'; every call site uses the as-of form — :11057 `issue.is_expired_tombstone_at(retention_days, export_as_of.to_owned())`, :11058, :11514, :11570, :11761, :11786.

</details>

<details><summary><b>DWS emits a hash-bound export publication receipt with a retained recovery path; LOCAL records only a chunked witness</b></summary>

- **Severity:** low · **Present in:** dws-only · **Effort:** medium

**Description**

> After a flush, LOCAL computes an observed_jsonl_witness and stores it, which is enough to detect that the file changed out from under it on a later probe, and LOCAL does use durable_rename (temp file + rename + parent-directory fsync) for power-loss durability, so raw atomicity is at parity. What LOCAL does not produce is a publication receipt: DWS captures the source snapshot before the write, re-reads the persisted bytes afterwards, fails the export if the persisted generation does not match the one it planned against, and returns an ExportPublicationReceipt carrying content_sha256, output_path and retained_recovery_path under an explicit ExportPublicationAtomicity classification. That receipt is the artifact an operator or a downstream reviewer can cite to prove which bytes were published.

**Impact**

> Lower severity than the items above because the write itself is already atomic and crash-safe in LOCAL. The concrete loss is auditability: after `br sync --flush-only`, LOCAL's JSON output reports counts but carries no digest of the bytes it published, so an operator cannot verify that the file now on disk is the file br intended to write, and there is no recorded recovery copy to point at.

**Local evidence**

> rg -c 'ExportPublicationReceipt|ExportPublicationAtomicity|JsonlSourceStateWitness|retained_recovery_path' /Users/tranquangdang21/Projects/beads_rust/src -> 0 hits. LOCAL's export-side verification is the witness path only: src/sync/mod.rs:2708 `fn observed_jsonl_witness(jsonl_path: &Path) -> Result<JsonlWitness>`, called at :2620, :2704, :2863, :3069 and recorded via :2722 record_observed_jsonl_witness_in_tx (:2883, :4650). Atomicity itself IS at parity — src/sync/mod.rs:2339-2340 '// Atomic rename plus parent-directory fsync for power-loss durability' followed by `crate::util::durable_rename(&temp_path, output_path)?`, also at :3240 and :5217; the export temp file is fsync'd at :2316.

**Upstream evidence**

> DWS src/sync/mod.rs:3512 `pub enum ExportPublicationAtomicity`, :3549-3588 `pub struct ExportPublicationReceipt` with `pub fn content_sha256(&self) -> &str`, `pub fn output_path(&self) -> &str`, `pub fn retained_recovery_path(&self) -> Option<&str>`; source-vs-persisted verification at :2935-2967 ('persisted_source.state_witness() != staged_source.state_witness() || persisted_source.content_sha256() != content_sha256') and the guard at :3625-3636 ('JSONL export result has no verified persisted source generation to finalize' / '...to retain'); staged-bytes check at :12874-12984 ('JSONL bytes do not match the incremental auto-flush hash'); JsonlSourceStateWitness at :140-170.

</details>

### C — `testing-parity-docs` (8 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | high | both-upstreams | `missing_test` | CI runs zero integration tests: `cargo test --lib` excludes all 155 test files / 105,934 LOC under tests/ |
| — | medium | both-upstreams | `divergent_behavior` | LOCAL's own ci.yml/release.yml violate the repo's mandatory immutable action-pin policy — 0 of 27 action refs pinned, and the 937-line test that detects it never runs |
| — | high | dws-only | `missing_test` | No multi-process linearizability oracle for the concurrent write path |
| — | medium | both-upstreams | `missing_feature` | The br↔bd conformance suite (~18,900 LOC across 6 files) has no CI job — the parity claim is never machine-checked |
| — | medium | dws-only | `missing_test` | No model-based differential storage test — no engine-free oracle replays operations against SqliteStorage |
| — | low | both-upstreams | `missing_test` | No MCP protocol or shutdown e2e tests for the shipped `br serve` MCP server |
| — | low | go-only | `missing_feature` | No CI coverage measurement or upload gate — LOCAL owns a coverage config and script that no workflow runs |
| — | low | both-upstreams | `missing_test` | No documentation-contract tests — AGENTS.md structure, README command blocks and captured JSON doc examples are never executed or verified |

<details><summary><b>CI runs zero integration tests: `cargo test --lib` excludes all 155 test files / 105,934 LOC under tests/</b></summary>

- **Severity:** critical · **Present in:** both-upstreams · **Effort:** medium

**Description**

> `.github/workflows/ci.yml` is 37 lines and contains exactly two cargo invocations: `cargo fmt --all -- --check` (line 31) and `cargo test --lib --all-features` (line 35). The `--lib` selector restricts Cargo to the library unit-test target; every `tests/*.rs` integration binary is silently excluded. LOCAL has 155 test files totalling 105,934 lines, containing 1,830 `#[test]` functions — including tests/conformance.rs (13,599 lines), tests/e2e_concurrency.rs (3,007), all 20 proptest_*.rs files, all 25 repro_*.rs regression files, and tests/workflow_action_pins.rs (937). None of them execute on any push or PR. No other LOCAL workflow substitutes: `rg -n 'conformance.sh|e2e.sh|e2e_full.sh|coverage.sh|ci-local.sh|bench.sh' .github/workflows/` returns NO MATCHES. DWS shards all 188 test files across 5 CI matrix jobs via a filename-derived shard script whose cargo invocation is `cargo test --locked --all-features` plus per-binary `--test <name>` args. GO runs 1,679 `_test.go` files / 9,417 Test funcs in a matrix job with `-race -short -coverprofile`.

**Impact**

> LOCAL's entire integration suite — every e2e, storage, conformance, property and regression test — is dead code from CI's perspective. A regression that breaks a test in `tests/` cannot turn CI red, so it reaches `main` and a tagged release unnoticed. The `tests/` tree is 105K LOC of investment that currently buys nothing on the merge path.

**Local evidence**

> `find tests -maxdepth 1 -name '*.rs' | wc -l` = 155; `wc -l tests/*.rs | tail -1` = 105,934 total; `rg -c '#\[test\]' tests/*.rs` summed = 1,830. `cat -n .github/workflows/ci.yml` = 37 lines; only cargo lines are `31: cargo fmt --all -- --check` and `35: cargo test --lib --all-features`. `rg -n 'conformance.sh|e2e.sh|coverage.sh|ci-local.sh' .github/workflows/` -> no matches. `grep -cE 'cargo (clippy|test --test|test --all|test --tests)' .github/workflows/*.yml` -> 0 for all three workflows.

**Upstream evidence**

> DWS: `.github/workflows/ci.yml:91-106` — `test:` job with `strategy.matrix.shard: [lib, e2e-a-l, e2e-m-z, storage, misc]`, step `run: scripts/test-shard.sh ${{ matrix.shard }}`; `scripts/test-shard.sh:26` `CARGO_TEST=(cargo test --locked --all-features)`, lines 57-68 loop every `tests/*.rs` in the shard and emit one `--test <name>` per binary; the script's own comment (lines 2-4) states 'so a new tests/*.rs file always lands in exactly one shard'. GO: `.github/workflows/main.yml:468-482` test job, `test-flags: '-v -race -short -coverprofile=coverage.out'`.

</details>

<details><summary><b>LOCAL's own ci.yml/release.yml violate the repo's mandatory immutable action-pin policy — 0 of 27 action refs pinned, and the 937-line test that detects it never runs</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** small

**Description**

> LOCAL built a complete supply-chain pinning apparatus: `.github/action-pins.jsonl` (7.3K immutable-SHA inventory), `.github/action-pin-upstreams.jsonl` (3.1K), `docs/CI_SUPPLY_CHAIN.md` (143 lines declaring the policy canonical), `tests/workflow_action_pins.rs` (937 lines) whose `verify_action_pins` rejects any ref that is "not pinned to a 40-character SHA" and whose top-level test is literally `repository_workflow_action_pins_are_inventory_backed`, plus `scripts/audit-workflow-action-pins.sh` and `scripts/verify-workflow-action-pins.sh`. The workflows themselves violate that policy. `audit.yml` is fully pinned (3 pinned / 0 mutable), but `ci.yml` is 0 pinned / 3 mutable and `release.yml` is 0 pinned / 24 mutable. Only `.github/workflows/audit.yml` was ever brought into compliance. Because `workflow_action_pins.rs` lives in `tests/`, finding #1 means the detector is never executed.

**Impact**

> A third-party action tag can be moved between LOCAL's CI/release runs — for `softprops/action-gh-release`, which runs with `permissions: contents: write` (release.yml:9), that is a release-credential supply-chain exposure. The repo has the policy, the inventory, the verifier and the regression test; the workflows were simply never updated, and the test that would have caught it is not on the CI path.

**Local evidence**

> Pin/mutable census run per workflow: `for f in .github/workflows/*.yml; do echo "$f: pinned=$(grep -cE 'uses: [^ ]+@[0-9a-f]{40}' $f) mutable=$(grep -cE 'uses: [^ ]+@(v[0-9]|main|master)' $f)"; done` -> `audit.yml: pinned=3 mutable=0`, `ci.yml: pinned=0 mutable=3`, `release.yml: pinned=0 mutable=24`. Concrete violations: ci.yml:23 `actions/checkout@v4`, :27 `Swatinem/rust-cache@v2`, :28 `actions/setup-node@v4`; release.yml:19,47,74,99,124,153 `actions/checkout@v4`; release.yml:180 `softprops/action-gh-release@v2`; release.yml:35,63,88,113,142,192 `actions/upload-artifact@v4`; release.yml:155 `actions/download-artifact@v4`. `ls -la .github/` shows both `action-pins.jsonl` (7.3K) and `action-pin-upstreams.jsonl` (3.1K) present. `wc -l docs/CI_SUPPLY_CHAIN.md` = 143. `tests/workflow_action_pins.rs:97` `require_error_contains(&errors, "not pinned to a 40-character SHA")`.

**Upstream evidence**

> DWS `.github/workflows/ci.yml:38` `- uses: actions/checkout@3d3c42e5aac5ba805827da76410c181273ba90b1  # v7.0.1` (40-char SHA with version comment); same pattern at :94, :97, :228 and throughout all 8 DWS workflows. DWS `.github/workflows/conformance.yml:34,37,42,45,103,111` all SHA-pinned. GO `.github/workflows/main.yml:25,30` and `conformance.yml:25` likewise SHA-pinned.

</details>

<details><summary><b>No multi-process linearizability oracle for the concurrent write path</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** large

**Description**

> LOCAL's largest concurrency test, tests/e2e_concurrency.rs, is 3,007 lines — comparable in size to DWS's 4,074 — but it asserts behavioural invariants (no corruption, exit codes, workspace integrity) without an oracle that proves the observed history is *equivalent to some legal serial execution*. DWS adds tests/linearizability_multiprocess.rs (2,927 lines), which is a different kind of test: 8 worker streams each drive their own `br` process history against one workspace for a fixed budget, recording every invocation as `{pid, invoke, return, op, outcome}`; because every operation touches a single issue and dependency edges only point into a fixed sink set, histories partition by issue id, and each partition is checked with a Wing & Gong search for a linearization that respects real-time process-call order against a sequential model. It then requires `PRAGMA integrity_check` ok, dense rowids, and that the JSONL published by `br sync --flush-only` equals the linearized final state. It runs as a named CI gate, not just a test.

**Impact**

> LOCAL can ship a write-lock or flush-ordering bug that produces a history no serial execution could have produced — e.g. a lost update across the workspace `.write.lock` family, or a JSONL export that disagrees with SQLite after interleaved writers — and no test will fail. Behavioural concurrency tests pass on such bugs because each individual invocation looks correct; only a sequential oracle catches the composition.

**Local evidence**

> `rg -ln 'lineariz|Wing.*Gong|sequential model|BTreeMap.*model' tests/ src/` -> 0 files. `rg -ln 'model-based|model based|reference model|projection mismatch' tests/` -> 0 files. LOCAL `find tests -maxdepth 1 -name '*.rs' -exec basename {}` contains no linearizability file; the only overlap is tests/e2e_concurrency.rs (3,007 lines, 104 KB) and tests/workspace_failure_replay.rs (999 lines), neither of which builds a sequential oracle.

**Upstream evidence**

> DWS `tests/linearizability_multiprocess.rs:1-35` (module doc): "N worker threads each drive their own stream of `br` processes against one workspace for a fixed wall-clock budget and record every invocation as a history entry `{pid, invoke, return, op, outcome}` ... each partition is checked with a Wing & Gong style search for a linearization that respects the real-time order of the process calls and the sequential model"; ":20-21" "The database must then pass `PRAGMA integrity_check`, its rowids must be dense, and the JSONL published by `br sync --flush-only` must match that final state"; ":22" `BR_LINEARIZABILITY_PROCESSES` default 8. Wired into CI at `.github/workflows/ci.yml:159-165` as the "Multi-process linearizability check" step of the `reliability-gates` job (`cargo test --test linearizability_multiprocess -- --nocapture`, env `BR_LINEARIZABILITY_ARTIFACT_DIR: target/test-artifacts/linearizability`), gated `if: github.event_name != 'push'`.

</details>

<details><summary><b>The br↔bd conformance suite (~18,900 LOC across 6 files) has no CI job — the parity claim is never machine-checked</b></summary>

- **Severity:** high · **Present in:** both-upstreams · **Effort:** small

**Description**

> LOCAL ships the machinery for Go-parity testing and never wires it into CI. `scripts/conformance.sh` (6.5K) drives a live `bd` binary through a real workspace, supports `CONFORMANCE_STRICT`, `CONFORMANCE_TIMEOUT`, emits `target/test-artifacts/conformance_summary.json`, and exits 2 when `bd` is unavailable. `tests/common/binary_discovery.rs` (11K) locates the reference binary. The test files themselves are substantial: conformance.rs 13,599 lines, conformance_labels_comments.rs 1,729, conformance_edge_cases.rs 1,464, conformance_workflows.rs 1,308, conformance_schema.rs 1,190, conformance_text_output.rs 896 — ~18,900 lines. All of it runs only when a developer invokes the script by hand. DWS gives the same suite a dedicated weekly + manual workflow that pins the Go reference to an immutable commit and deliberately refuses to report success when the oracle is missing. GO runs its own conformance suite on every PR and merge_group.

**Impact**

> Go/Rust output parity — one of LOCAL's headline design goals, asserted by AGENTS.md as "conformance tests validate this" — is verified only on demand. Output drift in text, JSON or TOON rendering reaches a release without any gate noticing. `tests/conformance*.rs` are also excluded from CI by finding #1, so both the static and dynamic halves of the parity claim are dark.

**Local evidence**

> `rg -n 'conformance.sh|e2e.sh|e2e_full.sh|coverage.sh|ci-local.sh|bench.sh' .github/workflows/` -> NO MATCHES. `ls -1 .github/workflows/` -> only `audit.yml`, `ci.yml`, `release.yml`. `wc -l scripts/conformance.sh` = 6.5K bytes; its header comment (lines 20-22) documents `BD_BINARY=/path/to/bd  # Override bd binary location (required)` and line 26 `Exit code: 0 on success, 1 on failure, 2 on bd unavailable`. `wc -l tests/conformance*.rs` -> 13599 + 1729 + 1464 + 1308 + 1190 + 896 = 19,186 lines.

**Upstream evidence**

> DWS `.github/workflows/conformance.yml` (115 lines) — `on: workflow_dispatch` (with a `strict_mode` boolean input) + `schedule: cron '0 6 * * 1'`; lines 63-78 build the Go reference and verify it: `BD_REF: v0.46.0`, `BD_COMMIT: 812f4e529795df94b680912c996ac9a2ea8e39c6` with the comment at :60-62 "This step must not be `continue-on-error`: if the reference binary is unavailable the parity gate did not run, and reporting success would be false assurance"; line 94 `run: scripts/conformance.sh "${ARGS[@]}"`; lines 101-115 upload `conformance_summary.json` and per-test artifacts. GO `.github/workflows/conformance.yml:6-7` `on: pull_request: branches: [main, 'release/**']` + `merge_group`, line 32 `./scripts/conformance.sh`.

</details>

<details><summary><b>No model-based differential storage test — no engine-free oracle replays operations against SqliteStorage</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** large

**Description**

> LOCAL's property tests all exercise data-shape round-trips (JSONL round-trip, merge determinism, validation rules, ID generation, time parsing, status partitioning) — none drive the storage engine's mutation surface. DWS adds tests/model_based_storage.rs (1,539 lines), which replays randomly generated sequences of issue operations against a file-backed `SqliteStorage` AND against a tiny engine-free `BTreeMap` reference model, then after every operation asserts that what storage projects (issue fields, labels, comments, dependency edges, the listing) equals what the model says; readiness and annotated-blocker witnesses are checked against independent graph rules including typed edges and hierarchy; deliberate invalid dependencies and predicted cycles must fail without mutating issue data, events or dirty tracking; and `PRAGMA integrity_check` must be `ok` at the end of every case. Its module doc names the specific bug classes it was built to catch: GitHub #457/#460/#461 corruption and the #426 rowid-order bug found by 264 sequential dependency removals.

**Impact**

> Storage-engine corruption — the exact class DWS needed this test to find — has no systematic detector in LOCAL. LOCAL's `#[cfg(test)]` unit tests and `tests/storage_*.rs` assert expected values written alongside the implementation, so they share its assumptions; an independent model is what breaks that circularity.

**Local evidence**

> `rg -ln 'model-based|model based|reference model|projection mismatch' tests/` -> 0 files. `find tests -maxdepth 1 -name '*.rs' -exec basename {} | sort` shows LOCAL's 20 property files are all shape-level: proptest_jsonl_roundtrip.rs (675), proptest_merge.rs (555), proptest_validation.rs (501), proptest_hash.rs (471), proptest_model_roundtrip.rs (365), proptest_time.rs (354), proptest_id.rs (288), proptest_status_partition.rs (255), proptest_parent_child.rs (178), proptest_claim_exclusion.rs (159), proptest_sync_path.rs (121). `rg -ln 'BTreeMap.*model' tests/ src/` -> 0.

**Upstream evidence**

> DWS `tests/model_based_storage.rs:1-20` (module doc): "replays random sequences of issue operations against a file-backed `SqliteStorage` and against a tiny engine-free reference model (`BTreeMap`s), and after every operation checks that what the storage projects (issue fields, labels, comments, dependency edges, the listing) equals what the model says ... The August 2026 corruption class (GitHub #457/#460/#461) and the GitHub #426 rowid-order bug (264 sequential dependency removals) would both have produced a projection mismatch here. `PRAGMA integrity_check` must be `ok` at the end of every case." `:21-23` "Readiness and annotated blocker witnesses are checked against independent graph rules, including typed edges and hierarchy ... Indices resolve against live issues so shrinking preserves meaningful operations. Set `BR_MODEL_CASES` to run more cases than the default." Wired into the DWS `storage` shard: `scripts/test-shard.sh:32` includes `linearizability_multiprocess.rs` in the storage shard glob.

</details>

<details><summary><b>No MCP protocol or shutdown e2e tests for the shipped `br serve` MCP server</b></summary>

- **Severity:** high · **Present in:** dws-only · **Effort:** medium

**Description**

> LOCAL ships a full MCP server surface — `src/mcp/mod.rs`, `tools.rs`, `resources.rs`, `prompts.rs`, wired as the optional `mcp` feature (`Cargo.toml:110-111` `fastmcp-rust = { version = "0.3.1", optional = true }`, `Cargo.toml:166` `mcp = ["dep:fastmcp-rust"]`) and documented in AGENTS.md as exposing 7 tools, 12 resources and 4 prompts over stdio. LOCAL has zero MCP test files. `find tests -name '*mcp*'` returns only benchmark artifact directories under `tests/artifacts/perf/`, not test sources; `rg -l 'mcp' tests/` matches only tests/e2e_version.rs (incidental). The consequence is unusual for LOCAL specifically: its CI command is `cargo test --lib --all-features`, which is the one place the `mcp` feature *would* be compiled — but there is no `#[cfg(test)]` MCP protocol test inside `src/mcp/` to compile, and no integration target to run. DWS covers the wire protocol, tool dispatch, resource reads, prompt rendering and clean stdio shutdown.

**Impact**

> An advertised agent integration (7 tools, 12 resources, 4 prompts) has no test that speaks JSON-RPC to it. Schema drift, an unhandled tool error shape, a resource that returns malformed content, or a process that fails to flush and exit on stdin close all reach users undetected.

**Local evidence**

> `find src -ipath '*mcp*' -maxdepth 3` -> `src/mcp/prompts.rs`, `src/mcp/tools.rs`, `src/mcp/mod.rs`, `src/mcp/resources.rs`. `find tests -name '*mcp*'` -> only `tests/artifacts/perf/beads-perf-20260504T-mcp-{update,create,show,list,close,manage}-issue-batch` and `...-mcp-read-snapshot` (artifact dirs, not tests). `rg -l 'mcp' tests/` -> `tests/e2e_version.rs` only. `rg -n 'mcp' Cargo.toml` -> `:110 # MCP server (optional; gated behind the mcp feature)`, `:111 fastmcp-rust = { version = "0.3.1", optional = true }`, `:166 mcp = ["dep:fastmcp-rust"]`.

**Upstream evidence**

> DWS `tests/e2e_mcp_protocol.rs` (3,069 lines, 111 KB) and `tests/e2e_mcp_shutdown.rs` (184 lines, 6.0 KB) — both absent from LOCAL. Confirmed via `comm -13 /tmp/_local_tests.txt /tmp/_dws_tests.txt`, which lists `e2e_mcp_protocol.rs` and `e2e_mcp_shutdown.rs` among the 33 DWS-only test files.

</details>

<details><summary><b>No CI coverage measurement or upload gate — LOCAL owns a coverage config and script that no workflow runs</b></summary>

- **Severity:** medium · **Present in:** go-only · **Effort:** small

**Description**

> LOCAL ships `tarpaulin.toml` (1.0K) and `scripts/coverage.sh` (2.5K) but no workflow invokes either, so no coverage number is ever produced, stored, or compared. GO runs its full test suite with `-coverprofile=coverage.out` and uploads to Codecov on every PR. Note the scope boundary honestly: DWS is *also* missing a coverage upload — `rg -n 'coverage|tarpaulin|codecov' DWS/.github/workflows/*.yml` finds only two prose mentions of 'coverage_scope' in benchmark comparison JSON — so this is a GO-only gap, and DWS compensates with named reliability-gate jobs instead of a coverage number.

**Impact**

> Untested code accumulates invisibly. With the integration suite not running at all (finding #1), coverage is the one signal that could have revealed it, and it is not being collected. Downstream it means a contributor cannot see whether new code is exercised.

**Local evidence**

> `ls -la tarpaulin.toml` -> present, 1.0K. `ls -1 scripts/` includes `coverage.sh` (2.5K). `rg -n 'conformance.sh|e2e.sh|coverage.sh|ci-local.sh|bench.sh' .github/workflows/` -> NO MATCHES, so coverage.sh is unreferenced. `ls -1 .github/workflows/` -> audit.yml, ci.yml, release.yml only.

**Upstream evidence**

> GO `.github/workflows/main.yml:482` `test-flags: '-v -race -short -coverprofile=coverage.out'`; `:567` step "Test (with coverage + JUnit XML)"; `:587` and `:596` `uses: codecov/codecov-action@fb8b3582c8e4def4969c97caa2f19720cb33a72f # v6`; also `codecov.yml` at repo root and `.github/workflows/nightly.yml:99` codecov upload.

</details>

<details><summary><b>No documentation-contract tests — AGENTS.md structure, README command blocks and captured JSON doc examples are never executed or verified</b></summary>

- **Severity:** medium · **Present in:** dws-only · **Effort:** medium

**Description**

> LOCAL has a 42K AGENTS.md, a 34K README.md and 36 markdown files under `docs/`, plus a `<!-- from: ... -->` captured-output convention available in the docs format — and nothing checks any of it. DWS's tests/agents_md_contract.rs (280 lines) asserts that every path in AGENTS.md's Project Structure tree exists, that every direct child of `src/` appears in that tree, and that every crate in the Key Dependencies table is a real dependency; its doc comment records that a prior reality check found AGENTS.md "listing a `storage/queries/` directory that does not exist, omitting eleven top-level modules, and naming a lint level the crate does not use." tests/docs_examples.rs (419 lines) runs every `<!-- from: br ... -->` JSON block in a scratch workspace and asserts the live output's key-path structure equals the documented block. tests/e2e_readme_examples.rs (483 lines) gives every README ```bash block its own initialized workspace and runs each `br` command, failing with the README line number on a mismatch. LOCAL has none of the three.

**Impact**

> AGENTS.md is the first document an agent reads and the map LOCAL relies on for routing. It can drift silently — as it demonstrably did upstream. Command examples in README/docs can reference renamed subcommands or removed flags and stay wrong indefinitely. For an agent-first tool whose documentation is a primary interface, this is a correctness surface, not cosmetics.

**Local evidence**

> `rg -l 'AGENTS\.md|README\.md' tests/` -> only incidental hits: `tests/e2e_sync_git_safety.rs`, `tests/conformance_schema.rs`, `tests/e2e_routing.rs`, `tests/e2e_git_safety_full_cli.rs`, `tests/e2e_doctor_chokepoint.rs`, `tests/snapshots/snapshots/snapshots__snapshots__cli_output__{help,doctor}_output.snap`, and 8 fixture READMEs under `tests/doctor_fixtures/`. `rg -n 'read_to_string.*(AGENTS|README)|include_str!.*(AGENTS|README)' tests/ src/` -> only `src/cli/commands/agents.rs:2023,2054` (the `br agents` command reading a user's AGENTS.md at runtime, not a test). `rg -c '<!-- from:' docs/` -> 0 files. `comm -13 local_docs dws_docs` lists `agents_md_contract.rs`, `docs_examples.rs`, `e2e_readme_examples.rs` as DWS-only.

**Upstream evidence**

> DWS `tests/agents_md_contract.rs:1-9` — "every path in the Project Structure tree exists; every direct child of `src/` (files and directories, hidden files excluded) appears in that tree; every crate named in the Key Dependencies table is a real dependency." DWS `tests/docs_examples.rs:1-18` — "This test finds every such marker, runs the command in a scratch workspace, and asserts that the JSON *key structure* ... of the live output equals the documented block ... renaming or dropping a key fails." DWS `tests/e2e_readme_examples.rs:1-16` — "Every `br` command in README.md's ```bash blocks runs in a scratch workspace and must exit as documented ... A command whose exit code differs from the documented expectation fails the test with the README line number." DWS also carries `scripts/verify-agent-contracts.sh` (954 bytes), absent from LOCAL's `scripts/`. `rg -c '<!-- from:' DWS/docs/` -> matches in `docs/README.md` and `docs/ARCHITECTURE.md`.

</details>

### C — `triage-analytics` (2 findings)

| # | Severity | Present in | Type | Finding |
|---|---|---|---|---|
| — | info | go-only | `missing_flag` | No open-issues-only filter on graph output (bd graph --open missing) |
| — | low | dws-only | `missing_feature` | No WIP capacity occupancy metrics in br stats and no br capacity command |

<details><summary><b>No open-issues-only filter on graph output (bd graph --open missing)</b></summary>

- **Severity:** low · **Present in:** go-only · **Effort:** small

**Description**

> GO can render the graph with closed and deferred issues filtered out, which keeps the displayed dependency structure limited to currently-actionable work. LOCAL `br graph` has no status filter - a user must post-filter by hand.

**Impact**

> Minor: graph output over a mature backlog is noisier on LOCAL. Note `br ready` and `br blocked` already provide status-filtered lists, so the practical loss is limited to the graph view.

**Local evidence**

> GraphArgs at /Users/tranquangdang21/Projects/beads_rust/src/cli/mod.rs:3978-3991 = { issue, --all, --compact }; no status filter of any kind. `rg -n -i 'open|status' /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/graph.rs` -> no status-based graph filtering; graph.rs:260 graph_all consumes the unfiltered issue set.

**Upstream evidence**

> GO: cmd/bd/graph.go:362 `graphCmd.Flags().BoolVar(&graphOpen, "open", false, "Show only open issues (filters out closed/deferred), forces compact layer format")`; applied at graph.go:173 and :234-235 via :670 `func filterSubgraphOpen(subgraph *TemplateSubgraph) *TemplateSubgraph` and :655 `func isOpenStatus(s types.Status) bool`.

</details>

<details><summary><b>No WIP capacity occupancy metrics in br stats and no br capacity command</b></summary>

- **Severity:** low · **Present in:** dws-only · **Effort:** large

**Description**

> DWS adds a workflow-capacity (WIP limit) subsystem that surfaces an occupancy table inside `br stats` - per-capacity counted / aggregate / exempt / soft-limit / hard-limit / remaining columns plus a STATE column - and a `br capacity` subcommand (exempt/renew/revoke/exemptions) with an append-only audit history. LOCAL has no capacity concept at all: no CapacityCommands variant in its CLI enum, no capacity dispatch in main.rs, and no capacity table in stats output. LOCAL therefore cannot report queue saturation, which is the closest thing either Rust codebase has to a workload/throughput metric.

**Impact**

> LOCAL cannot show how close a queue is to its configured WIP ceiling, nor how many issues are exempt. This is a real stats-surface delta but it is a workflow-admission feature rather than graph analytics, and it is gated behind configuration LOCAL does not define - so practical impact is low unless the capacity feature is being adopted.

**Local evidence**

> `rg -ni 'capacity' /Users/tranquangdang21/Projects/beads_rust/src` -> 180 matches, ALL unrelated Vec/HashSet::with_capacity preallocations (e.g. src/close_policy.rs:606, src/sync/mod.rs:807,1411,1543,1919,1965,1976,1986,2390,2392,3140,3285,3411,3412,3731); zero WIP/capacity-policy hits. `rg -n -i 'capacity' /Users/tranquangdang21/Projects/beads_rust/src/main.rs` -> 0 hits (no Commands::Capacity dispatch). `rg -n -i 'capacity' /Users/tranquangdang21/Projects/beads_rust/src/cli/commands/stats.rs` -> 0 hits. `rg -ni 'wip_limit|wip-limit|concurrent_limit|max_in_progress|in_flight' /Users/tranquangdang21/Projects/beads_rust/src` -> 0 hits. DWS-only lines in the stats.rs diff are exactly the capacity additions.

**Upstream evidence**

> DWS: src/cli/commands/capacity.rs:1-12 documents `br capacity exempt/renew/revoke/exemptions`; registered at src/main.rs:704-705 `Commands::Capacity { command } => commands::capacity::execute(...)`; stats integration adds `capacity_stats(storage)` calling `storage.capacity_snapshot()` and `print_capacity_table` (diff vs LOCAL stats.rs shows the header row CAPACITY/COUNTED/AGGREGATES/EXEMPT/SOFT/HARD/REMAINING + STATE); backing policy at src/close_policy.rs:281 `pub struct CapacityPolicy` with `statuses`, `groups`, `admission`, `exemptions`, `scopes`.

</details>
