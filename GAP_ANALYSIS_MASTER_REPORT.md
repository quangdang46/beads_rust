# Master Gap Analysis Report: Go beads (bd) vs Rust beads_rust (br)

**Generated:** 2026-06-28  
**Source:** Multi-agent deep comparison workflow (26/30 agents completed, 7 phases)  
**Go Repo:** https://github.com/gastownhall/beads  
**Rust Repo:** https://github.com/quangdang46/beads_rust

---

## 📊 Executive Summary

| Metric | Go (bd) | Rust (br) | Delta |
|--------|---------|-----------|-------|
| **Total Lines of Code** | ~120,000+ | ~45,000+ | 2.7× |
| **Storage Engine** | Dolt (Git-versioned MySQL) | fsqlite (pure-Rust SQLite) | Fundamental divergence |
| **Architecture Style** | Clean Architecture, 14 sub-interfaces | Monolithic SqliteStorage (22K lines) | Interface vs concrete |
| **CLI Commands** | 50+ | 55 | Different philosophies |
| **External Integrations** | ADO, Linear, Jira, GitLab (plugin framework) | None | Major gap |
| **Extensibility** | Formula (3398 loc), Query DSL (1738 loc) | MCP server (5000+ loc), coordination | DSL vs protocol |

**Key Finding:** The two projects share the same *conceptual foundation* (SQLite + JSONL hybrid, issue tracking, dependencies) but have **diverged architecturally** around their storage engines. Go bet on Dolt for native SQL version control; Rust bet on fsqlite + JSONL git sync for simplicity and agent-first workflows.

---

## 🔟 Top 10 Priority Features to Port (Ranked by Impact/Effort)

| # | Feature | Category | Impact | Effort | Why It Matters |
|---|---------|----------|--------|--------|----------------|
| 1 | **Formula Language** | query | 🔴 High | 🔴 Large | 3398-line workflow-as-code engine enabling reusable templates, massive work generation, multi-agent coordination. Single biggest productivity multiplier. |
| 2 | **Query DSL** | query | 🔴 High | 🟡 Medium | String-based boolean expressions (`NOT`, wildcards, `7d` shorthands, metadata access). Flag-based filtering in Rust is less expressive and not programmatically accessible. |
| 3 | **Dolt Version Control / Time-Travel** | storage | 🔴 High | 🟣 Very Large | Native SQL versioning: `AS OF` queries, cell-level merge, branching, push/pull. Would require MySQL protocol client or application-layer VC. |
| 4 | **Federation / Peer-to-Peer Sync** | storage | 🔴 High | 🔴 Large | Dolt-backed distributed sync with sovereignty tiers, encrypted credentials, remote cache. Rust is single-repo JSONL only. |
| 5 | **External Tracker Integrations** | integration | 🟡 Medium | 🔴 Large | Plugin framework with FieldMapper adapters for ADO (state machine), Linear, Jira, GitLab. Enterprise adoption blocker. |
| 6 | **Backup/Restore System** | vc | 🟡 Medium | 🟡 Medium | Dolt-native remote backup (file://, DoltHub, aws://, gs://) with watermarks, auto-backup (15min throttle), cross-filesystem awareness. |
| 7 | **HookFiringStore Decorator** | storage | 🟡 Medium | 🟢 Small-Medium | Auto-fires `on_create`/`on_update`/`on_close` hooks after mutations with transaction-scoped deferral. Enables extensibility. |
| 8 | **Wisp Dual-Table Model** | storage | 🟡 Medium | 🟡 Medium | Separate `wisps` table (7 TTL types) for inter-agent coordination, concurrent work pools. Rust has single `issues` table only. |
| 9 | **Custom Status/Type System (DB-backed)** | storage | 🟡 Medium | 🟢 Small-Medium | Runtime workflow config without code changes. `custom_statuses`/`custom_types` tables with behavioral categories (active/wip/done/frozen). |
| 10 | **Compaction & Haiku AI Summarization** | vc | 🟢 Low-Medium | 🟡 Medium | Tiered compaction with Anthropic Haiku AI summaries, reversible via git snapshots. Rust stores metadata but has no compaction engine. |

---

## ✅ Where Rust Already Leads (No Gap to Fill)

| Feature | Rust Advantage |
|---------|----------------|
| **MCP Server** | Production-grade embedded (fastmcp_rust): 7 tools, 4 prompts, 12 resources, snapshot caching, sub-ms reads, structured errors. Go's is thin CLI wrapper over `bd` + Dolt. |
| **Rich Terminal Output** | TOON token-efficient format, 6 rich components (dep_tree, issue_panel, issue_table, progress, stats), themes, syntax highlighting, auto mode detection. |
| **Agent-First Design** | Shell completions for 5 shells, coordination contracts (pure evidence), scheduler, close policies (YAML), adaptive policies (schema-versioned). |
| **Type Safety** | Strong enums with `Custom(String)` fallback, `Option<T>` everywhere, `const fn` helpers, `sync_equals` semantic comparison (order-independent). |
| **Sync Safety** | Path containment validation, `.git` rejection, static analysis assertions, NUL byte rejection for SQLite compatibility. |

---

## 📋 Complete Gap Catalog (30+ Entries)

### 🗄️ Storage Layer Gaps

| Feature | Go Has | Rust Has | Gap Severity | Effort |
|---------|--------|----------|--------------|--------|
| **Dolt Version-Controlled Database** | Native Dolt (embedded + server), `CALL DOLT_COMMIT/MERGE/PUSH/PULL`, `AS OF` time-travel, cell-level merge, auto-commit/push, GC, flatten | fsqlite only, no versioning, no branching, no remote sync at DB level | 🔴 Critical | 🟣 Very Large |
| **Multi-Backend Storage Abstraction** | `Storage` interface (150 methods), 14 sub-interfaces, `Iter[T]` streaming, 3 backends (Dolt, EmbeddedDolt, placeholders) | Single `SqliteStorage` concrete struct, no trait, no backend abstraction | 🔴 High | 🔴 Large |
| **Schema Migration with Skew Detection** | 105 migrations, dual cursor tables, SHA-256 content hashing, 10-step pipeline, forward/backward skew blocking errors | 13 programmatic migrations via `PRAGMA user_version`, single `SCHEMA_SQL` constant | 🟡 Medium | 🟡 Medium |
| **HookFiringStore Decorator** | Auto-fires `on_create/on_update/on_close` with transaction-scoped deferral, nil-runner passthrough | No hook infrastructure | 🟡 Medium | 🟢 Small-Medium |
| **Wisp Dual-Table Model** | `wisps` + `wisp_events` + `wisp_comments`, 7 TTL types (heartbeat, ping, patrol, gc_report, recovery, error, escalation), `NoHistory` flag | Single `issues` table, `ephemeral` flag only | 🟡 Medium | 🟡 Medium |
| **Custom Status/Type System (DB-backed)** | `custom_statuses`/`custom_types` tables, `StatusCategory` (active/wip/done/frozen), max 50 custom statuses, category-annotated parsing | Compile-time enums with `Custom(String)` variant only | 🟡 Medium | 🟢 Small-Medium |
| **Federation / Peer-to-Peer Sync** | Dolt-backed `FederationPeer` (name, URL, credentials, sovereignty T1-T4), remote cache with TTL freshness, 16 URL schemes | Single-repo JSONL git sync only | 🔴 High | 🔴 Large |
| **Backup/Restore System** | Dolt-native remote backup (file://, DoltHub, aws://, gs://), watermark change detection, auto-backup (15min throttle), cross-filesystem, recovery `.bak` with fingerprint | Local JSONL history (`.br_history/`) with rotation (100 max, 30 days) | 🟡 Medium | 🟡 Medium |
| **Compaction & Haiku AI Summarization** | Tiered compaction, Anthropic Haiku prompts, `issue_snapshots`/`compaction_snapshots` tables, git-reversible, size validation | Inline metadata only (`compaction_level`, `compacted_at`, `original_size`) | 🟢 Low-Medium | 🟡 Medium |
| **Merge Slot System** | Serialized conflict resolution with wait queues | No equivalent | 🟢 Low | 🟡 Medium |
| **File-Based Circuit Breaker** | Dolt server connection resilience with configurable thresholds/cooldown | No equivalent (retry loops only) | 🟢 Low | 🟢 Small |
| **Credential Encryption (AES-GCM)** | Federation peer credentials encrypted, key derived from `.beads/` file | No equivalent | 🟢 Low | 🟢 Small |
| **Clone-Local Ignored Migrations** | `ignored_schema_migrations` table for wisps/local_metadata | No equivalent | 🟢 Low | 🟢 Small |

### 🔍 Query & Formula Gaps

| Feature | Go Has | Rust Has | Gap Severity | Effort |
|---------|--------|----------|--------------|--------|
| **Formula Language (Workflow-as-Code)** | 3398 lines: Parse→Resolve→Expand→Transform→ControlFlow→Condition→Cook. 4 types: workflow, expansion, aspect, convoy. Variables with defaults/enums/regex. Composition via BondPoints, Hooks, BranchRules, GateRules. | **None whatsoever**. Only CSV formula injection prevention comments. | 🔴 Critical | 🔴 Large |
| **Query DSL (String-Based Filtering)** | 1738 lines: lexer/parser/evaluator, recursive-descent AST, `NOT` > `AND` > `OR` precedence, wildcards (`*`), duration shorthands (`7d`), metadata access (`metadata.key`), boolean expressions with parentheses, library entry points | Flag-based `ListFilters` struct, client-side filtering for non-SQL fields, saved queries store `SavedFilters` structs not strings | 🔴 High | 🟡 Medium |
| **Duration Shorthand Time Filters** | `created>7d`, `updated<24h` on all timestamp fields | `updated_before`/`updated_after` exist on `ListFilters` but **not exposed to CLI** | 🟡 Medium | 🟢 Small |
| **Wildcard ID/Spec Matching** | `id=bd-*`, `spec=proj-*` | No wildcard support | 🟢 Low | 🟢 Small |
| **Metadata Field Filtering** | `metadata.key=value`, `has_metadata_key` | No metadata column on issues (separate table only) | 🟡 Medium | 🟡 Medium |
| **Owner/MolType Boolean Filters** | `owner=alice`, `pinned=true`, `mol_type=swarm` | Not exposed via CLI `ListArgs` | 🟢 Low | 🟢 Small |

### 🔌 Integration & Server Gaps

| Feature | Go Has | Rust Has | Gap Severity | Effort |
|---------|--------|----------|--------------|--------|
| **External Tracker Integrations** | Plugin framework: ADO (PAT auth, WIQL, state machines for Agile/Scrum/CMMI process templates, exponential backoff), Linear, GitLab, Jira. `IssueTracker` interface + `FieldMapper` adapters. | **Zero**. No tracker trait, no HTTP clients, no auth management. | 🔴 High | 🔴 Large |
| **Proxied Server Mode (HTTP REST API)** | Dolt SQL proxy: `create/show/list/close_proxied_server`, auto-port, PID files, file locking, multi-process concurrency, `--as-of` historical queries | MCP stdio server only (AI agents), no HTTP REST, no database proxy | 🟡 Medium | 🔴 Large |
| **MCP Server Architecture** | Python/fastmcp wrapper shelling to `bd` CLI → Dolt. 14 tool-like fns, 1 resource, 0 prompts, seconds latency | **Rust supersedes**: embedded fastmcp_rust, 7 tools (batch, structured errors), 4 prompts (guided workflows), 12 resources (analytics, graph health, bottlenecks), snapshot caching | ✅ Rust Leads | N/A |
| **Recipe/Plugin System (AI Tool Install)** | `bd init_templates` → AGENTS.md with ProfileFull/Minimal variants, skill templates (SKILL.md), 12+ AI tool recipes (Claude Code, Cursor, Windsurf, Copilot, Codex, Aider), NPM package | Single `AGENT_BLURB` constant, `br agents` manages AGENTS.md with versioned markers only | 🟡 Medium | 🟢 Small-Medium |
| **Metrics/Telemetry (OTLP)** | OTLP export for observability | No metrics/telemetry, only health diagnostics | 🟢 Low | 🟡 Medium |

### 🌿 Version Control & Git Gaps

| Feature | Go Has | Rust Has | Gap Severity | Effort |
|---------|--------|----------|--------------|--------|
| **Git Hooks System** | `bd hooks` install: post-merge import, pre-commit, update-close hooks. Auto import/export on git operations. | **Explicitly avoids git commands**. Uses `git2` for read-only. No hook infrastructure. | 🟡 Medium | 🟢 Small-Medium |
| **Worktree Integration** | `bd worktree` command, redirect system (`.beads/redirect`), per-worktree DB modes, shared `.beads` via `git-common-dir`, JJ secondary workspace parity, symlink normalization | Simpler discovery in `StartupContext::init`, no redirect, no JJ parity, no per-worktree DB | 🟡 Medium | 🟡 Medium |
| **Issue Snapshots for Compaction** | `issue_snapshots` + `compaction_snapshots` tables with `compressed_size`, `archived_events` | Inline metadata only | 🟢 Low-Medium | 🟡 Medium |
| **Repository File Tracking** | `repo_mtimes` table for efficient re-sync | `dirty_issues` + `export_hashes` only | 🟢 Low | 🟢 Small |

### 🖥️ CLI Command Gaps

| Feature | Go Has | Rust Has | Gap Severity | Effort |
|---------|--------|----------|--------------|--------|
| **Template Management** | `bd template` CRUD, required sections per IssueType (bugs: Steps + Acceptance; tasks/features: Acceptance; epics: Success Criteria), template validation via lint | `br lint` checks sections but no template CRUD. `is_template` flag exists but no loading/management | 🟡 Medium | 🟢 Small |
| **Export Command** | `bd export` to JSONL, Obsidian, Graph formats | `br sync --flush-only` + schema for JSON only | 🟢 Low | 🟢 Small |
| **Import Command** | `bd import` from JSONL, CSV, Markdown | `br sync --import-only/--merge` only | 🟢 Low | 🟢 Small |
| **Rename Command** | `bd rename` for issue ID prefix changes | No equivalent | 🟢 Low | 🟢 Small |
| **Quick/Quickstart Command** | `bd quick` / `bd quickstart` for rapid onboarding | `br q` exists but different purpose | 🟢 Low | 🟢 Small |
| **Admin Command** | `bd admin` for administrative operations | No equivalent | 🟢 Low | 🟢 Small |

### 🏷️ Data Model Gaps (Fields in Go Issue NOT in Rust)

| Field | Go Type | Purpose |
|-------|---------|---------|
| `SpecID` | string | Links issue to specification document |
| `StartedAt` | timestamp | When issue transitioned to in_progress |
| `Metadata` | `json.RawMessage` | Arbitrary JSON blob for extensions (validated well-formed) |
| `NoHistory` | bool | Wisps stored in wisps table but NOT GC-eligible |
| `WispType` | enum | TTL-based compaction: heartbeat, ping, patrol, gc_report, recovery, error, escalation |
| `BondedFrom` | `[]BondRef` | Compound molecule lineage |
| `AwaitType`, `AwaitID`, `Timeout`, `Waiters` | various | Async gate coordination (formula gates) |
| `SourceFormula`, `SourceLocation` | string | Formula cooking origin tracing |
| `MolType` | enum | Molecule type: swarm, patrol, work |
| `WorkType` | enum | Assignment model: mutex, open_competition |
| `EventKind`, `Actor`, `Target`, `Payload` | various | Event-oriented operational state changes |
| `IDPrefix`, `PrefixOverride` | string | Internal routing (not JSON-serialized) |
| `StatusHooked` | const | 7th built-in status: "hooked" |

### 🏷️ Missing Enum Types in Rust

| Enum | Go Variants | Rust Equivalent |
|------|-------------|-----------------|
| `WispType` | heartbeat, ping, patrol, gc_report, recovery, error, escalation | ❌ None |
| `MolType` | swarm, patrol, work | ❌ None |
| `WorkType` | mutex, open_competition | ❌ None |
| `StatusCategory` | active, wip, done, frozen, unspecified | ❌ None |
| `CustomStatus` | name + category struct with parsing | ❌ None |
| `RequiredSection` | heading + hint per IssueType | ❌ None |

### 🏷️ Missing IssueType Variants in Rust

| Go Built-in Types | Rust Built-in Types |
|-------------------|---------------------|
| bug, feature, task, epic, chore, **decision, message, molecule, gate, spike, story, milestone, event** (13) | task, bug, feature, epic, chore, docs, question (7) |

### 🏷️ Missing DependencyType Variants in Rust

| Go (19) | Rust (11) | Missing |
|---------|-----------|---------|
| blocks, parent-child, conditional-blocks, waits-for, related, discovered-from, replies-to, relates-to, duplicates, supersedes, **authored-by, assigned-to, approved-by, attests, tracks, until, validates, delegated-from, caused-by** | blocks, parent-child, conditional-blocks, waits-for, related, discovered-from, replies-to, relates-to, duplicates, supersedes, caused-by | 8 variants |

---

## 📁 Missing Database Tables in Rust (Exist in Go)

| Table | Purpose |
|-------|---------|
| `wisps` | Extended/shared work items mirroring `issues` columns |
| `wisp_events` | Events on wisps |
| `wisp_comments` | Comments on wisps |
| `custom_statuses` | Dynamic workflow status configuration |
| `custom_types` | Dynamic issue type configuration |
| `issue_snapshots` | Full issue snapshots with `compressed_size`, `archived_events` |
| `compaction_snapshots` | BLOB snapshot JSON for compaction recovery |
| `repo_mtimes` | Repository file mtime tracking for sync |
| `routes` | Route prefix → path mapping |
| `issue_counter` | Prefix-based sequential ID counter |
| `interactions` | Agent session recording (tool calls, prompts, responses, errors, exit codes) |
| `federation_peers` | Multi-repo federation configuration |
| `ready_issues` (VIEW) | Recursive CTE view of unblocked ready issues |
| `blocked_issues` (VIEW) | View of blocked issues with blocker counts |

---

## 📁 Extra Tables in Rust (Not in Go)

| Table | Purpose |
|-------|---------|
| `dirty_issues` | Explicit change-tracking for incremental JSONL export |
| `export_hashes` | Content hash tracking per issue for incremental sync |
| `blocked_issues_cache` | Materialized cache of blocked-by relationships (Go uses VIEWs) |
| `close_metadata` | Closure-time policy gate attribution + bypass audit |
| `gate_results` | Workflow gate pass/fail verdicts per (issue, gate, provider) |

---

## 🏗️ Architectural Differences Deep Dive

### Storage Engine: The Fundamental Divergence

| Aspect | Go (Dolt) | Rust (fsqlite) |
|--------|-----------|----------------|
| **Version Control** | Native: `CALL DOLT_COMMIT`, branches, `AS OF` | Application-layer: JSONL + Merkle witnesses |
| **Time Travel** | `SELECT * FROM issues AS OF '2026-01-01'` | No equivalent |
| **Merge** | Cell-level, Dolt merge algorithms, conflict resolution | Git merge on JSONL (line-based) |
| **Remote Sync** | `DOLT_PUSH/PULL/FETCH` to DoltHub/remotes | `git push/pull` on JSONL files |
| **Schema Evolution** | 105 migrations, skew detection, content hashing | 13 versions, programmatic, no skew detection |
| **Multi-Process** | Client-server model, connection pooling | File locks (`.write.lock`, `.sync.lock`) |

### Architecture Style

| Aspect | Go (Clean Architecture) | Rust (Monolithic) |
|--------|------------------------|-------------------|
| **Storage Interface** | `Storage` (150 methods), 14 sub-interfaces | Single `SqliteStorage` struct |
| **Repository Pattern** | Domain interfaces + SQL repos in `issueops/` | Inline SQL in `sqlite.rs` (22K lines) |
| **Unit of Work** | `UoW` provider, transaction-scoped hooks | Manual `BEGIN IMMEDIATE` + retry loops |
| **Hooks** | `HookFiringStore` decorator | None |
| **Error Handling** | `dberrors` typed errors (NotFound, Conflict, NotImplemented) | `BeadsError::Database` wrapping `FrankenError` |

### Concurrency Model

| Aspect | Go | Rust |
|--------|-----|------|
| **Write Concurrency** | Dolt SQL transactions, commit-based | WAL-mode SQLite, 8-attempt jittered retry |
| **Cross-Process Sync** | Dolt server coordination | File locks + JSONL witnesses |
| **Read Scaling** | Connection pooling | Single connection (fsqlite) |

---

## 💡 Implementation Recommendations

### Phase 1: Quick Wins (1-2 weeks each)

1. **Template CRUD** — Add `br template` commands using existing `is_template` issues
2. **Custom Status/Type Tables** — Schema migration + validation + CLI config
3. **Hook Infrastructure** — Middleware trait wrapper: `HookFiringStore<S: SyncStorage>`
4. **Export/Import Commands** — Wrap existing sync logic with format options

### Phase 2: Medium Investments (1-3 months each)

5. **Query DSL** — Separate crate using `pest`/`nom`. SQL translation for simple-AND path + predicate functions for OR/NOT.
6. **Wisp Dual-Table** — Add `wisps`/`wisp_events`/`wisp_comments` tables, TTL compaction types, query modifications.
7. **Backup Cloud Targets** — Extend `.br_history/` with S3/GCS/Azure targets, auto-trigger config.
8. **Agent/Skill Templates** — Embedded templates with ProfileFull/Minimal variants, marker versioning.
9. **Duration Shorthand** — Expose `updated_before`/`updated_after` to CLI, add duration parsing.

### Phase 3: Major Undertakings (6-18 months each)

10. **Formula Language** — Parser, type system, expansion engine, control flow interpreter, composition logic. Essentially a small programming language.
11. **Dolt Integration** — Option A: MySQL protocol client to external `dolt sql-server` (closest parity). Option B: Application-layer VC on JSONL. Option C: Wait for native Rust Dolt bindings.
12. **Federation** — Requires Dolt integration or new CRDT-based sync protocol. Remote cache alone is significant subsystem.
13. **External Tracker Plugins** — `Tracker` trait, `reqwest` HTTP clients, auth management, field mapping, state machines for each tracker's process templates.

---

## 📈 Effort vs Impact Matrix

```
IMPACT
  ▲
H │  ◼ Formula Language          ◼ Dolt VC/Time-Travel
I │  ◼ Query DSL                 ◼ Federation
G │  ◼ External Trackers
H │  ◼ Backup/Restore            ◼ Wisp Dual-Table
  │  ◼ HookFiringStore           ◼ Compaction/Haiku
M │  ◼ Custom Status/Type
E │  ◼ Template CRUD             ◼ Export/Import
D │  ◼ Duration Shorthand
I │  ◼ Agent Templates           ◼ Rename/Quick/Admin
U │  ◼ Git Hooks                 ◼ Worktree
M │  ◼ Repo Mtimes               ◼ Circuit Breaker
  │  ◼ Credential Encryption     ◼ Ignored Migrations
  └─────────────────────────────────────────────▶ EFFORT
    Small      Medium        Large       Very Large
```

---

## 🎯 Recommended Priority Order for beads_rust

1. **Template CRUD** (Small) — Immediate user value, leverages existing `is_template`
2. **Custom Status/Type Tables** (Small-Medium) — Unlocks workflow flexibility without code changes
3. **Hook Infrastructure** (Small-Medium) — Enables extensibility for notifications/integrations
4. **Query DSL** (Medium) — High leverage for programmatic access and complex filters
5. **Wisp Dual-Table** (Medium) — Enables true agent swarm coordination
6. **Backup Cloud Targets** (Medium) — Operational robustness
7. **Agent/Skill Templates** (Small-Medium) — Improves AI onboarding consistency
8. **Formula Language** (Large) — Transformative productivity multiplier
9. **Dolt VC Integration** (Very Large) — Architectural decision point
10. **Federation** (Large) — Requires Dolt or new protocol
11. **External Tracker Plugins** (Large) — Enterprise adoption

---

## 📊 Lines of Code Comparison

| Component | Go (bd) | Rust (br) | Notes |
|-----------|---------|-----------|-------|
| **Storage Layer** | ~3.5 MB (105 .go + 115 .sql) | ~1.0 MB (4 .rs) | Go has Dolt, embedded Dolt, migrations, federation |
| **Core Types** | 54 KB (1,467 lines) | 19 KB (1,919 lines) | Go has 50+ fields, wisps, formulas, gates, molecules |
| **Formula Engine** | 3,398 lines | 0 | Complete gap |
| **Query DSL** | 1,738 lines | 0 (flag-based only) | Complete gap |
| **CLI Commands** | ~50 commands | ~55 commands | Different philosophy |
| **MCP Server** | Thin wrapper | 5,000+ lines | Rust leads |
| **Integrations** | ADO, Linear, Jira, GitLab, Notion, GitHub, GitLab | None | Complete gap |
| **Backup/VC** | Dolt backup, dolt version control | JSONL history only | Major gap |
| **Templates/Recipes** | Embedded agent/skill templates, 12+ AI recipes | Single constant | Major gap |

---

## 🔗 Related Resources

- **Workflow Artifacts:** `/private/tmp/claude-501/-Users-tranquangdang21-Projects-beads-rust/378c1ed3-73e1-4b36-8e2c-98a256962e47/tasks/w840ewczd.output`
- **Go Repository:** https://github.com/gastownhall/beads
- **Rust Repository:** https://github.com/quangdang46/beads_rust
- **Analysis Date:** 2026-06-28
- **Agents Used:** 30 agents across 7 phases (26 successful, 4 API errors)

---

## 📝 Conclusion

The gap analysis reveals **two distinct but related products** sharing a common ancestor (Steve Yegge's beads). 

**Go (bd)** evolved toward **enterprise/distributed workflows**: Dolt for version control, federation for multi-repo sync, formula/query DSLs for programmable workflows, tracker integrations for enterprise tooling.

**Rust (br)** evolved toward **agent-first local workflows**: MCP for AI access, TOON for token efficiency, coordination contracts for swarm diagnosis, close policies for governance.

**The biggest strategic question for beads_rust:** Whether to pursue Dolt integration (major architectural shift) or double down on JSONL git sync with application-layer versioning (current path). The formula language and query DSL are the highest-leverage features that could be ported without changing the storage engine.

**Recommendation:** Start with quick wins (templates, custom status, hooks) to close UX gaps, then evaluate formula language as a separate crate that could benefit both projects.

---

*Report generated by multi-agent ultracode workflow. All findings based on code analysis as of 2026-06-28.*