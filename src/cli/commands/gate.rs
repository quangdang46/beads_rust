//! Workflow gate engine commands (`br gate`), issue #312 layer 2.
//!
//! `br gate report <id> --gate <name> --provider <name> --status pass|fail`
//! records a gate result; `br gate list <id>` shows recorded results and, when
//! the project configures `workflow.gates`, the computed required-gate status
//! for each guarded transition out of the issue's current status.
//!
//! Gate results are auxiliary, project-local metadata (like `close_metadata`):
//! they are not synced through JSONL. Enforcement at the close/transition
//! chokepoint lives in `close_policy::evaluate_gates`, hooked from
//! `commands::update` and `commands::close`.

use super::{
    acquire_routed_workspace_write_lock, auto_import_storage_ctx_if_stale, resolve_issue_id,
};
use crate::cli::{GateCommands, GateListArgs, GateReportArgs, GateStatus};
use crate::close_policy::{self, GateResult, Workflow};
use crate::config;
use crate::error::{BeadsError, Result};
use crate::format::sanitize_terminal_inline;
use crate::output::OutputContext;
use crate::util::id::{IdResolver, ResolverConfig};
use serde::Serialize;
use std::path::Path;

/// JSON payload for `br gate report`.
#[derive(Debug, Serialize)]
struct GateReportOutput {
    issue_id: String,
    gate: String,
    provider: String,
    passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

/// Computed status of one required gate for a guarded transition.
#[derive(Debug, Serialize)]
struct RequiredGateStatus {
    gate: String,
    satisfied: bool,
}

/// One guarded transition out of the issue's current status, with the
/// computed satisfaction of each gate it requires.
#[derive(Debug, Serialize)]
struct GatedTransitionStatus {
    from: String,
    to: String,
    gates: Vec<RequiredGateStatus>,
    /// True when every required gate for this transition is satisfied.
    satisfied: bool,
}

/// JSON payload for `br gate list`.
#[derive(Debug, Serialize)]
struct GateListOutput {
    issue_id: String,
    current_status: Option<String>,
    results: Vec<GateResult>,
    /// Gated transitions out of `current_status` and their satisfaction. Empty
    /// when the project has no `workflow.gates` config or the current status
    /// could not be resolved.
    gated_transitions: Vec<GatedTransitionStatus>,
}

/// Execute the gate command.
///
/// # Errors
///
/// Returns an error if database operations fail or the issue cannot be
/// resolved.
pub fn execute(
    command: &GateCommands,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    match command {
        GateCommands::Report(args) => execute_report(args, cli, ctx, &beads_dir),
        GateCommands::List(args) => execute_list(args, cli, ctx, &beads_dir),
    }
}

fn execute_report(
    args: &GateReportArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
    beads_dir: &Path,
) -> Result<()> {
    let gate = args.gate.trim();
    let provider = args.provider.trim();
    if gate.is_empty() {
        return Err(BeadsError::validation("gate", "--gate must not be empty"));
    }
    if provider.is_empty() {
        return Err(BeadsError::validation(
            "provider",
            "--provider must not be empty",
        ));
    }

    // Gate results are local auxiliary metadata; take the workspace write lock
    // so a concurrent flush/import doesn't race the write, mirroring the other
    // mutating commands.
    let _lock = acquire_routed_workspace_write_lock(beads_dir, false, cli.lock_timeout)?;
    let mut storage_ctx = config::open_storage_with_cli(beads_dir, cli)?;
    auto_import_storage_ctx_if_stale(&mut storage_ctx, cli)?;

    let config_layer = storage_ctx.load_config(cli)?;
    let actor = config::resolve_actor(&config_layer);
    let resolver = build_resolver(&config_layer);
    let issue_id = resolve_issue_id(&storage_ctx.storage, &resolver, &args.id)?;

    let passed = matches!(args.status, GateStatus::Pass);
    let note = args
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    storage_ctx
        .storage
        .record_gate_result(&issue_id, gate, provider, passed, note, &actor)?;

    crate::util::set_last_touched_id(beads_dir, &issue_id);

    let output = GateReportOutput {
        issue_id: issue_id.clone(),
        gate: gate.to_string(),
        provider: provider.to_string(),
        passed,
        note: note.map(str::to_string),
    };

    if ctx.is_toon() {
        ctx.toon(&output);
    } else if args.robot || ctx.is_json() {
        ctx.json_pretty(&output);
    } else {
        let verdict = if passed { "pass" } else { "fail" };
        ctx.success(&format!(
            "Recorded gate '{}' = {} (provider {}) for {}",
            sanitize_terminal_inline(gate),
            verdict,
            sanitize_terminal_inline(provider),
            sanitize_terminal_inline(&issue_id),
        ));
    }
    Ok(())
}

fn execute_list(
    args: &GateListArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
    beads_dir: &Path,
) -> Result<()> {
    let storage_ctx = config::open_storage_with_cli(beads_dir, cli)?;
    let config_layer = storage_ctx.load_config(cli)?;
    let resolver = build_resolver(&config_layer);
    let issue_id = resolve_issue_id(&storage_ctx.storage, &resolver, &args.id)?;

    let issue = storage_ctx.storage.get_issue(&issue_id)?;
    let current_status = issue.as_ref().map(|i| i.status.as_str().to_string());
    let results = storage_ctx.storage.get_gate_results(&issue_id)?;

    // Compute required-gate status for each guarded transition out of the
    // current status, using the project's workflow.gates config. Absent
    // config / status leaves this empty (backward compatible).
    let policy = close_policy::load_for_beads_dir(beads_dir)?;
    let labels = storage_ctx.storage.get_labels(&issue_id)?;
    let priority = issue.as_ref().map_or(0, |i| i.priority.0);
    let gated_transitions = compute_gated_transitions(
        &policy.workflow,
        &issue_id,
        current_status.as_deref(),
        &labels,
        priority,
        &results,
    );

    let output = GateListOutput {
        issue_id: issue_id.clone(),
        current_status,
        results,
        gated_transitions,
    };

    if ctx.is_toon() {
        ctx.toon(&output);
    } else if args.robot || ctx.is_json() {
        ctx.json_pretty(&output);
    } else {
        print_gate_list_human(ctx, &output);
    }
    Ok(())
}

/// Compute, for each `workflow.gates` rule whose `from` matches the issue's
/// current status, the satisfaction of every gate that rule requires for this
/// issue. Returns an empty vec when gates are not configured or the current
/// status is unknown.
fn compute_gated_transitions(
    workflow: &Workflow,
    issue_id: &str,
    current_status: Option<&str>,
    labels: &[String],
    priority: i32,
    results: &[GateResult],
) -> Vec<GatedTransitionStatus> {
    let Some(from) = current_status else {
        return Vec::new();
    };
    if workflow.gates.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    // Iterate the configured rules; only surface those whose `from` matches the
    // issue's current status (the transitions the issue could attempt next).
    for key in workflow.gates.keys() {
        let Some((rule_from, rule_to)) = key.split_once("->") else {
            continue;
        };
        let rule_from = rule_from.trim();
        let rule_to = rule_to.trim();
        if !rule_from.eq_ignore_ascii_case(from) {
            continue;
        }
        let violations = close_policy::evaluate_gates(
            workflow, issue_id, from, rule_to, labels, priority, results,
        );
        let required = workflow.required_gates_for(from, rule_to, labels, priority);
        let gates: Vec<RequiredGateStatus> = required
            .iter()
            .map(|spec| {
                let id = spec.id();
                let satisfied = !violations.iter().any(|v| v.gate == format!("gate_{id}"));
                RequiredGateStatus {
                    gate: id.to_string(),
                    satisfied,
                }
            })
            .collect();
        out.push(GatedTransitionStatus {
            from: from.to_string(),
            to: rule_to.to_string(),
            satisfied: violations.is_empty(),
            gates,
        });
    }
    out
}

fn print_gate_list_human(ctx: &OutputContext, output: &GateListOutput) {
    let id = sanitize_terminal_inline(&output.issue_id);
    if output.results.is_empty() {
        ctx.info(&format!("No gate results recorded for {id}."));
    } else {
        ctx.print_line(&format!("Gate results for {id}:"));
        for result in &output.results {
            let verdict = if result.passed { "pass" } else { "fail" };
            let gate = sanitize_terminal_inline(&result.gate);
            let provider = sanitize_terminal_inline(&result.provider);
            let mut line = format!("  {gate} [{provider}]: {verdict}");
            if let Some(note) = &result.note {
                line.push_str(&format!(" — {}", sanitize_terminal_inline(note)));
            }
            ctx.print_line(&line);
        }
    }

    if !output.gated_transitions.is_empty() {
        ctx.newline();
        ctx.print_line("Required gates for next transitions:");
        for transition in &output.gated_transitions {
            let marker = if transition.satisfied {
                "OK"
            } else {
                "BLOCKED"
            };
            ctx.print_line(&format!(
                "  {} -> {} [{}]",
                sanitize_terminal_inline(&transition.from),
                sanitize_terminal_inline(&transition.to),
                marker,
            ));
            for gate in &transition.gates {
                let status = if gate.satisfied {
                    "satisfied"
                } else {
                    "missing"
                };
                ctx.print_line(&format!(
                    "    {}: {}",
                    sanitize_terminal_inline(&gate.gate),
                    status
                ));
            }
        }
    }
}

fn build_resolver(config_layer: &config::ConfigLayer) -> IdResolver {
    let id_config = config::id_config_from_layer(config_layer);
    IdResolver::new(ResolverConfig::with_prefix(id_config.prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::close_policy::{ConditionalGate, GateRule, GateSpec};
    use crate::config::CliOverrides;
    use crate::model::{Issue, IssueType, Priority, Status};
    use std::fs;
    use tempfile::TempDir;

    fn open_storage(beads_dir: &Path) -> config::OpenStorageResult {
        config::open_storage_with_cli(beads_dir, &CliOverrides::default()).expect("storage")
    }

    fn make_issue(id: &str, status: Status) -> Issue {
        let now = chrono::Utc::now();
        Issue {
            id: id.to_string(),
            title: format!("issue {id}"),
            status,
            priority: Priority::MEDIUM,
            issue_type: IssueType::Task,
            created_at: now,
            updated_at: now,
            ..Issue::default()
        }
    }

    #[test]
    fn report_records_result_and_list_reads_it_back() {
        let temp = TempDir::new().unwrap();
        let beads_dir = temp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        {
            let mut ctx = open_storage(&beads_dir);
            ctx.storage
                .create_issue(&make_issue("bd-1", Status::Open), "tester")
                .unwrap();
        }

        let report = GateReportArgs {
            id: "bd-1".to_string(),
            gate: "ci_green".to_string(),
            provider: "ci".to_string(),
            status: GateStatus::Pass,
            note: Some("build #42".to_string()),
            robot: true,
        };
        let ctx = OutputContext::from_flags(true, false, true);
        execute_report(&report, &CliOverrides::default(), &ctx, &beads_dir).unwrap();

        let storage_ctx = open_storage(&beads_dir);
        let results = storage_ctx.storage.get_gate_results("bd-1").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].gate, "ci_green");
        assert_eq!(results[0].provider, "ci");
        assert!(results[0].passed);
        assert_eq!(results[0].note.as_deref(), Some("build #42"));
    }

    #[test]
    fn report_overwrites_same_provider_and_gate() {
        let temp = TempDir::new().unwrap();
        let beads_dir = temp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        {
            let mut ctx = open_storage(&beads_dir);
            ctx.storage
                .create_issue(&make_issue("bd-1", Status::Open), "tester")
                .unwrap();
        }
        let ctx = OutputContext::from_flags(true, false, true);
        let base = GateReportArgs {
            id: "bd-1".to_string(),
            gate: "ci_green".to_string(),
            provider: "ci".to_string(),
            status: GateStatus::Fail,
            note: None,
            robot: true,
        };
        execute_report(&base, &CliOverrides::default(), &ctx, &beads_dir).unwrap();
        let pass = GateReportArgs {
            status: GateStatus::Pass,
            ..base.clone()
        };
        execute_report(&pass, &CliOverrides::default(), &ctx, &beads_dir).unwrap();

        let storage_ctx = open_storage(&beads_dir);
        let results = storage_ctx.storage.get_gate_results("bd-1").unwrap();
        assert_eq!(results.len(), 1, "same (gate, provider) must overwrite");
        assert!(results[0].passed);
    }

    #[test]
    fn report_rejects_empty_gate_and_provider() {
        let temp = TempDir::new().unwrap();
        let beads_dir = temp.path().join(".beads");
        fs::create_dir_all(&beads_dir).unwrap();
        let ctx = OutputContext::from_flags(true, false, true);
        let empty_gate = GateReportArgs {
            id: "bd-1".to_string(),
            gate: "   ".to_string(),
            provider: "ci".to_string(),
            status: GateStatus::Pass,
            note: None,
            robot: true,
        };
        assert!(execute_report(&empty_gate, &CliOverrides::default(), &ctx, &beads_dir).is_err());
    }

    #[test]
    fn list_computes_required_gate_status() {
        let mut gates = std::collections::BTreeMap::new();
        gates.insert(
            "in_review -> closed".to_string(),
            GateRule {
                require_all: vec![
                    GateSpec::Named("ci_green".to_string()),
                    GateSpec::MinReviewers(1),
                ],
                require_if: vec![ConditionalGate {
                    label: Some("security-sensitive".to_string()),
                    gate: GateSpec::Named("security_sign_off".to_string()),
                    ..Default::default()
                }],
            },
        );
        let workflow = Workflow {
            strict: true,
            gates,
            ..Default::default()
        };

        // No results yet: ci_green + min_reviewers required, security not (no label).
        let transitions =
            compute_gated_transitions(&workflow, "bd-1", Some("in_review"), &[], 2, &[]);
        assert_eq!(transitions.len(), 1);
        let t = &transitions[0];
        assert_eq!(t.to, "closed");
        assert!(!t.satisfied);
        let gate_ids: Vec<&str> = t.gates.iter().map(|g| g.gate.as_str()).collect();
        assert!(gate_ids.contains(&"ci_green"));
        assert!(gate_ids.contains(&"min_reviewers"));
        assert!(!gate_ids.contains(&"security_sign_off"));

        // With ci pass + one reviewer pass, both satisfied.
        let results = vec![
            GateResult {
                gate: "ci_green".to_string(),
                provider: "ci".to_string(),
                passed: true,
                note: None,
            },
            GateResult {
                gate: "min_reviewers".to_string(),
                provider: "reviewer:alice".to_string(),
                passed: true,
                note: None,
            },
        ];
        let transitions =
            compute_gated_transitions(&workflow, "bd-1", Some("in_review"), &[], 2, &results);
        assert!(transitions[0].satisfied);
    }

    #[test]
    fn list_is_empty_without_gate_config() {
        let workflow = Workflow::default();
        let transitions =
            compute_gated_transitions(&workflow, "bd-1", Some("in_review"), &[], 2, &[]);
        assert!(transitions.is_empty());
    }
}
