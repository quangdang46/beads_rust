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
//! - `fixers` — empty for now; WP3-WP12 fills it in
//! - `detectors` — empty for now; the legacy `check_*` family is not
//!   yet enumerated in a single registry
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
    /// Fixer registry. Empty in WP1; populated by WP3-WP12.
    pub fixers: Vec<FixerEntry>,
    /// Detector registry. Empty in WP1.
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
            fixers: Vec::new(),
            detectors: Vec::new(),
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
                              matches `from`, snapshots the DB file verbatim, then defers \
                              the actual DDL to schema.rs (warning: \
                              migration_logic_not_yet_routed).",
                    params: vec!["from", "to"],
                    fully_routed: false,
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
                .any(|o| o["name"] == "db_migrate" && o["fully_routed"] == false),
            "db_migrate must be advertised as scaffold-only"
        );
        for op in ops {
            assert!(op["params"].is_array(), "op.params must be an array: {op}");
        }
    }
}
