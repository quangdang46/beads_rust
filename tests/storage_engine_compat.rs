//! On-disk compatibility gate for the fsqlite (frankensqlite) -> rusqlite migration.
//!
//! The migration's central on-disk claim is that frankensqlite writes genuine `SQLite format 3`
//! files that C SQLite can read with no data migration. Section 3 of
//! `docs/RUSQLITE_MIGRATION_PLAN_2026_09_25.md` asserts this and defers the content claim to
//! Phase 1. This file is the instrument that settles it.
//!
//! # The result: the format claim held, the content claim did not
//!
//! Measured 2026-09-26, under `rusqlite` 0.40.2 and independently under Python's `sqlite3`
//! (a different C SQLite binding); both agree.
//!
//! * **10** of the 13 fixtures open clean and non-empty.
//! * **1** opens, but `PRAGMA integrity_check` returns 99 violations.
//! * **2 cannot be opened at all.**
//!
//! The two hard failures are:
//!
//! ```text
//! malformed database schema (idx_blocked_cache_blocked_at)
//!   - no such table: main.blocked_issues_cache
//! malformed database schema (idx_issues_status) - no such table: main.issues
//! ```
//!
//! C SQLite parses `sqlite_master` in **rowid order**. frankensqlite wrote INDEX rows at rowids
//! LOWER than the TABLE row they reference, so C SQLite reaches the index definition before the
//! table it names exists. Measured: `idx_issues_*` at rowids 2-16 while `issues` is at 47; in
//! `beads_rust`, `idx_blocked_cache_blocked_at` at 4219 while `blocked_issues_cache` is at 4220,
//! and that index is *duplicated* at 4222 by the stale-schema-cache re-emit that `execute_batch`'s
//! `is_index && is_stale_schema` skip (`src/storage/schema.rs:527-532`) swallows.
//!
//! Both engines write valid SQLite 3 containers, so this is a frankensqlite **write-path** bug
//! that C SQLite's stricter schema parser exposes, not a format difference.
//!
//! This project's own live `.beads/beads.db` was checked and is clean (`integrity_check = ok`,
//! 951 issues), so the defect is possible rather than universal, and section 3's "the cache is
//! disposable, rebuild from `issues.jsonl`" argument still holds. What does **not** hold today is
//! that the rebuild is automatic: the failure arrives as an open-time schema-parse error, a shape
//! the current open path does not recognise. Section 16.4 of the plan specifies the fallback.
//!
//! # Read-only by construction
//!
//! Every connection is opened as `file:<ABSOLUTE_PATH>?immutable=1` with
//! `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI`. `immutable=1` is the load-bearing half: SQLite
//! treats the file as immutable, so there is no locking, no WAL recovery, no `-shm` creation, and
//! no byte of a checked-in fixture is ever mutated. `SQLITE_OPEN_READ_ONLY` alone is **not**
//! sufficient for this corpus -- all 13 fixtures are WAL mode, and a read-only WAL open requires
//! creating a `-shm` that cannot be created, so 10 of them fail `SQLITE_CANTOPEN` for a reason
//! that has nothing to do with schema validity.
//!
//! # Expectations encode measurement, not aspiration
//!
//! `Expect` below records the state observed on `feat/rusqlite-migration` at `6ff3acbe`. The plan's
//! stated exit criterion, "all 13 fixtures open with `integrity_check = ok`", is unmeetable. If a
//! fixture is ever regenerated and starts opening clean, this test **fails and says so** -- that is
//! the intended direction of travel, and fixing it means updating the expectation deliberately
//! rather than discovering it in production.
//!
//! # Not run under `cargo package` / `cargo publish`
//!
//! `Cargo.toml` `exclude` lists `sample_beads_db_files/`, so a published crate has no corpus.
//! `corpus_present()` early-returns with an explanatory note instead of failing.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

/// What C SQLite is expected to do with a fixture. Measured, not assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// Opens; `PRAGMA integrity_check` == `["ok"]`; `issues` has at least one row.
    Clean,
    /// Opens; `issues` has rows; `PRAGMA integrity_check` returns more than just `["ok"]`.
    /// The assertion is that it is non-empty and does NOT start with a parse failure -- the exact
    /// count is reported but not pinned, because `integrity_check` line count is a property of the
    /// data, not of the engine swap, and pinning it would make this test a data change detector.
    IntegrityFindings { observed: usize },
    /// The 16-byte header is a valid SQLite 3 database, but C SQLite refuses the whole file during
    /// schema parse because an `index` is stored at a `sqlite_master` rowid lower than the `table`
    /// row it references. The error message names both objects.
    MalformedSchema {
        index: &'static str,
        table: &'static str,
    },
}

struct Fixture {
    /// Path relative to the crate root, forward slashes, literal.
    rel: &'static str,
    expect: Expect,
    /// `SELECT COUNT(*) FROM issues` at authoring time. Reported for context; the gate is
    /// "readable and non-empty", so a fixture regenerated smaller must not fail the suite.
    observed_issues: Option<i64>,
}

const FIXTURES: &[Fixture] = &[
    // --- the 11 project databases: sample_beads_db_files/<name>/beads.db ---
    Fixture { rel: "sample_beads_db_files/asupersync/beads.db", expect: Expect::Clean, observed_issues: Some(3378) },
    Fixture {
        rel: "sample_beads_db_files/beads_rust/beads.db",
        expect: Expect::MalformedSchema { index: "idx_blocked_cache_blocked_at", table: "blocked_issues_cache" },
        observed_issues: None,
    },
    Fixture { rel: "sample_beads_db_files/flywheel_connectors/beads.db", expect: Expect::Clean, observed_issues: Some(970) },
    Fixture { rel: "sample_beads_db_files/franken_whisper/beads.db", expect: Expect::Clean, observed_issues: Some(170) },
    Fixture { rel: "sample_beads_db_files/frankensqlite/beads.db", expect: Expect::Clean, observed_issues: Some(1484) },
    Fixture { rel: "sample_beads_db_files/frankenterm/beads.db", expect: Expect::Clean, observed_issues: Some(1727) },
    Fixture { rel: "sample_beads_db_files/frankentui/beads.db", expect: Expect::Clean, observed_issues: Some(2590) },
    Fixture { rel: "sample_beads_db_files/mcp_agent_mail_rust/beads.db", expect: Expect::Clean, observed_issues: Some(1600) },
    Fixture { rel: "sample_beads_db_files/mcp_agent_mail_website/beads.db", expect: Expect::Clean, observed_issues: Some(160) },
    Fixture { rel: "sample_beads_db_files/ntm/beads.db", expect: Expect::Clean, observed_issues: Some(1947) },
    Fixture { rel: "sample_beads_db_files/remote_compilation_helper/beads.db", expect: Expect::Clean, observed_issues: Some(733) },
    // --- the 2 repro fixtures. NOTE the extra `.beads/` level: a single
    // `sample_beads_db_files/*/beads.db` glob yields 11 paths, not 13. ---
    Fixture {
        rel: "sample_beads_db_files/repro_beadsrust_import_write.M6eaGY/.beads/beads.db",
        expect: Expect::IntegrityFindings { observed: 99 },
        observed_issues: Some(551),
    },
    Fixture {
        rel: "sample_beads_db_files/repro_frankensqlite_import_write.ljw6cl/.beads/beads.db",
        expect: Expect::MalformedSchema { index: "idx_issues_status", table: "issues" },
        observed_issues: None,
    },
];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn corpus_present() -> bool {
    crate_root().join("sample_beads_db_files").is_dir()
}

/// Open a fixture so that no byte of it can be modified.
///
/// `immutable=1` is what makes this safe for WAL-mode fixtures: SQLite skips locking, WAL recovery
/// and `-shm` creation entirely, so the on-disk family is never touched.
fn open_readonly_immutable(path: &Path) -> rusqlite::Result<Connection> {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let uri = format!("file:{}?immutable=1", abs.display());
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
}

/// Open a fixture and force the schema to be parsed.
///
/// The schema is parsed **lazily**: `Connection::open_with_flags` on these fixtures returns `Ok`
/// even when the schema is unusable, and the refusal only surfaces on the first statement that
/// needs a resolved table name. Probing with `PRAGMA integrity_check` is deliberate, because it is
/// both the force and the thing this test actually wants to know. Treating a bare `open` as the
/// gate would classify these two fixtures as "opened fine".
fn open_and_parse(path: &Path) -> rusqlite::Result<Connection> {
    let conn = open_readonly_immutable(path)?;
    // Force schema resolution; the error, if any, is returned to the caller.
    conn.prepare("SELECT COUNT(*) FROM issues")?;
    Ok(conn)
}

#[test]
fn frankensqlite_fixtures_behave_as_measured_under_c_sqlite() {
    if !corpus_present() {
        eprintln!(
            "skipping: sample_beads_db_files/ is absent (excluded from the published crate by \
             Cargo.toml `exclude`). The on-disk compatibility gate only runs in a repo checkout."
        );
        return;
    }

    let root = crate_root();
    let mut clean = 0usize;
    let mut degraded = 0usize;
    let mut refused = 0usize;

    for fixture in FIXTURES {
        let path = root.join(fixture.rel);
        assert!(
            path.exists(),
            "fixture missing: {} (the corpus is excluded from the published crate, so a missing \
             file inside a checkout means the file was deleted, not that packaging is in play)",
            fixture.rel
        );

        match open_and_parse(&path) {
            Err(err) => {
                // The open failed. This is the only outcome that proves the migration's on-disk
                // claim is incomplete, so it must match precisely.
                let Expect::MalformedSchema { index, table } = fixture.expect else {
                    panic!(
                        "FIXTURE CHANGED: {} opened but was expected to fail.\n\
                         If this fixture was regenerated and now opens cleanly, that is progress --\n\
                         update its `Expect` deliberately rather than loosening the assertion.\n\
                         C SQLite said: {err}",
                        fixture.rel
                    );
                };
                let msg = err.to_string();
                assert!(
                    msg.contains("malformed database schema"),
                    "{}: expected a schema-parse refusal, got a different error: {err}\n\
                     A different failure mode would mean the migration has a NEW problem, not a \
                     known one.",
                    fixture.rel
                );
                assert!(
                    msg.contains(index) && msg.contains(table),
                    "{}: expected the refusal to name index `{index}` and table `{table}`, got: {msg}",
                    fixture.rel
                );
                refused += 1;
                eprintln!("REFUSED   {}  ({index} precedes {table})", fixture.rel);
            }
            Ok(conn) => {
                let issues = conn
                    .query_row("SELECT COUNT(*) FROM issues", [], |row| row.get::<_, i64>(0))
                    .unwrap_or_else(|e| {
                    panic!("{}: opened but `SELECT COUNT(*) FROM issues` failed: {e}", fixture.rel)
                });
                assert!(
                    issues > 0,
                    "{}: opened but has zero issues rows; the corpus is supposed to be populated, \
                     and a non-empty read is half of what this gate proves",
                    fixture.rel
                );

                // `PRAGMA integrity_check` returns one row per finding, so it has to be walked as a
                // statement; `query_row` would only ever see the first.
                let findings: Vec<String> = {
                    let mut stmt = conn
                        .prepare("PRAGMA integrity_check")
                        .unwrap_or_else(|e| panic!("{}: prepare integrity_check: {e}", fixture.rel));
                    let rows = stmt
                        .query_map([], |row| row.get::<_, String>(0))
                        .unwrap_or_else(|e| panic!("{}: query integrity_check: {e}", fixture.rel));
                    rows.collect::<rusqlite::Result<Vec<String>>>()
                        .unwrap_or_else(|e| panic!("{}: read integrity_check: {e}", fixture.rel))
                };

                match fixture.expect {
                    Expect::Clean => {
                        assert_eq!(
                            findings,
                            vec!["ok".to_string()],
                            "{}: was expected to be clean, integrity_check said {findings:?}",
                            fixture.rel
                        );
                        clean += 1;
                        eprintln!(
                            "CLEAN     {}  {issues} issue rows (observed {} at authoring time)",
                            fixture.rel,
                            fixture.observed_issues.unwrap_or(-1)
                        );
                    }
                    Expect::IntegrityFindings { observed } => {
                        assert!(
                            !findings.iter().all(|f| f == "ok"),
                            "{}: was expected to have integrity findings but reported clean. If \
                             the fixture was regenerated, update its `Expect` deliberately.",
                            fixture.rel
                        );
                        assert!(
                            !findings[0].contains("malformed database schema"),
                            "{}: an openable fixture is now failing schema parse, which is a NEW \
                             failure mode rather than the known data-state one: {:?}",
                            fixture.rel,
                            findings[0]
                        );
                        // Reported, not asserted: the finding count is a property of the fixture's
                        // data, and pinning it would turn this into a data-change detector.
                        eprintln!(
                            "DEGRADED  {}  {} issue rows, {} integrity findings (observed {observed} \
                             at authoring time), first: {}",
                            fixture.rel,
                            issues,
                            findings.len().saturating_sub(1),
                            findings[0]
                        );
                        degraded += 1;
                    }
                    Expect::MalformedSchema { index, table } => {
                        panic!(
                            "FIXTURE CHANGED: {} now OPENS, but was expected to fail with a \
                             schema-parse refusal naming `{index}` and `{table}`.\n\
                             This is the intended direction of travel -- a regenerated fixture is \
                             readable again -- so update its `Expect` deliberately. It now reports \
                             {issues} issue rows and integrity_check {findings:?}.",
                            fixture.rel
                        );
                    }
                }
            }
        }
    }

    // The corpus is 13 fixtures: 11 at `sample_beads_db_files/<name>/beads.db` and 2 at
    // `sample_beads_db_files/repro_*/.beads/beads.db`.
    assert_eq!(FIXTURES.len(), 13, "corpus size changed; update the expectations above");
    assert_eq!(clean, 10, "expected exactly 10 fixtures to open clean, saw {clean}");
    assert_eq!(degraded, 1, "expected exactly 1 fixture to open with findings, saw {degraded}");
    assert_eq!(refused, 2, "expected exactly 2 fixtures to be refused, saw {refused}");
}

/// The refusal must be a *schema parse* failure, not corruption of the file container.
///
/// This is the distinction that decides whether section 3's "no data migration required" survives.
/// Both engines write valid SQLite 3 files, so if the 16-byte header is right and the pages parse,
/// the data is intact and the only thing wrong is the order of `sqlite_master` rows -- which means
/// the rows are still recoverable, and a rebuild from `issues.jsonl` is a valid remedy rather than
/// data loss.
#[test]
fn refused_fixtures_still_have_intact_sqlite_containers() {
    if !corpus_present() {
        return;
    }

    let root = crate_root();
    for fixture in FIXTURES.iter().filter(|f| matches!(f.expect, Expect::MalformedSchema { .. })) {
        let path = root.join(fixture.rel);
        let bytes = std::fs::read(&path).expect("read fixture");

        assert!(
            bytes.starts_with(b"SQLite format 3\0"),
            "{}: header is not a SQLite 3 database, so the container itself is damaged and the \
             'rebuild from JSONL' remedy would be masking real data loss",
            fixture.rel
        );

        // Page size is a big-endian u16 at offset 16, where the value 1 means 65536. Offsets 18
        // and 19 are the write and read format versions, NOT part of the page size.
        assert!(
            bytes.len() >= 18,
            "{}: file is only {} bytes, too short to hold a page size",
            fixture.rel,
            bytes.len()
        );
        let raw = u16::from_be_bytes([bytes[16], bytes[17]]);
        let page_size = u32::from(if raw == 1 { 65536 } else { u32::from(raw) });
        assert!(
            (512..=65536).contains(&page_size) && page_size.is_power_of_two(),
            "{}: implausible page size {raw}, so the header is not trustworthy",
            fixture.rel
        );
        assert_eq!(
            bytes.len() % page_size as usize,
            0,
            "{}: file length {} is not a whole number of {page_size}-byte pages, so the file is \
             truncated or corrupt",
            fixture.rel,
            bytes.len()
        );

        // The schema text is still in the file, which is what makes the data recoverable.
        let haystack = String::from_utf8_lossy(&bytes[..bytes.len().min(1 << 20)]);
        assert!(
            haystack.contains("CREATE TABLE"),
            "{}: no CREATE TABLE text found in the first MiB. The tables are genuinely absent, \
             which is a different and worse condition than a misordered sqlite_master.",
            fixture.rel
        );
    }
}
