//! `br.doctor.capabilities.v1` — machine-readable doctor contract.
//!
//! WP1 emits a *foundation* capabilities document describing the
//! contract surface AI agents can rely on. Concretely:
//!
//! - `doctor_version` — `CARGO_PKG_VERSION`
//! - `contract_version` — `"1"` (frozen for WP1)
//! - `exit_codes` — derived from [`super::exit_codes::DoctorExitCode::all`]
//! - `write_scopes` — `.beads/`, `.doctor/`
//! - `env_vars` — environment variables the doctor honors
//! - `fixers` — currently wired repair/refuse paths
//! - `detectors` — currently wired flat-doctor check IDs
//!
//! Stability: the JSON shape is stable contract. New fields are
//! purely additive; agents must tolerate unknown keys.

#![allow(dead_code)] // WP1 foundation; consumed by WP3-WP12.

use std::path::Path;

use serde::Serialize;

use super::exit_codes::DoctorExitCode;

/// Single exit-code dictionary entry.
#[derive(Debug, Clone, Serialize)]
pub struct ExitCodeEntry {
    /// Numeric exit value.
    pub code: i32,
    /// Stable kebab-case name.
    pub name: &'static str,
    /// Short human description.
    pub description: &'static str,
}

/// Top-level capabilities document.
#[derive(Debug, Clone, Serialize)]
pub struct DoctorCapabilities {
    /// Always `"br.doctor.capabilities"`.
    pub schema: &'static str,
    /// Always `"1"` for the WP1 contract.
    pub contract_version: &'static str,
    /// `CARGO_PKG_VERSION` of the running binary.
    pub doctor_version: &'static str,
    /// The structured exit-code dictionary.
    pub exit_codes: Vec<ExitCodeEntry>,
    /// Workspace-relative directories the doctor may write under
    /// `--repair`. WP1 ships `.beads/` and `.doctor/`; future WPs may
    /// extend.
    pub write_scopes: Vec<String>,
    /// Environment variables the doctor honors. Documented in
    /// `playbook.md` §1.4.
    pub env_vars: Vec<&'static str>,
    /// Fixer registry populated from the currently wired repair/refuse paths.
    pub fixers: Vec<FixerEntry>,
    /// Detector registry populated from the currently wired flat-doctor checks.
    pub detectors: Vec<DetectorEntry>,
    /// Names of every [`super::mutate::Op`] variant the chokepoint
    /// supports plus the parameters each takes. Stable contract: the
    /// names are kebab-case to match `actions.jsonl` `op` values, the
    /// `params` array enumerates the field names a fixer must supply.
    pub ops_supported: Vec<OpEntry>,
}

/// One row in the supported-ops list. Matches the variants in
/// [`super::mutate::Op`] one-for-one so the capabilities envelope
/// surfaces the exact contract a fixer can call into.
#[derive(Debug, Clone, Serialize)]
pub struct OpEntry {
    /// Stable kebab-case name (matches `actions.jsonl` `op` values).
    pub name: &'static str,
    /// Human-readable summary.
    pub summary: &'static str,
    /// Field names the variant requires (informational; agents should
    /// still read the Rust source for the authoritative shape).
    pub params: Vec<&'static str>,
    /// `true` when the op is wired all the way through the chokepoint;
    /// `false` flags ops that ship with safety scaffolding only (e.g.
    /// [`super::mutate::Op::DbMigrate`] until the schema.rs hook lands).
    pub fully_routed: bool,
}

/// One row in the fixer registry. Stable shape.
#[derive(Debug, Clone, Serialize)]
pub struct FixerEntry {
    pub id: String,
    pub subsystem: String,
    pub auto_fixable: bool,
    pub mutates: bool,
    pub addressed_findings: Vec<String>,
}

/// One row in the detector registry. Stable shape.
#[derive(Debug, Clone, Serialize)]
pub struct DetectorEntry {
    pub id: String,
    pub subsystem: String,
    pub severity_default: String,
    pub fast_path: bool,
}

impl DoctorCapabilities {
    /// Build a fresh capabilities document. Pure (no I/O).
    #[must_use]
    pub fn build() -> Self {
        let exit_codes = DoctorExitCode::all()
            .iter()
            .map(|code| ExitCodeEntry {
                code: code.as_i32(),
                name: code.as_str(),
                description: code.description(),
            })
            .collect();

        Self {
            schema: "br.doctor.capabilities",
            contract_version: "1",
            doctor_version: env!("CARGO_PKG_VERSION"),
            exit_codes,
            write_scopes: vec![".beads/".into(), ".doctor/".into()],
            env_vars: vec![
                super::run_dir::ENV_RUNS_DIR,
                "BR_NO_AUTOFLUSH",
                "BD_NO_AUTOFLUSH",
                "RUST_LOG",
                "BD_DB",
                "BEADS_JSONL",
            ],
            fixers: build_fixer_registry(),
            detectors: build_detector_registry(),
            ops_supported: vec![
                OpEntry {
                    name: "write_file",
                    summary: "Atomic create-or-overwrite via tmpfile + rename(2).",
                    params: vec!["content", "mode"],
                    fully_routed: true,
                },
                OpEntry {
                    name: "append_file",
                    summary: "Append-only write; creates the file if missing.",
                    params: vec!["content"],
                    fully_routed: true,
                },
                OpEntry {
                    name: "rename",
                    summary: "Move a path within write_scopes (no Op::Delete by design).",
                    params: vec!["to"],
                    fully_routed: true,
                },
                OpEntry {
                    name: "chmod",
                    summary: "Set the mode bits of a path.",
                    params: vec!["mode"],
                    fully_routed: true,
                },
                OpEntry {
                    name: "symlink_atomic",
                    summary: "Replace the symlink at path with one pointing at target.",
                    params: vec!["target"],
                    fully_routed: true,
                },
                OpEntry {
                    name: "db_exec",
                    summary: "Run a parameterized SQL statement inside BEGIN IMMEDIATE; \
                              snapshots affected rows to backups/db/ before COMMIT.",
                    params: vec!["sql", "args", "affected_tables", "affected_predicate"],
                    fully_routed: true,
                },
                OpEntry {
                    name: "db_migrate",
                    summary: "Versioned schema migration. Verifies PRAGMA user_version \
                              matches `from`, snapshots the DB file verbatim to \
                              backups/db/beads.db.pre-migrate, then drives \
                              schema::run_migrations_atomic inside BEGIN IMMEDIATE / COMMIT \
                              (rolls back + restores from snapshot on failure).",
                    params: vec!["from", "to"],
                    fully_routed: true,
                },
            ],
        }
    }

    /// Render to pretty JSON (stable key order via `serde_json` map
    /// iteration).
    ///
    /// # Errors
    ///
    /// Returns [`serde_json::Error`] on serialization failure (effectively
    /// never with the data shapes used here).
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Convenience: render against an arbitrary repo root for callers
    /// that want absolute scope paths. Currently a no-op aside from
    /// validation. Reserved for future extension.
    #[must_use]
    pub fn build_for_repo(_repo_root: &Path) -> Self {
        Self::build()
    }
}

/// Detector registry — populated from the canonical `check_*` family
/// in `src/cli/commands/doctor.rs`. Phase 10 cold-prober finding
/// (`beads_rust-3idn`): a cold agent reading
/// `br doctor capabilities --format json` previously saw `detectors:
/// []` and read it as "the contract is half-wired". This list pins
/// the agent-visible name of every detector the flat `br doctor`
/// surface runs, so consumers can build allow-lists, `--only`/`--skip`
/// selectors (future), and `--quick` parity tables against it.
///
/// `fast_path: true` flags the detectors the `--quick` mode keeps
/// (cheap stat / parse / single-PRAGMA checks); `false` flags the
/// ones `--quick` drops for sub-second pre-commit latency.
fn build_detector_registry() -> Vec<DetectorEntry> {
    // (id, subsystem, severity_default, fast_path)
    let rows: &[(&str, &str, &str, bool)] = &[
        ("beads_dir", "configs", "error", true),
        ("metadata", "configs", "error", true),
        ("gitignore.beads_inner", "configs", "warn", true),
        ("gitignore.root", "configs", "warn", true),
        ("routes_jsonl", "routes_external", "warn", true),
        ("routes.targets", "routes_external", "warn", true),
        ("rust_log", "observability", "warn", true),
        ("permissions.beads_dir", "permissions", "warn", true),
        ("config.yaml", "configs", "warn", true),
        ("metadata.json", "configs", "warn", true),
        ("binary_version", "external_artifacts", "warn", true),
        ("write_lock", "concurrency_primitives", "warn", true),
        ("jsonl.parse", "state_files", "error", true),
        ("jsonl.merge_artifacts", "state_files", "warn", true),
        ("sync_jsonl_path", "state_files", "warn", true),
        ("sync_conflict_markers", "state_files", "error", true),
        ("db.exists", "state_files", "error", true),
        ("db.open", "state_files", "error", true),
        ("db.sidecars", "state_files", "warn", true),
        ("db.recovery_artifacts", "state_files", "info", true),
        ("schema.tables", "schemas", "error", true),
        ("schema.columns", "schemas", "error", true),
        ("schema.inspect", "schemas", "error", true),
        ("db.null_defaults", "schemas", "warn", true),
        ("sqlite.integrity_check", "state_files", "error", true),
        // --quick drops these:
        ("db.recoverable_anomalies", "caches_indexes", "warn", false),
        ("counts.db_vs_jsonl", "state_files", "warn", false),
        ("sync.metadata", "state_files", "warn", false),
        ("sqlite3.integrity_check", "state_files", "error", false),
        ("db.write_probe", "state_files", "warn", false),
        (
            "audit.suspect_close_reasons",
            "agent_coordination",
            "warn",
            true,
        ),
    ];
    rows.iter()
        .map(|(id, subsystem, sev, fast)| DetectorEntry {
            id: (*id).to_string(),
            subsystem: (*subsystem).to_string(),
            severity_default: (*sev).to_string(),
            fast_path: *fast,
        })
        .collect()
}

/// Fixer registry — one entry per `repair_*` path currently wired in
/// `src/cli/commands/doctor.rs`. Phase 10 cold-prober finding
/// (`beads_rust-3idn`): with this populated, an agent can list every
/// fixer the doctor can apply under `--repair` without reading source.
///
/// `auto_fixable: true` means `--repair` will attempt the fix without
/// further prompts; `false` reserved for advisory-only / refuse paths.
/// `mutates: true` means the fixer routes writes through the
/// `mutate()` chokepoint (per WP1+WP3 contract); `false` flags the
/// few legacy paths that still bypass the chokepoint (see
/// `beads_rust-8fud` for the migration plan).
fn build_fixer_registry() -> Vec<FixerEntry> {
    // (id, subsystem, auto_fixable, mutates_via_chokepoint, addressed_findings)
    let rows: &[(&str, &str, bool, bool, &[&str])] = &[
        (
            "doctor.gitignore_repair",
            "configs",
            true,
            true,
            &["gitignore.beads_inner", "gitignore.root"],
        ),
        (
            "doctor.repair_recoverable_db_state",
            "caches_indexes",
            true,
            false,
            &["db.recoverable_anomalies"],
        ),
        (
            "doctor.repair_partial_indexes",
            "caches_indexes",
            true,
            false,
            &["db.recoverable_anomalies"],
        ),
        (
            "doctor.repair_via_vacuum",
            "state_files",
            true,
            false,
            &["sqlite.integrity_check", "sqlite3.integrity_check"],
        ),
        (
            "doctor.repair_database_from_jsonl",
            "state_files",
            true,
            true,
            &[
                "db.open",
                "counts.db_vs_jsonl",
                "schema.tables",
                "schema.columns",
            ],
        ),
        (
            "doctor.repair_database_sidecars",
            "state_files",
            true,
            true,
            &["db.sidecars"],
        ),
        (
            "refuse_gates.schema_version_downgrade",
            "schemas",
            false,
            false,
            &["schema.columns"],
        ),
        (
            "refuse_gates.recovery_fingerprint_integrity",
            "state_files",
            false,
            false,
            &["db.recovery_artifacts"],
        ),
    ];
    rows.iter()
        .map(
            |(id, subsystem, auto_fixable, mutates, addressed)| FixerEntry {
                id: (*id).to_string(),
                subsystem: (*subsystem).to_string(),
                auto_fixable: *auto_fixable,
                mutates: *mutates,
                addressed_findings: addressed.iter().map(|s| (*s).to_string()).collect(),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_builds_with_expected_shape() {
        let caps = DoctorCapabilities::build();
        assert_eq!(caps.schema, "br.doctor.capabilities");
        assert_eq!(caps.contract_version, "1");
        assert_eq!(caps.doctor_version, env!("CARGO_PKG_VERSION"));
        assert!(!caps.exit_codes.is_empty());
        // Healthy / RefusedUnsafe / IoError must always be present.
        let mut codes: Vec<i32> = caps.exit_codes.iter().map(|e| e.code).collect();
        codes.sort_unstable();
        assert!(codes.contains(&0));
        assert!(codes.contains(&4));
        assert!(codes.contains(&74));
        // Scopes are non-empty.
        assert!(caps.write_scopes.contains(&".beads/".to_string()));
        assert!(caps.write_scopes.contains(&".doctor/".to_string()));
        // Env vars include the run-dir override.
        assert!(caps.env_vars.contains(&super::super::run_dir::ENV_RUNS_DIR));
    }

    #[test]
    fn capabilities_is_stable_json() {
        let caps = DoctorCapabilities::build();
        let json = caps.to_pretty_json().expect("json");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["schema"], "br.doctor.capabilities");
        assert_eq!(parsed["contract_version"], "1");
        assert!(parsed["exit_codes"].is_array());
        assert!(parsed["fixers"].is_array());
        assert!(parsed["detectors"].is_array());
        // WP4: ops_supported must enumerate the chokepoint contract.
        let ops = parsed["ops_supported"]
            .as_array()
            .expect("ops_supported array");
        assert!(
            ops.iter()
                .any(|o| o["name"] == "db_exec" && o["fully_routed"] == true),
            "db_exec must be advertised as fully routed"
        );
        assert!(
            ops.iter()
                .any(|o| o["name"] == "db_migrate" && o["fully_routed"] == true),
            "db_migrate must be advertised as fully routed (beads_rust-folg)"
        );
        for op in ops {
            assert!(op["params"].is_array(), "op.params must be an array: {op}");
        }
    }

    #[test]
    fn capabilities_detectors_and_fixers_are_populated() {
        // Phase 10 cold-prober finding (`beads_rust-3idn`): a cold
        // agent must see the actual detector + fixer registries, not
        // empty arrays. Lock the floor so future refactors can't
        // silently drop entries back to [].
        let caps = DoctorCapabilities::build();
        assert!(
            !caps.detectors.is_empty(),
            "detector registry must enumerate the check_* family"
        );
        assert!(
            !caps.fixers.is_empty(),
            "fixer registry must enumerate the repair_* family"
        );
        // Canonical entries must be present.
        let detector_ids: Vec<&str> = caps.detectors.iter().map(|d| d.id.as_str()).collect();
        let mut sorted_detector_ids = detector_ids.clone();
        sorted_detector_ids.sort_unstable();
        sorted_detector_ids.dedup();
        assert_eq!(
            sorted_detector_ids.len(),
            detector_ids.len(),
            "detector registry must not contain duplicate ids"
        );
        for required in &[
            "gitignore.beads_inner",
            "jsonl.parse",
            "db.open",
            "sqlite.integrity_check",
            "schema.tables",
        ] {
            assert!(
                detector_ids.contains(required),
                "detector registry missing {required}"
            );
        }
        for obsolete in &["merge_artifacts", "sqlite.cli_integrity"] {
            assert!(
                !detector_ids.contains(obsolete),
                "detector registry must not advertise obsolete check id {obsolete}"
            );
        }
        let detector = |id: &str| {
            caps.detectors
                .iter()
                .find(|d| d.id == id)
                .unwrap_or_else(|| panic!("missing detector {id}"))
        };
        assert_eq!(detector("jsonl.merge_artifacts").severity_default, "warn");
        assert_eq!(detector("schema.inspect").severity_default, "error");
        let fixer_ids: Vec<&str> = caps.fixers.iter().map(|f| f.id.as_str()).collect();
        for required in &[
            "doctor.gitignore_repair",
            "doctor.repair_via_vacuum",
            "doctor.repair_database_from_jsonl",
            "refuse_gates.schema_version_downgrade",
        ] {
            assert!(
                fixer_ids.contains(required),
                "fixer registry missing {required}"
            );
        }
        // Every detector must declare a subsystem the existing checks
        // recognize.
        for d in &caps.detectors {
            assert!(
                !d.subsystem.is_empty(),
                "detector {} missing subsystem",
                d.id
            );
        }
        // At least one fixer must route through the chokepoint (WP3+).
        assert!(
            caps.fixers.iter().any(|f| f.mutates),
            "at least one fixer must route through mutate()"
        );
    }
}
