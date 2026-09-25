# Migration Plan: fsqlite (frankensqlite) to rusqlite

**Status:** Proposed, ready for execution
**Date:** 2026-09-25
**Tracking bead:** `beads_rust-t6se`
**Scope:** Replace the 15 `fsqlite-*` crates with `rusqlite` 0.40.2 backed by bundled SQLite C
**Estimate:** 35 engineer-days across 8 phases

---

## 1. Decision

```toml
rusqlite = { version = "0.40.2", default-features = false,
             features = ["bundled", "cache", "fallible_uint"] }
```

Delete all 15 `fsqlite-*` declarations at `Cargo.toml:45-59` in the same commit that adds this
line. Land the port as a single atomic merge to `main`. Work proceeds on a long-lived branch as
8 individually revertible commits, but `main` never sees a half-migrated tree.

**Rollback** is a cache rebuild, or a revert of the merge commit if that is ever needed.

### Why this target

`rusqlite` is the only mature, actively maintained, synchronous Rust binding to SQLite. Its API is
close to a drop-in for the existing call sites: `Connection::open`, `execute`, `prepare`,
`query_row`, `execute_batch`, `types::Value`, and `Error::QueryReturnedNoRows` all exist. The
object-safe `Storage` trait at `src/storage/trait_.rs:28` needs no signature change, so all 35+
CLI commands, the formatter, and the sync engine stay untouched.

MSRV 1.88.0 and edition 2024 match this crate exactly.

### Why the alternatives were rejected

**libsqlite3-sys used directly: structurally blocked.**
`unsafe_code = "forbid"` is declared twice, at `Cargo.toml:183` and `src/lib.rs:22`. `forbid` cannot
be downgraded with `#[allow]`, so hand-written FFI glue cannot live in this crate at all. Making it
work would require a new sibling path-crate that permits unsafe, which converts a clean "zero
unsafe" property into a qualified one. It would also cost an estimated 1800 to 3000 lines of new
unsafe code reimplementing `Connection`, `Statement`, `Row`, `ValueRef`, `ToSql`/`FromSql`, the
error taxonomy, and Drop safety, putting the deterministic exit-code table (asserted by the
Go-parity conformance suite) at risk of silent drift.

This buys nothing: `rusqlite` re-exports the identical `-sys` crate as `rusqlite::ffi` and exposes
`Connection::handle()`, so the same API reach arrives with the safe layer intact.

**sqlx 0.9: rejected.**
This is not a driver swap, it is a rewrite of the crate's concurrency model, and it ends worse on
four stated properties:

1. Zero synchronous API. Every `Executor` method returns `Pin<Box<dyn Future + Send>>`,
   `Connection::connect` is an `impl Future`, and `Connection::transaction` requires an async
   closure. The `Storage` trait and 35+ CLI modules would go async, or every query would be wrapped
   in `block_on`, which is a permanent source of "why is my CLI hanging" for a tool whose value is
   fast agent invocation.
2. `tokio` becomes a mandatory default-path dependency, reversing a deliberate design where it is
   web-feature-only.
3. MSRV 1.94.0 exceeds the declared 1.88.
4. Version 0.9.0 made `SqliteValueRef` decoding stateful, so reading a column as `&str` and then as
   `i64` on the same row errors without a reset. That directly punishes this codebase's
   `row.get(0).and_then(as_text)` pattern, which appears roughly 250 times.

**Pure-Rust engines: there is no viable option.**
The binding constraint is multi-process WAL on Windows, which is `br`'s actual operating mode
(`.beads/.write.lock` serializing an agent swarm). Reviewed and rejected:

- **frankensqlite (stay or upgrade):** ships `LicenseRef-MIT-OpenAI-Anthropic-Rider`, already
  surfaced in this repo at `README.md:1008` and `CHANGELOG.md:698`. The rider text purports to deny
  rights to OpenAI, Anthropic, and entities acting on their behalf, and defines "use" to include
  executing and testing. It is also pre-1.0 with 35 published versions in 7 months, where v0.3.9 was
  an expedited patch because v0.3.8 shipped a CLI that could not open file databases at all.
- **Turso:** the only pure-Rust engine with real production evidence, and also pre-1.0. Its MVCC is
  documented as not production-ready (no index creation under MVCC, only
  `wal_checkpoint(TRUNCATE)` works, docs warn queries may return incorrect results). Its
  multi-process `.tshm` coordinator is Unix-only: on Windows the flag is accepted and silently has
  no effect, so the database opens single-process.
- **oxisqlite:** Alpha, 1 dependent crate, two yanked versions, roughly 315 surviving `.unwrap()`
  calls in production paths.
- **graphitesql:** 8 stars, 3 months old, one contributor.

Staying on frankensqlite is not a fallback. The license rider alone rules it out for an agent-first
tool, and "wait for a better frankensqlite" cannot resolve a rider that names the tool's own users.

### Confidence and what would change the decision

**HIGH** on target selection, the data-compatibility go/no-go, and the build-feasibility go/no-go.
All three were verified against primary sources and the local tree, not recalled.

**HIGH** on the shape of the port. **MEDIUM** on the 35-day figure, which carries real uncertainty
in exactly one place: the 1877-call-site read/write port inside a 24,646-line file.

Conditions that would reopen the decision:

1. A `rusqlite` 0.40.x regression in WAL plus `BEGIN IMMEDIATE` under heavy multi-process writer
   contention. This is the one untested assumption. See section 9.
2. A discovered frankensqlite-only on-disk artifact. The 11 project fixtures share the standard
   SQLite format 3 header, which is strong but not exhaustive. A non-standard sidecar or WAL
   layout quirk would only surface by actually opening them, which Phase 1 does in 1.5 days.
3. If `bundled` fails to build on the RCH fleet, that is a one-day infra fix discovered in Phase 1,
   not at Phase 8.
4. If a mature pure-Rust engine ships cross-process WAL on Windows within the migration window,
   re-open the target choice. Turso's `.tshm` coordinator would need to become cross-platform, and
   its MVCC would need to leave the not-production-ready state.

---

## 2. Why each feature flag

| Flag | Reason |
|---|---|
| `bundled` | The only mode that avoids a `pkg-config` or vcpkg bootstrap. Compiles the vendored SQLite amalgamation through the `cc` build dependency. |
| `cache` | Prepared-statement LRU. Moved into the default feature set in 0.38, so it is lost only because `default-features = false` is passed. Listed explicitly so the decision is manifest-visible and survives a future upstream default change. Losing statement reuse across the hot query paths would be a silent benchmark-only regression. |
| `fallible_uint` | Re-enables `u64`/`usize` `ToSql`/`FromSql`, disabled by default since 0.38.0. The storage layer uses unsigned counters, so this is mandatory. |
| `default-features = false` | Drops `ffi-sqlite-wasm-rs`, the only other default member. It activates solely on `wasm32-unknown-unknown` and is irrelevant to a native-only CLI. |

**Two features must stay OFF:** `session` and `preupdate_hook`. Verified from the upstream manifest,
`preupdate_hook = ["libsqlite3-sys?/buildtime_bindgen"]` and `session = ["preupdate_hook", ...]`, so
either one silently reintroduces a libclang build dependency on all 8 RCH workers.

`bundled` transitively enables `modern_sqlite` (which is `bundled_bindings`), so it does not need to
be listed separately. The historical line at `d3d9bce6^:Cargo.toml:22` listed it redundantly.

---

## 3. On-disk compatibility: go, no data migration required

**Verdict: PASS.**

The 11 project databases under `sample_beads_db_files/*/beads.db` (asupersync, beads_rust,
flywheel_connectors, franken_whisper, frankensqlite, frankenterm, frankentui, mcp_agent_mail_rust,
mcp_agent_mail_website, ntm, remote_compilation_helper) plus 2 `repro_*` fixtures all begin with the
bytes `SQLite format 3`, verified offline with a header read. frankensqlite writes a genuine SQLite
format 3 file, so the format claim is established. What remains to prove is the content claim:
that C SQLite can open these files, run `integrity_check`, and read rows out of them. Phase 1 does
that in half a day.

**The recovery path is a cache rebuild, not a data migration.** `.beads/beads.db` is a gitignored,
machine-local derived cache and is absent from the tree. The canonical tracked source is
`.beads/issues.jsonl`. A user upgrading `br` rebuilds the cache through the already-shipped
`rebuild_database_from_jsonl` / `repair_database_from_jsonl` paths.

Three findings that specifically rule out a bespoke audit-and-repair migration:

1. The `SELECT`-then-`INSERT` uniqueness check at `sqlite.rs:2402` is not TOCTOU-racy: it runs
   inside `BEGIN IMMEDIATE`, which serializes writers.
2. The hand-rolled `DELETE`+`INSERT` sequences cannot self-collide, because the conflicting row is
   deleted first. That is precisely why the pattern is used instead of `INSERT OR REPLACE`. A
   mid-sequence UNIQUE abort rolls the whole transaction back, so no partially applied mutation is
   possible.
3. The `config` and `metadata` key-value tables deliberately declare no constraints
   (`schema.rs:203-211`) and are rebuilt to strip a legacy autoindex, so they are unaffected.

Duplicate and orphan auditing already exists in `br doctor`.

---

## 4. Build and CI feasibility: go

**Verdict: PASS. C is not a new requirement class for this repository.**

`Cargo.toml:127-128` declares `mimalloc = "0.1.52"` under `[target.'cfg(not(windows))'.dependencies]`,
which resolves to `libmimalloc-sys` and then to `cc`. `blake3`, `alloca`, and
`iana-time-zone-haiku` are further `cc` consumers in `Cargo.lock`. All 8 Contabo RCH workers and the
Ubuntu CI runners therefore already require a working C compiler to build `main` today. What
`bundled` adds is one more `cc::Build` over the roughly 250,000-line SQLite amalgamation. The
marginal cost is build time, on the order of 10 to 30 seconds per clean target build.

Nuance in the other direction: because `mimalloc` is gated on `cfg(not(windows))`, the Windows dev
box currently has no C requirement from `mimalloc`. MSVC's `cl.exe` is present and `cc`
auto-detects it, so `bundled` still needs no vcpkg bootstrap.

**libclang is not required.** From the upstream `libsqlite3-sys` manifest, `bundled = ["cc",
"bundled_bindings"]` and `bundled_bindings = []`. `build.rs` copies pregenerated bindings from
`sqlite3/bindgen_bundled_version.rs` into `OUT_DIR`, so an offline-capable build is achievable.
`buildtime_bindgen` is the only path that needs libclang, and only `session` and `preupdate_hook`
reach it.

**Cross-compilation does not apply.** Every release job is a native runner: `ubuntu-latest` with
`x86_64-unknown-linux-musl`, `ubuntu-24.04-arm` (its own workflow comment reads "Native ARM runner -
no cross-compilation / no cross crate"), `macos-latest` twice, and `windows-latest` with
`x86_64-pc-windows-msvc`. The musl jobs already run `apt-get install -y musl-tools` at
`release.yml:24-25` and `:52-53`, which is exactly the toolchain `cc`-rs needs. No `CC_<target>`
variables, no `-arch` handling, and no workflow restructuring.

`Vcpkg`, `VCPKGRS_DYNAMIC`, `SQLITE3_LIB_DIR`, `SQLITE3_INCLUDE_DIR`, and `SQLITE3_STATIC` are
system-mode-only and are not needed.

**RCH dependency sync actually gets easier.** The comment at `Cargo.toml:44` says the `fsqlite`
crates are listed explicitly "so rch syncs them". That constraint disappears: `rusqlite`,
`libsqlite3-sys`, `hashlink`, and `cc` all resolve from crates.io with no sibling checkout. Verify
all 8 workers resolve and build them in Phase 1 anyway.

**Binary size: measure, do not assume.** Linking C SQLite adds roughly 1 to 2 MB against the
`opt-level = "z"` / `lto = true` / `codegen-units = 1` / `panic = "abort"` / `strip = true` release
profile. Record the baseline in Phase 1 and the delta in Phase 8. A large regression is a finding,
not a note.

**This repository already shipped this exact configuration.** `git show d3d9bce6^:Cargo.toml:22`
reads:

```toml
rusqlite = { version = "0.38", features = ["bundled", "modern_sqlite", "fallible_uint"] }
```

That build passed this same CI matrix and this same Windows dev box.

### Supply-chain documentation to update

- `docs/CI_SUPPLY_CHAIN.md`: record the marginal C compile and the `session`/`preupdate_hook`
  guardrail. The policy currently has no provision for a vendored C amalgamation.
- `docs/ARCHITECTURE.md`
- `docs/SYNC_SAFETY.md`: record the composed worst-case timeout.
- `docs/operations/UPGRADE_LOG.md`: record the license rider removal and the MVCC negative result.
- `AGENTS.md` dependency table.
- `README.md`: the "MIT with OpenAI/Anthropic Rider" badge at `:9` and the text at `:1008` both need
  replacing with plain MIT.
- `CHANGELOG.md`: the "Updated to MIT with OpenAI/Anthropic Rider" entry at `:698` is historical and
  stays as a record; add a new entry that supersedes it.

---

## 5. Call-site inventory

**1,877 fsqlite call sites in `src/storage/sqlite.rs` alone** (24,646 lines: 1,534 production, 343
test). Plus 162 direct `conn.*` DML and query calls plus 25 shim invocations in
`src/storage/schema.rs`, 48 in `src/storage/events.rs`, and 62 `use fsqlite` import lines spread
across 37 files in `src/`, `tests/`, and `benches/`. All locally verified.

| Pattern | Count |
|---|---|
| `SqliteValue::from` constructors | 602 |
| `row.get(idx)` returning `Option<&SqliteValue>` | >268 |
| `SqliteValue::as_text` accessor | 305 |
| `conn.execute_with_params(sql, &[SqliteValue])` | 138 |
| `SqliteValue::as_integer` accessor | 95 |
| `conn.execute(sql)` with no params | 92 |
| `conn.query_with_params(...)` returning `Vec<Row>` | >91 |
| `conn.query(sql)` returning `Vec<Row>` | >70 |
| `SqliteValue::Null` construction (nullable-column idiom) | 71 |
| `conn.query_row_with_params(sql, &[SqliteValue])` | 41 |
| `SqliteValue` variant matches (Integer/Text/Blob/Float) | 18 |
| `conn.query_row(sql)` with no params | 15 |
| `Connection::open` and `fsqlite::compat::open_with_flags` | 14 |
| `Row::values()` returning `&[SqliteValue]` | 19 |
| `conn.prepare(sql)` | 7 |
| `FrankenError::is_transient()` | 7 |
| `FrankenError::QueryReturnedNoRows` | 20 |
| `Statement::explain()` (test-only, delete, do not port) | 2 |
| `conn.close_in_place()` (Drop teardown, delete) | 1 |
| `conn.query_map` (not used anywhere) | 0 |

`row.get` (268) plus `SqliteValue::from` (602) plus `as_text` (305) equals 1,175 of 1,877, or 63% of
the surface. This is the read and parameter idiom, and it is the dominant cost. 250 of the 268
`row.get` sites are production. The `row.get(i).and_then(SqliteValue::as_text)` double-Option chain
appears roughly 250 times.

**Correction to earlier research notes:** there is no `SqliteValue::Real` in frankensqlite. The
variant is `Float` (3 uses: 2 production in `parse_datetime_value` / `parse_opt_datetime_value`, 1
test). rusqlite names it `Real`. A mechanical `Float` to `Real` rename is correct; any search for
`Real` in the current codebase will wrongly conclude the code is inconsistent.

---

## 6. Execution strategy: big-bang on the branch, atomic on main

Both judge panels in planning split between this and a strangler-adapter approach. `AGENTS.md` is
the tiebreaker and it is unambiguous: "Never create compatibility shims", "We want to do things the
RIGHT way with NO TECH DEBT", and the bar for new files is "incredibly high".

A strangler's `src/storage/dialect/` module is a multi-file compatibility shim behind an engine
switch, and its two claimed benefits are not deliverable by its own mechanism. A compile-time switch
cannot deliver per-file incremental landing, because the whole crate flips at once. Making it
incremental requires a runtime enum, which is the dual-engine shim the strategy explicitly refuses
to write.

The decisive additional fact: a strangler keeps frankensqlite in the tree and in every release for
the entire window, and frankensqlite ships a license rider that names the tool's own users. Holding
that in-tree for five weeks to serve as a rollback arm is the wrong trade for a tool whose
`AGENTS.md` drives Claude Code and Codex sessions.

One idea from the strangler is genuinely better and is kept: a **differential parity harness**
comparing fixed CLI invocations across engines. It is grafted in as a **permanent test** rather than
a temporary module, so it survives the migration and keeps paying after the adapter is deleted.

---

## 7. Phase plan

Each phase is one commit on the branch, individually revertible during development. The whole
sequence merges to `main` as one merge commit with the 15 `fsqlite` lines removed.

### Phase 1: Build de-risk, frozen baseline, on-disk format proof (2.5 days)

**Goal:** retire the largest unknown before any of the 1877 call sites depend on it, and prove the
bundled build resolves on the RCH fleet.

- Write `tests/storage_engine_compat.rs`: open all 11 `sample_beads_db_files/*/beads.db` plus the 2
  repro fixtures under rusqlite, assert `PRAGMA integrity_check == 'ok'` and
  `SELECT COUNT(*) FROM issues > 0` for each.
- Swap the manifest: delete the 15 declarations at `Cargo.toml:45-59`, add the `rusqlite` line,
  regenerate `Cargo.lock`. Nothing imports rusqlite yet, so this compiles only the dependency.
- Run `rch exec -- cargo build --release` on all 8 workers. This is the gating infra check. If a
  worker cannot build it, that is a one-day fix discovered now rather than in Phase 8.
- Freeze the baseline. Run `rch exec -- cargo test --all-features` and
  `rch exec -- cargo clippy --all-targets -- -D warnings` on unmodified `main`. Record exact pass
  and fail counts and the current release binary size. Every later phase is judged against "no NEW
  failures versus this baseline", not "all green" on a suite of roughly 1830 tests.
- Build the differential parity corpus and goldens: roughly 60 fixed CLI invocations capturing
  stdout, stderr, and exit code (create, show, update, close, dep, labels, ready, blocked, list,
  search, stats, epic, audit, sql, `doctor --json`, `sync --flush-only`), plus `sqlite_master` and
  `PRAGMA table_info` dumps for every table.

**Exit criteria:** all 13 fixtures open with `integrity_check = ok`; `bundled` builds on all 8
workers; baseline pass/fail counts and binary size recorded; parity goldens committed.

### Phase 2: Error taxonomy and `is_transient` against C error codes (2 days)

**Goal:** reimplement the single predicate that gates the entire concurrency model, before any
volume code depends on it.

- Change `src/error/mod.rs:42` to `Database(#[from] rusqlite::Error)`.
- Reimplement `BeadsError::is_transient()` at `error/mod.rs:210` against `sqlite_error_code()`
  for `DatabaseBusy` and `DatabaseLocked`. Match `Error::SqliteFailure(ref e, _)`, never
  string-match. `sqlite.rs:10868` (`is_transient_wal_tail_read_error`) is exactly the wrong shape.
- Add a table-driven test covering `SQLITE_BUSY`, `SQLITE_LOCKED`, `SQLITE_CONSTRAINT`,
  `SQLITE_CORRUPT`, `SQLITE_READONLY`, including extended-code variants. Decide explicitly, in a
  doc comment, whether `SQLITE_PROTOCOL`, `SQLITE_INTERRUPT`, and `SQLITE_IOERR` are transient.
- Exit-code and structured-error tests must pass with **zero test edits**, because the deterministic
  exit-code table is a shipped Go-parity contract.

**Exit criteria:** `cargo test error` green; the full error-taxonomy test set green with no
assertion edits.

### Phase 3: Type-compat adapter and 26 leaf consumers outside storage (5.5 days)

**Goal:** build the shape-compat layer that collapses the mechanical majority into type-path churn,
then prove it on the 26 files that contain no transaction logic before touching the 24.6k-line file.

- Create `src/storage/db.rs` with four helpers:
  - `SqlValue`: a newtype over `rusqlite::types::Value` implementing `ToSql`, preserving the `From`
    impls so all 602 `SqliteValue::from` sites and roughly 50 `.map_or(Null, from)` bindings stay
    untouched.
  - `query_rows`: a **mandatory** eager `Vec<Row>` collector. fsqlite returns owned rows; rusqlite
    returns a lazy type that is not an `Iterator`.
  - `query_one`: remaps `QueryReturnedNoRows`.
  - `db_exec` and `params` pass-throughs.
  This file is deleted in Phase 8.
- Port the 26 non-storage consumers, trickiest first by reference density:
  `doctor_subsystems/mutate.rs` (27 refs, the frankensqlite-to-`serde_json` bridge), `logging.rs`
  (10), `config/mod.rs` (11, including `with_database_family_snapshot` which is pure file copy and
  survives unchanged), `delete.rs` (9), `doctor_subsystems/surface.rs` (8), `doctor.rs` (7), then the
  1-to-3 ref files.

**Exit criteria:** `cargo check --all-targets` reports errors in **only**
`src/storage/{sqlite,schema,events}.rs`. That is what proves nothing outside storage was missed.

### Phase 4: `schema.rs` and `events.rs` (4.5 days)

**Goal:** migrate the DDL and migration engine plus the audit log. This phase contains a real
deletion.

- Delete `split_sql_statements` (`schema.rs:402`) and the local `execute_batch` shim
  (`schema.rs:511`), about 110 lines total. Route all 25 call sites to `conn.execute_batch`,
  including the roughly 120-statement `SCHEMA_SQL`.
- Then grep the finished file for `execute("...;")`. rusqlite has rejected trailing semicolons and
  multi-statement strings since 0.35.0, and this fails at **runtime**, not compile time. The
  compiler will not catch it.
- Migrate `events.rs` (925 lines, 34 `SqliteValue` refs) with its transaction boundaries intact.

**Exit criteria:** `cargo test schema` and `cargo test events` green; no `split_sql_statements` or
local shim remains; the `SCHEMA_SQL` DDL applies cleanly on a fresh database and on each of the 13
fixtures.

### Phase 5: `sqlite.rs` core (4 days)

**Goal:** move the engine-facing surface where the semantics live, as distinct from the two phases
that are mostly mechanical type-path churn.

- `Connection::open` and `fsqlite::compat::open_with_flags` (14 sites).
- The pragma block at `:1059` and `:1157`, including the explicit `PRAGMA busy_timeout=0`.
- **Fix the Drop teardown first.** Delete `close_in_place` (`sqlite.rs:13568`), but change
  `conn: Connection` to `conn: Option<Connection>` and take-and-drop it in teardown, so
  `remove_temp_db_files` provably runs after the connection is closed. Without this, the Windows
  unlink fails and the `-wal`, `-shm`, and `-journal` sidecars leak into TMPDIR (#299). Add a comment
  stating the ordering requirement, because it is invisible in code.
- Port both retry loops in the same commit: `with_connection_write_transaction` (`:741`) and
  `with_write_transaction` (`:1484`). Copy `jittered_backoff` verbatim including its `i128` arithmetic
  that dodges `u64` to `i64` truncation. A plain `2u64.pow(attempt)` backoff would reintroduce
  thundering-herd lockstep retries across agents, which is the exact failure the function's doc
  comment says it exists to prevent.
- Add a test asserting a freshly opened connection reports `busy_timeout == 0`, so a future refactor
  that drops the pragma fails loudly.

**Exit criteria:** `cargo test storage` green; the busy-timeout assertion passes; both retry loops
proven to retry identically under injected `SQLITE_BUSY`; no temp-file residue on Windows.

### Phase 6: `sqlite.rs` read path (5 days)

**Goal:** absorb the single largest mechanical cost, working parser-by-parser rather than
site-by-site.

- 14 `*_from_row` parser functions, 268 `row.get` sites.
- Each parser funnels through six local closures (`get_str`, `get_opt_str`, `get_non_empty_str`,
  `get_opt_i32`, `get_bool`, `get_opt_datetime`). Rewriting one closure absorbs every site that
  uses it and produces a self-contained review unit. This is the reason to work parser-by-parser:
  it is the only unit where the review is tractable.
- Reimplement the three-way boolean fallback by hand rather than delegating to `FromSql for bool`.
  rusqlite accepts only INTEGER 0 or 1 and hard-errors on a legacy TEXT `'0'` or a `2`. Test with
  TEXT `'0'`, INTEGER `2`, and NULL asserting false, true, false.
- Preserve all five arms in `parse_datetime_value` (`:11600`) and `parse_opt_datetime_value`
  (`:11614`): Null, Text, Integer, Float, Blob.

**Exit criteria:** byte-identical golden snapshot comparison against the Phase 1 baseline. Any diff
is a silent shift and must be investigated, not accepted. `cargo test --test proptest_time_parsing`
green.

### Phase 7: `sqlite.rs` write path (6 days)

**Goal:** absorb the remaining bulk, under one governing rule: the engine's correctness workarounds
stay exactly as they are.

- 602 parameter sites.
- **Governing rule: keep every workaround.** At least 8 distinct documented frankensqlite defects are
  compensated: false PK conflicts on multi-VALUES reinsert, IN-clause DELETE bugs, unreliable UNIQUE
  upserts, unenforced non-rowid UNIQUE, false FK violations under page-buffer exhaustion (#215),
  btree cursor bugs on large DELETE, a hard crash on `SUM(CASE)`, and phantom btree entries after bulk
  DELETE. Grep each cited line and confirm both the code and its explanatory comment are intact. Any
  change is a review-blocking finding.
- Delete `test_diag_data_visibility` (`:20211`) and `test_diag_root_page_visibility` (`:20315`)
  without porting. They are the only `Statement::explain()` call sites in the repo (verified: exactly
  lines 20243 and 20264) and they probe the frankensqlite planner, not product behavior. Do not
  shell out to `EXPLAIN` to preserve them; VDBE program text is not a stable format. Deleting test
  functions is not deleting files, so `AGENTS.md` Rule 1 is not engaged, but note it in the commit
  message.

**Exit criteria:** all 602 sites ported; every documented workaround comment intact; the
`cargo test --all-features` failure count matches the Phase 1 baseline.

### Phase 8: Concurrency contract, full suite, supply-chain docs, atomic merge (5.5 days)

**Goal:** validate the part that is a genuine behavior change rather than a type change, land the
whole port as one atomic merge, and close the supply-chain loop.

- Run `tests/e2e_concurrency.rs` (11 tests) against real C SQLite. **This is the go/no-go for the
  whole plan.** See section 9.
- Measure the composed worst case end to end (30s `.write.lock` + engine timeout + app retry) and
  write the number into a doc comment near `DEFAULT_BUSY_TIMEOUT_MS` and into `docs/SYNC_SAFETY.md`.
- Update the supply-chain and architecture documentation listed in section 4.
- Record the MVCC negative result in `UPGRADE_LOG.md`: no `src/` file references MVCC, snapshot
  isolation, `TransactionBehavior`, or deferred transactions. All transactions are raw SQL strings
  (17 `BEGIN IMMEDIATE`, 2 `BEGIN EXCLUSIVE`). The actual cross-process gate is `.write.lock`, pure
  `std::fs::File::try_lock` with a 25ms poll and a 30s timeout. `fsqlite-mvcc` at `Cargo.toml:58`
  is a compile-graph artifact, not a used feature. The migration buys no multi-writer capability
  and loses none. Record this so no future reader assumes a concurrency regression that never
  existed, or credits a fix that was never needed.
- Fix test assertions by quoting the actual C SQLite message, never by loosening the matcher until
  it passes. `tests/e2e_concurrency.rs:221` asserts the lowercase string
  `"unique constraint failed: blocked_issues_cache.issue_id"` while C SQLite emits uppercase
  `UNIQUE`. Flag in the commit message any test that becomes trivially true under real WAL MVCC, so
  it is not counted as evidence the port is correct.
- Merge to `main` as one merge commit. Then `git push origin main` and `git push origin main:master`.

**Exit criteria:** `e2e_concurrency.rs` fully green; composed worst case documented; docs updated;
one merge commit with the 15 `fsqlite` lines removed.

---

## 8. Behavior changes

### Keep, with the comment corrected

**`DEFAULT_BUSY_TIMEOUT_MS = 0` (`sqlite.rs:43`, applied at `:1059` and `:1157`).**
Keep the value at 0. Two reasons.

First, the current doc comment is factually wrong for the version actually being built.
`Cargo.toml:46` pins `fsqlite = "0.1.7"` as a caret requirement, but `Cargo.lock` resolves
`0.1.19`, and in 0.1.19 the busy handling is sleep-based: `retry_busy_connection_bootstrap` runs
bounded `spin_loop()` counts and then `std::thread::sleep` on a 1ms to 50ms ladder. The starvation
that #243 actually hit is visible elsewhere, in the MVCC page-write contention loops, which
re-acquire the engine handle inside the retry loop. That mutex-retry-under-lock is the real hot-spin
vector. The workaround's effect is still load-bearing, but for a different reason than its comment
states. Leaving the comment as-is would be an active lie in the code.

Second, and more important: the new rationale is that the app's jittered exponential backoff
desynchronizes competing agents better than SQLite's fixed-delay table
({1,2,5,10,15,20,25,25,25,50,50,100}ms then 100ms). Failing fast at the engine and retrying at the
application layer is the better design, not a legacy artifact.

**The critical hazard runs the other way.** rusqlite defaults new connections to a 5000ms busy
timeout. If the explicit `PRAGMA busy_timeout=0` at `:1157` is dropped during the port, the composed
worst case goes from roughly 43s to roughly 114s per contended write, with no compile error and no
test failure. Treat `:1059` and `:1157` as reviewed, load-bearing lines in the port diff, and let
the new test assert the value.

**The two 8-attempt retry loops: keep verbatim, all parameters.**
`MAX_RETRIES = 8`, `base_backoff_ms = 50`, exponential 50ms to 6400ms plus 25% jitter, roughly
12.7s total. Gated on `is_transient()` at three points per loop: BEGIN, COMMIT, and the closure
body. After a transient COMMIT error the code explicitly does not return, it falls through to
retry. Rollback is always attempted and its failure is non-fatal. This is the load-bearing safety
net for in-process thread contention and for the read-only auto-import path that does not take
`.write.lock`; the cross-process case is already handled by the flock. Preserve the deliberate
asymmetry: the static variant lacks the `mutation_count` and PASSIVE-checkpoint block the method
variant has. Do not extract a shared helper.

**WAL checkpoint policy: no change.** `WAL_CHECKPOINT_INTERVAL = 50` with a PASSIVE checkpoint after
the 50th committed mutation, inside the retry loop, on the successful-commit path only. PASSIVE is
used deliberately over TRUNCATE because TRUNCATE requires an exclusive lock and was a major source
of "database is busy" under parallel agent operations (#219). `checkpoint_full` uses TRUNCATE only
at quiescent points where `.write.lock` is already held exclusively, and downgrades to PASSIVE on
failure. Do not enable `wal_autocheckpoint`: it must stay 0, because `with_write_transaction` does
manual PASSIVE checkpoints at a controlled interval, and re-enabling SQLite's 1000-page default
returns the latency-spike regression #219 was meant to fix.

**Foreign key posture: no net change, and that is the point.** The bundled build hard-codes
`-DSQLITE_DEFAULT_FOREIGN_KEYS=1`, but that only sets a **new** connection's initial state, and the
code already sets it explicitly to ON at open and explicitly OFF around bulk writes. The effective
enforcement posture is chosen by the code, not the engine, so the swap changes nothing on its own.
Do not let the pragma dance be removed as "unnecessary" during the port; that is a policy decision,
not a mechanical one. Preserve the disable and verified-restore sequences exactly, and record the
decision in a doc comment on `restore_foreign_keys` (`schema.rs:1072`).

### Improvements, and how to sequence them

**UNIQUE enforcement becomes real.**
`sqlite.rs:2402-2403` documents that frankensqlite does not enforce UNIQUE on non-rowid columns. C
SQLite does. The application-level checks become belt-and-braces. Keep every workaround during the
port, then remove them one per commit after the merge, each with its own test. Add a new test
proving C SQLite now rejects a duplicate `issues.id`, so a future reader understands the hand-rolled
check is no longer load-bearing.

### Silent-failure hazards specific to the port

**`FrankenError::is_transient()` is a 7-way frankensqlite-specific classifier, and 4 of its 7 members
have no C equivalent.** Only `Busy` and `BusyRecovery` (SQLITE_BUSY = 5) and `DatabaseLocked`
(SQLITE_LOCKED = 6) are reachable under C SQLite. `BusySnapshot` is frankensqlite's MVCC
snapshot-conflict code, `WriteConflict` and `SerializationFailure` are its SSI and page-patch
internals, and `PageBufferCapacityExhausted` is a frankensqlite resource bug. The whole concurrency
model gates on this one predicate at 7 production call sites. Worse, `BeadsError::is_transient()`
(`error/mod.rs:210`) also returns true for `Self::Io` of kind Interrupted, TimedOut, or WouldBlock,
so removing the `FrankenError` arm without care silently changes retry semantics for non-database
I/O errors too, with no compile error.

**Row type inference is lost at 268 sites.** rusqlite's `row.get::<Option<String>, _>(idx)` is
generic, returns `Result` rather than `Option<&Value>`, and has no `Option<&str>` or `Option<i64>`
accessor, so each of the 250 production sites needs a hand-written type annotation and error arm.
The 1090-constructor half is only mechanical if the `SqlValue` newtype is kept. The danger is that
SQLite is dynamically typed: rusqlite's typed `get` compiles even when a row's storage class shifts
(TEXT `'2'` read as Integer), and the 14 positional `*_from_row` parsers reach column 41, so a
silent column or type drift is undetectable at the type level. Hence the byte-identical golden gate.

**Datetime storage classes are the highest-risk silent-corruption site.** The current reader matches
five storage classes on purpose, including numeric ones that legacy data contains. The doc comment
on `parse_datetime_value` records that a previous `as_text().unwrap_or("")` reader silently mapped
empty to `UNIX_EPOCH` and corrupted the value on export. A dropped arm repeats that failure. Gate on
`cargo test --test proptest_time_parsing`.

**`FromSql for bool` is stricter than the current fallback.** The existing `get_bool` closures do a
three-way fallback: missing means false, 0 means false, nonzero means true, and `row_bool` at
`schema.rs:2259-2266` additionally treats text as `text != "0"`. rusqlite's `FromSql for bool`
accepts only INTEGER 0 or 1. This applies to the `ephemeral`, `pinned`, and `is_template` columns
and to the `get_bool` closure inside every `*_from_row` parser.

**`conn.execute` rejects trailing semicolons and multi-statement strings since rusqlite 0.35.0, at
runtime only.** `schema.rs` alone carries 86 DDL statements plus 25 script invocations.

**Drop teardown ordering is invisible in code.** See Phase 5.

---

## 9. The one untested assumption

**A `rusqlite` 0.40.x regression in WAL plus `BEGIN IMMEDIATE` under heavy multi-process writer
contention would invalidate this plan.** Everything else in this document was verified; this was
not.

The mitigating factor is that the cross-process gate is `.beads/.write.lock`, pure
`std::fs::File::try_lock` with zero frankensqlite involvement, so the engine rarely sees
`SQLITE_BUSY` from another process. In-process thread contention is handled by the app retry loop.

But "rarely" is not "never", and `tests/e2e_concurrency.rs` has 11 tests that must be run against
real C SQLite before this document is a plan rather than a hope. That is Phase 8, and it is a
go/no-go gate, not a checkbox. If the tests fail in a way that indicates an engine-level WAL defect
rather than a port bug, the correct response is to fall back to system-linked SQLite or to raise the
issue upstream, not to patch around it.

---

## 10. First 10 actionable items

1. Write `tests/storage_engine_compat.rs`: open all 11 `sample_beads_db_files/*/beads.db` plus the
   2 repro fixtures under rusqlite, assert `PRAGMA integrity_check == 'ok'` and
   `SELECT COUNT(*) FROM issues > 0` for each. Half a day. This is the on-disk-format go/no-go.
2. Freeze the baseline before touching engine code: run `rch exec -- cargo test --all-features` and
   `rch exec -- cargo clippy --all-targets -- -D warnings` on unmodified `main`. Record exact pass
   and fail counts and the current release binary size.
3. Swap the manifest as a pure dependency probe: delete the 15 declarations at `Cargo.toml:45-59`,
   add the `rusqlite` line, regenerate `Cargo.lock`. Nothing imports rusqlite yet, so this compiles
   only the dependency.
4. Run `rch exec -- cargo build --release` on all 8 workers. The single gating infra check.
5. Build the differential parity corpus and goldens: roughly 60 fixed CLI invocations capturing
   stdout, stderr, and exit code, plus `sqlite_master` and `PRAGMA table_info` dumps for every table.
6. Port the error taxonomy: change `src/error/mod.rs:42` and reimplement `is_transient()` at `:210`
   against `sqlite_error_code()`. Never string-match. Do this before any volume code depends on it;
   the failure mode is silent.
7. Create `src/storage/db.rs` with `SqlValue`, `query_rows`, `query_one`, and the `db_exec` / `params`
   pass-throughs. It is deleted in Phase 8.
8. Port the 26 non-storage consumers, trickiest first. Exit criterion: `cargo check --all-targets`
   reports errors in **only** `src/storage/{sqlite,schema,events}.rs`.
9. Delete `split_sql_statements` (`schema.rs:402`) and the local `execute_batch` shim (`:511`), route
   all 25 call sites to `conn.execute_batch`, then grep the finished file for `execute("...;")`.
10. Fix the Drop teardown before porting anything that depends on it: change
    `conn: Connection` to `conn: Option<Connection>`, take-and-drop in teardown, and add a Windows
    test asserting `remove_temp_db_files` leaves no residue.

---

## 11. Findings that research got wrong

Recorded so nobody re-derives them. Each was surfaced by an adversarial verification pass and
corrected against the local tree.

| Claim | Correction |
|---|---|
| The migration is build-blocked on C-toolchain provisioning across the 8 RCH workers, the release pipeline, and the Windows dev box. | False for this repo. C is already an in-tree requirement via `mimalloc` at `Cargo.toml:127-128` and `libmimalloc-sys` to `cc`. All 8 Linux RCH workers already need a C compiler. Marginal build time, not a blocker. |
| `rusqlite#1735` is closed by the `cc = "1.2.27"` floor, so pin `cc` and prove cross builds early. | The issue is still open and the citation is inverted: the maintainer-recommended workaround is downgrading `cc` to 1.2.14, and the floor exists specifically to block that. `Cargo.lock` already resolves `cc` 1.4.0, so pinning is a no-op. Moot regardless, because every release job is a native runner. |
| Existing on-disk `.beads/*.db` files already contain UNIQUE and FK-violating rows, so a bespoke audit-and-repair data migration is required. | The engine-behavior delta is real, but all three load-bearing conclusions are false. The check at `:2402` runs inside `BEGIN IMMEDIATE` so it is not TOCTOU-racy. The DELETE+INSERT sequences cannot self-collide, and a mid-sequence abort rolls back the whole transaction. `.beads/beads.db` is a gitignored derived cache absent from the tree, and `config`/`metadata` deliberately declare no constraints. A cache rebuild is sufficient. |
| The only reason to leave frankensqlite is a non-standard license. | A real and serious reason, but not the only one, and the strongest evidence is elsewhere. The rider is already surfaced at `README.md:1008` and `CHANGELOG.md:698`, so it is a known standing issue rather than a discovery. The independent second reason is durability: 35 published versions in 7 months, and v0.3.9 was an expedited patch because v0.3.8 shipped a CLI that could not open file databases at all. |
| Dropping the roughly 50 frankensqlite workarounds is required and low-risk because C SQLite enforces the constraints properly. | Inverted. Doing it in the same change that swaps the engine deletes the workaround and the constraint it compensates for simultaneously. Sequence the swap with every workaround intact, measure, then strip one per commit with a test each. |
| `busy_timeout = 0` is actively harmful under C SQLite and must be raised as part of this migration. | Overstated. SQLite's fixed-delay table would synchronize competing agents, whereas the existing jittered exponential backoff desynchronizes them, so fail-fast-at-engine plus retry-at-app is a coherent design. The real hazard is the opposite one and it was missed: rusqlite defaults connections to 5000ms, so dropping the explicit pragma silently moves the composed worst case from roughly 43s to 114s. Keep 0, keep the explicit set, rewrite the comment. |
| `Cargo.toml` declares 16 `fsqlite` crates that must be swapped in one atomic edit with the RCH sync manifest. | Miscount. It is 15, at `Cargo.toml:45-59`, verified locally. The atomic-manifest hazard also shrinks, because those entries are explicit only so "rch syncs them" and `rusqlite` resolves from crates.io with no sibling checkout. |
| `fsqlite-mvcc` is in use and provides concurrent-writer capability the migration would lose. | A negative finding, verified. No `src/` file references MVCC, snapshot isolation, `TransactionBehavior`, or deferred transactions. All transactions are raw SQL strings. `.write.lock` is pure `std::fs::File::try_lock`. The crate is a compile-graph artifact, not a used feature. The migration buys no multi-writer capability and loses none. |
| libclang and bindgen are required by `libsqlite3-sys`, adding a heavy build dependency. | Only under `buildtime_bindgen`, and `bundled` avoids it: `bundled = ["libsqlite3-sys?/bundled", "modern_sqlite"]` and `modern_sqlite = ["libsqlite3-sys?/bundled_bindings"]`, so `bundled` transitively enables the in-crate pregenerated bindings. The guardrail is simply to keep `session` and `preupdate_hook` off. |
| The `cache` feature is opt-in and forgetting it is a silent perf regression. | Half right, wrong emphasis. `default = ["cache", "ffi-sqlite-wasm-rs"]`, so `cache` moved into the default set in 0.38 and is lost only because this plan passes `default-features = false`. Listing it explicitly is correct and self-documenting, but it is a hygiene measure, not a fix for an active failure mode. |

---

## 12. Evidence appendix

### Locally verified

- 15 `fsqlite` crates at `Cargo.toml:45-59`, and `Cargo.lock` resolving `fsqlite` to 0.1.19 from the
  `0.1.7` caret requirement.
- `mimalloc` under `cfg(not(windows))` at `Cargo.toml:127-128`.
- `unsafe_code = "forbid"` at `Cargo.toml:183` and `src/lib.rs:22`.
- `DEFAULT_BUSY_TIMEOUT_MS: u64 = 0` at `sqlite.rs:43`, applied at `:1059` and `:1157`.
- Both retry loops with `MAX_RETRIES = 8` and `base_backoff_ms = 50` at `:749-750` and `:1494-1495`.
- `BeadsError::is_transient` delegating to `e.is_transient()` at `error/mod.rs:210`.
- `close_in_place()` at `sqlite.rs:13568`, with the comment stating the ordering requirement.
- `split_sql_statements` at `schema.rs:402` and the local `execute_batch` shim at `schema.rs:511`.
- `pub trait Storage` at `src/storage/trait_.rs:28`.
- All 11 `sample_beads_db_files/*/beads.db` fixtures beginning with the bytes `SQLite format 3`.
- `.beads/beads.db` absent from the tree, with `issues.jsonl` as the canonical tracked source.
- `git show d3d9bce6^:Cargo.toml:22` reading the historical `rusqlite` line.

### Upstream verified

- `rusqlite` 0.40.2 published 2026-08-08 is the current latest.
- `default = ["cache", "ffi-sqlite-wasm-rs"]`; `bundled` implies `modern_sqlite` implies
  `bundled_bindings`; `fallible_uint = []`; `session` implies `preupdate_hook` implies
  `buildtime_bindgen`.
- MSRV 1.88.0, edition 2024, MIT.
- `libsqlite3-sys`: `bundled = ["cc", "bundled_bindings"]` and `bundled_bindings = []`, with
  `build.rs` copying pregenerated bindings into `OUT_DIR`.

### Historical note

`fsqlite` was adopted in this repository to replace an earlier `rusqlite` dependency. The previous
configuration is recoverable at `d3d9bce6^:Cargo.toml:22` and has not been exercised since. Assume
its 0.38.0 API surface matches 0.40.2 and verify during Phase 1 rather than assuming it.
