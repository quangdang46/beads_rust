//! Close command implementation.

use crate::cli::CloseArgs as CliCloseArgs;
use crate::cli::commands::{
    acquire_routed_workspace_write_lock, auto_import_storage_ctx_if_stale,
    finalize_batched_blocked_cache_refresh, preserve_blocked_cache_on_error,
    report_auto_flush_failure, resolve_issue_ids, update_issue_with_recovery,
};
use crate::close_policy::{
    self, AttributionTier, AttributionValues, CloseEvidence, ClosePolicy, PolicyDocument,
    PolicyViolation,
};
use crate::config;
use crate::error::{BeadsError, Result};
use crate::format::sanitize_terminal_inline;
use crate::model::{Issue, IssueType, Status};
use crate::output::OutputContext;
use crate::storage::{IssueUpdate, SqliteStorage};
use crate::util::id::{IdResolver, ResolverConfig};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

/// Internal arguments for the close command.
#[derive(Debug, Clone, Default)]
pub struct CloseArgs {
    /// Issue IDs to close
    pub ids: Vec<String>,
    /// Close reason
    pub reason: Option<String>,
    /// Force close even if blocked
    pub force: bool,
    /// Session ID for `closed_by_session` field
    pub session: Option<String>,
    /// Return newly unblocked issues (single ID only)
    pub suggest_next: bool,
    /// Tier 1 attribution: agent name (issue #274 Phase 1).
    pub agent_name: Option<String>,
    /// Tier 1 attribution: harness identifier.
    pub harness: Option<String>,
    /// Tier 1 attribution: model identifier.
    pub model: Option<String>,
    /// Bypass closure-time policy gates.
    pub bypass_policy: bool,
    /// Reason for bypass. Required when `bypass_policy = true`.
    pub bypass_reason: Option<String>,
}

impl From<&CliCloseArgs> for CloseArgs {
    fn from(cli: &CliCloseArgs) -> Self {
        Self {
            ids: cli.ids.clone(),
            reason: cli.reason.clone(),
            force: cli.force,
            session: cli.session.clone(),
            suggest_next: cli.suggest_next,
            agent_name: cli.agent_name.clone(),
            harness: cli.harness.clone(),
            model: cli.model.clone(),
            bypass_policy: cli.bypass_policy,
            bypass_reason: cli.bypass_reason.clone(),
        }
    }
}

/// Aggregate of policy gates that fired for a single candidate close.
struct EvaluatedGates {
    violations: Vec<PolicyViolation>,
}

/// Validate the `--bypass-policy` / `--bypass-reason` flag pair before
/// touching storage. Mirrors the documented contract: bypass requires a
/// non-empty reason and is meaningless without `--bypass-policy`.
fn validate_bypass_args(args: &CloseArgs) -> Result<()> {
    if args.bypass_policy {
        let reason_present = args
            .bypass_reason
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        if !reason_present {
            return Err(BeadsError::validation(
                "bypass-reason",
                "--bypass-policy requires --bypass-reason \"<text>\"",
            ));
        }
    } else if args
        .bypass_reason
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        return Err(BeadsError::validation(
            "bypass-policy",
            "--bypass-reason was set without --bypass-policy",
        ));
    }
    Ok(())
}

/// Resolve attribution values for the close. CLI flags take precedence over
/// env vars; both are ignored when the policy.yaml `attribution.tier` is
/// `off`. Tier 2/3 ("require"/"allowlist") are out of scope for Phase 1.
fn resolve_attribution_for_close(
    args: &CloseArgs,
    policy_doc: &PolicyDocument,
) -> AttributionValues {
    if policy_doc.close_policy.attribution.tier == AttributionTier::Off {
        return AttributionValues::default();
    }
    AttributionValues::resolve_from_env(
        args.agent_name.as_deref(),
        args.harness.as_deref(),
        args.model.as_deref(),
    )
}

/// Run every enabled gate against `issue` and produce the (possibly empty)
/// violation list.
fn evaluate_close_policy(
    policy: &ClosePolicy,
    storage: &SqliteStorage,
    issue_id: &str,
    issue: &Issue,
    args: &CloseArgs,
    close_actor: &str,
) -> Result<EvaluatedGates> {
    // Look up the in_progress actor only when the gate is enabled — this
    // saves a query per close for repos that don't enable that specific
    // gate.
    let in_progress_actor = if policy.forbid_self_close_after_in_progress.enabled {
        storage.find_last_in_progress_actor(issue_id)?
    } else {
        None
    };

    let evidence = CloseEvidence {
        issue_id,
        close_reason: args.reason.as_deref(),
        description: issue.description.as_deref(),
        design: issue.design.as_deref(),
        acceptance_criteria: issue.acceptance_criteria.as_deref(),
        notes: issue.notes.as_deref(),
        close_actor,
        in_progress_actor: in_progress_actor.as_deref(),
    };

    let violations = close_policy::evaluate(policy, &evidence);
    Ok(EvaluatedGates { violations })
}

fn summarize_violations(violations: &[PolicyViolation]) -> String {
    if let [single] = violations {
        return single.message.clone();
    }
    let lines: Vec<String> = violations
        .iter()
        .map(|v| format!("- {}", v.message))
        .collect();
    format!("{} gates fired:\n{}", violations.len(), lines.join("\n"))
}

fn emit_bypass_warning(ctx: &OutputContext, issue_id: &str, violations: &[PolicyViolation]) {
    let summary = summarize_violations(violations);
    let id = sanitize_terminal_inline(issue_id);
    let summary = sanitize_terminal_inline(&summary);
    ctx.warning(&format!(
        "Closing {} despite policy violation(s) (--bypass-policy): {}",
        id.as_ref(),
        summary.as_ref()
    ));
}

/// Execute the close command from CLI args.
///
/// # Errors
///
/// Returns an error if database operations fail or IDs cannot be resolved.
pub fn execute_cli(
    cli_args: &CliCloseArgs,
    json: bool,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let args = CloseArgs::from(cli_args);
    execute_with_args(&args, json, cli, ctx)
}

/// Result of a close operation for JSON output.
#[derive(Debug, Serialize, Deserialize)]
pub struct CloseResult {
    pub closed: Vec<ClosedIssue>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub skipped: Vec<SkippedIssue>,
}

/// Result of closing with suggest-next.
#[derive(Debug, Serialize, Deserialize)]
pub struct CloseWithSuggestResult {
    pub closed: Vec<ClosedIssue>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub skipped: Vec<SkippedIssue>,
    pub unblocked: Vec<UnblockedIssue>,
}

/// An issue that became unblocked after closing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnblockedIssue {
    pub id: String,
    pub title: String,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedIssue {
    pub id: String,
    pub title: String,
    pub status: String,
    pub closed_at: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub close_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedIssue {
    pub id: String,
    pub reason: String,
}

#[allow(dead_code)]
#[derive(Debug, Default)]
struct CloseExecution {
    closed: Vec<ClosedIssue>,
    skipped: Vec<SkippedIssue>,
    unblocked: Vec<UnblockedIssue>,
    ordered_outcomes: Vec<CloseOutcome>,
}

#[derive(Debug, Clone)]
enum CloseOutcome {
    Closed(ClosedIssue),
    Skipped(SkippedIssue),
}

fn build_close_json_payload(
    args: &CloseArgs,
    closed_issues: Vec<ClosedIssue>,
    skipped_issues: Vec<SkippedIssue>,
    unblocked_issues: Vec<UnblockedIssue>,
) -> Result<String> {
    let json = if args.suggest_next {
        // suggest_next is br-only, so always use the wrapped machine format.
        let result = CloseWithSuggestResult {
            closed: closed_issues,
            skipped: skipped_issues,
            unblocked: unblocked_issues,
        };
        serde_json::to_string_pretty(&result)?
    } else if skipped_issues.is_empty() {
        // Preserve bd-compatible array output for pure-success closes.
        serde_json::to_string_pretty(&closed_issues)?
    } else {
        // Once skips are present, a bare array loses machine-readable reasons.
        let result = CloseResult {
            closed: closed_issues,
            skipped: skipped_issues,
        };
        serde_json::to_string_pretty(&result)?
    };

    Ok(json)
}

fn render_close_json(
    args: &CloseArgs,
    closed_issues: Vec<ClosedIssue>,
    skipped_issues: Vec<SkippedIssue>,
    unblocked_issues: Vec<UnblockedIssue>,
) -> Result<()> {
    let json = build_close_json_payload(args, closed_issues, skipped_issues, unblocked_issues)?;
    println!("{json}");
    Ok(())
}

fn emit_close_structured_output(
    args: &CloseArgs,
    closed_issues: Vec<ClosedIssue>,
    skipped_issues: Vec<SkippedIssue>,
    unblocked_issues: Vec<UnblockedIssue>,
    ctx: &OutputContext,
) -> Result<()> {
    if args.suggest_next {
        let result = CloseWithSuggestResult {
            closed: closed_issues,
            skipped: skipped_issues,
            unblocked: unblocked_issues,
        };
        if ctx.is_toon() {
            ctx.toon(&result);
        } else if ctx.is_json() {
            ctx.json_pretty(&result);
        } else {
            let json_ctx = OutputContext::from_flags(true, false, true);
            json_ctx.json_pretty(&result);
        }
        return Ok(());
    }

    if skipped_issues.is_empty() {
        if ctx.is_toon() {
            ctx.toon(&closed_issues);
        } else if ctx.is_json() {
            ctx.json_pretty(&closed_issues);
        } else {
            render_close_json(args, closed_issues, skipped_issues, unblocked_issues)?;
        }
        return Ok(());
    }

    let result = CloseResult {
        closed: closed_issues,
        skipped: skipped_issues,
    };
    if ctx.is_toon() {
        ctx.toon(&result);
    } else if ctx.is_json() {
        ctx.json_pretty(&result);
    } else {
        let json_ctx = OutputContext::from_flags(true, false, true);
        json_ctx.json_pretty(&result);
    }
    Ok(())
}

fn close_human_message(closed: &ClosedIssue) -> String {
    let id = sanitize_terminal_inline(&closed.id);
    let title = sanitize_terminal_inline(&closed.title);
    let mut message = format!("Closed {}: {}", id.as_ref(), title.as_ref());
    if let Some(reason) = &closed.close_reason {
        let reason = sanitize_terminal_inline(reason);
        message.push_str(" (");
        message.push_str(reason.as_ref());
        message.push(')');
    }
    message
}

fn skipped_human_message(skipped: &SkippedIssue) -> String {
    let id = sanitize_terminal_inline(&skipped.id);
    let reason = sanitize_terminal_inline(&skipped.reason);
    format!("Skipped {}: {}", id.as_ref(), reason.as_ref())
}

fn unblocked_human_line(issue: &UnblockedIssue) -> String {
    let id = sanitize_terminal_inline(&issue.id);
    let title = sanitize_terminal_inline(&issue.title);
    format!("  {}: {}", id.as_ref(), title.as_ref())
}

fn issue_input_text(input: &str) -> String {
    sanitize_terminal_inline(input).into_owned()
}

fn reorder_routed_items_by_requested_inputs<T>(
    requested_inputs: &[String],
    routed_items: Vec<(Vec<String>, Vec<T>)>,
    context: &str,
) -> Result<Vec<T>> {
    let mut positions_by_input: HashMap<&str, VecDeque<usize>> = HashMap::new();
    for (index, input) in requested_inputs.iter().enumerate() {
        positions_by_input
            .entry(input.as_str())
            .or_default()
            .push_back(index);
    }

    let mut ordered_items: Vec<Option<T>> = (0..requested_inputs.len()).map(|_| None).collect();
    for (batch_inputs, batch_items) in routed_items {
        if batch_inputs.len() != batch_items.len() {
            return Err(BeadsError::internal(format!(
                "{context} produced mismatched issue/result counts"
            )));
        }

        for (input, item) in batch_inputs.into_iter().zip(batch_items) {
            let Some(index) = positions_by_input
                .get_mut(input.as_str())
                .and_then(VecDeque::pop_front)
            else {
                let input = issue_input_text(&input);
                return Err(BeadsError::internal(format!(
                    "{context} returned unexpected issue input {input}"
                )));
            };
            let Some(slot) = ordered_items.get_mut(index) else {
                let input = issue_input_text(&input);
                return Err(BeadsError::internal(format!(
                    "{context} returned out-of-range issue input {input}"
                )));
            };
            *slot = Some(item);
        }
    }

    ordered_items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            item.ok_or_else(|| {
                let input = requested_inputs
                    .get(index)
                    .map(|input| issue_input_text(input))
                    .unwrap_or_else(|| "<unknown>".to_string());
                BeadsError::internal(format!("{context} did not produce a result for {input}"))
            })
        })
        .collect()
}

fn compute_batch_closable_ids(
    active_issue_ids: &HashSet<String>,
    internal_blockers_by_id: &HashMap<String, Vec<String>>,
    external_blockers_by_id: &HashMap<String, Vec<String>>,
) -> HashSet<String> {
    let mut closable: HashSet<String> = active_issue_ids
        .iter()
        .filter(|id| {
            external_blockers_by_id
                .get(*id)
                .is_none_or(std::vec::Vec::is_empty)
        })
        .cloned()
        .collect();

    loop {
        let to_remove: Vec<String> = closable
            .iter()
            .filter(|id| {
                internal_blockers_by_id
                    .get(*id)
                    .into_iter()
                    .flatten()
                    .any(|blocker_id| !closable.contains(blocker_id))
            })
            .cloned()
            .collect();

        if to_remove.is_empty() {
            break;
        }

        for id in to_remove {
            closable.remove(&id);
        }
    }

    closable
}

/// Execute the close command.
///
/// # Errors
///
/// Returns an error if database operations fail or IDs cannot be resolved.
pub fn execute(
    ids: Vec<String>,
    json: bool,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let args = CloseArgs {
        ids,
        ..CloseArgs::default()
    };

    execute_with_args(&args, json, cli, ctx)
}

/// Execute the close command with full arguments.
///
/// # Errors
///
/// Returns an error if database operations fail or IDs cannot be resolved.
#[allow(clippy::too_many_lines)]
pub fn execute_with_args(
    args: &CloseArgs,
    use_json: bool,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    tracing::info!("Executing close command");
    let use_structured_output = use_json || ctx.is_json() || ctx.is_toon();

    // Up-front bypass argument-pair validation. Done before any storage IO so
    // a misuse of the bypass flag never silently slips past policy gates.
    validate_bypass_args(args)?;

    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let mut target_inputs = args.ids.clone();
    if target_inputs.is_empty() {
        let last_touched = crate::util::get_last_touched_id(&beads_dir);
        if last_touched.is_empty() {
            return Err(BeadsError::validation(
                "ids",
                "no issue IDs provided and no last-touched issue",
            ));
        }
        target_inputs.push(last_touched);
    }

    if args.suggest_next && target_inputs.len() > 1 {
        return Err(BeadsError::validation(
            "suggest-next",
            "--suggest-next only works with a single issue ID",
        ));
    }
    let routed_batches = config::routing::group_issue_inputs_by_route(&target_inputs, &beads_dir)?;

    let mut closed_issues = Vec::new();
    let mut skipped_issues = Vec::new();
    let mut unblocked_issues = Vec::new();

    if routed_batches.iter().any(|batch| batch.is_external) {
        let normalized_local_beads_dir =
            dunce::canonicalize(&beads_dir).unwrap_or_else(|_| beads_dir.clone());
        let mut routed_outcomes = Vec::new();

        for batch in routed_batches {
            let mut batch_args = args.clone();
            batch_args.ids.clone_from(&batch.issue_inputs);

            let normalized_batch_beads_dir =
                dunce::canonicalize(&batch.beads_dir).unwrap_or_else(|_| batch.beads_dir.clone());
            let mut batch_cli = cli.clone();
            batch_cli.db = if normalized_batch_beads_dir == normalized_local_beads_dir {
                cli.db.clone()
            } else {
                None
            };

            let execution = execute_route(
                &batch_args,
                &batch_cli,
                ctx,
                &batch.beads_dir,
                batch.is_external,
            )?;
            let CloseExecution {
                unblocked,
                ordered_outcomes,
                ..
            } = execution;
            routed_outcomes.push((batch.issue_inputs, ordered_outcomes));
            unblocked_issues.extend(unblocked);
        }

        let ordered_outcomes = reorder_routed_items_by_requested_inputs(
            &target_inputs,
            routed_outcomes,
            "close routing",
        )?;
        for outcome in ordered_outcomes {
            match outcome {
                CloseOutcome::Closed(issue) => closed_issues.push(issue),
                CloseOutcome::Skipped(issue) => skipped_issues.push(issue),
            }
        }
    } else {
        let mut local_args = args.clone();
        local_args.ids = target_inputs;
        let execution = execute_route(&local_args, cli, ctx, &beads_dir, false)?;
        closed_issues = execution.closed;
        skipped_issues = execution.skipped;
        unblocked_issues = execution.unblocked;
    }

    let closed_count = closed_issues.len();
    let skipped_count = skipped_issues.len();

    if let Some(last_closed) = closed_issues.last() {
        crate::util::set_last_touched_id(&beads_dir, &last_closed.id);
    }

    if use_structured_output {
        emit_close_structured_output(args, closed_issues, skipped_issues, unblocked_issues, ctx)?;
    } else if closed_issues.is_empty() && skipped_issues.is_empty() {
        ctx.info("No issues to close.");
    } else {
        for closed in &closed_issues {
            ctx.success(&close_human_message(closed));
        }
        for skipped in &skipped_issues {
            ctx.warning(&skipped_human_message(skipped));
        }
        if !unblocked_issues.is_empty() {
            ctx.newline();
            ctx.info(&format!("Unblocked {} issue(s):", unblocked_issues.len()));
            for issue in &unblocked_issues {
                ctx.print_line(&unblocked_human_line(issue));
            }
        }
    }

    if closed_count == 0 && skipped_count > 0 {
        return Err(BeadsError::NothingToDo {
            reason: format!("all {skipped_count} issue(s) skipped"),
        });
    }

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn execute_route(
    args: &CloseArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
    beads_dir: &Path,
    auto_flush_external: bool,
) -> Result<CloseExecution> {
    let _routed_write_lock =
        acquire_routed_workspace_write_lock(beads_dir, auto_flush_external, cli.lock_timeout)?;
    let mut storage_ctx = config::open_storage_with_cli(beads_dir, cli)?;
    auto_import_storage_ctx_if_stale(&mut storage_ctx, cli)?;

    let config_layer = storage_ctx.load_config(cli)?;
    let actor = config::resolve_actor(&config_layer);
    let id_config = config::id_config_from_layer(&config_layer);
    let resolver = IdResolver::new(ResolverConfig::with_prefix(id_config.prefix));
    let resolved_ids = resolve_issue_ids(&storage_ctx.storage, &resolver, &args.ids)?;

    // Closure-time policy gates (issue #274 Phase 1). Loading happens once per
    // route; if the file is absent the doc is the all-off default.
    let policy_doc = close_policy::load_for_beads_dir(beads_dir)?;
    let policy_active = policy_doc.close_policy.is_active();
    let attribution = resolve_attribution_for_close(args, &policy_doc);
    if args.bypass_policy && !policy_doc.allow_bypass {
        return Err(BeadsError::validation(
            "bypass-policy",
            ".beads/policy.yaml has allow_bypass: false; --bypass-policy is disabled",
        ));
    }

    let epic_counts = storage_ctx.storage.get_epic_counts()?;
    let blocked_before: Vec<String> = if args.suggest_next {
        storage_ctx
            .storage
            .get_blocked_issues()?
            .into_iter()
            .map(|(i, _)| i.id)
            .collect()
    } else {
        Vec::new()
    };

    let requested_ids: HashSet<String> = resolved_ids.iter().cloned().collect();
    let mut open_issues: HashMap<String, crate::model::Issue> = HashMap::new();
    let mut internal_blockers_by_id: HashMap<String, Vec<String>> = HashMap::new();
    let mut external_blockers_by_id: HashMap<String, Vec<String>> = HashMap::new();
    let mut closed_issues: Vec<ClosedIssue> = Vec::new();
    let mut skipped_issues: Vec<SkippedIssue> = Vec::new();
    let mut ordered_outcomes = Vec::with_capacity(resolved_ids.len());
    let mut cache_dirty = false;

    for id in &resolved_ids {
        tracing::info!(id = %id, "Closing issue");

        let issue_result = storage_ctx.storage.get_issue(id);
        let Some(issue) = preserve_blocked_cache_on_error(
            &mut storage_ctx.storage,
            cache_dirty,
            "close",
            issue_result,
        )?
        else {
            let skipped = SkippedIssue {
                id: id.clone(),
                reason: "issue not found".to_string(),
            };
            ordered_outcomes.push(CloseOutcome::Skipped(skipped.clone()));
            skipped_issues.push(skipped);
            continue;
        };

        if issue.status.is_terminal() {
            let skipped = SkippedIssue {
                id: id.clone(),
                reason: format!("already {}", issue.status.as_str()),
            };
            ordered_outcomes.push(CloseOutcome::Skipped(skipped.clone()));
            skipped_issues.push(skipped);
            continue;
        }

        if !args.force
            && let Some(&(total, closed)) = epic_counts.get(id)
            && closed < total
        {
            let label = if issue.issue_type == IssueType::Epic {
                "epic"
            } else {
                "parent issue"
            };
            let skipped = SkippedIssue {
                id: id.clone(),
                reason: format!(
                    "{label} has {}/{} open children (use --force to close anyway)",
                    total - closed,
                    total
                ),
            };
            ordered_outcomes.push(CloseOutcome::Skipped(skipped.clone()));
            skipped_issues.push(skipped);
            continue;
        }

        // Supplementary guard: catch dot-notation children (e.g. `epic.1`,
        // `epic.2`) that exist in the issues table without a formal
        // parent-child dep row. These slip past `epic_counts` because
        // get_epic_counts only scans the dependencies table. Missing-dep
        // children occur with legacy-bd migrations, bulk JSONL imports,
        // and hand-edited JSONL. Without this check, closing the parent
        // silently orphans the open children.
        let requested_dot_children = if args.force {
            Vec::new()
        } else {
            let open_dot_children = storage_ctx.storage.get_open_dot_notation_children(id)?;
            let (requested_children, unrequested_children): (Vec<String>, Vec<String>) =
                open_dot_children
                    .into_iter()
                    .partition(|child_id| requested_ids.contains(child_id));
            if !unrequested_children.is_empty() {
                let label = if issue.issue_type == IssueType::Epic {
                    "epic"
                } else {
                    "parent issue"
                };
                let preview: Vec<String> = unrequested_children.iter().take(5).cloned().collect();
                let suffix = if unrequested_children.len() > preview.len() {
                    format!(", +{} more", unrequested_children.len() - preview.len())
                } else {
                    String::new()
                };
                let skipped = SkippedIssue {
                    id: id.clone(),
                    reason: format!(
                        "{label} has {} open dot-notation child issue(s): {}{} (use --force to close anyway)",
                        unrequested_children.len(),
                        preview.join(", "),
                        suffix
                    ),
                };
                ordered_outcomes.push(CloseOutcome::Skipped(skipped.clone()));
                skipped_issues.push(skipped);
                continue;
            }
            requested_children
        };

        if args.force {
            open_issues.insert(id.clone(), issue);
            continue;
        }

        let is_blocked_result = storage_ctx.storage.is_blocked(id);
        let mut blocker_ids = if preserve_blocked_cache_on_error(
            &mut storage_ctx.storage,
            cache_dirty,
            "close",
            is_blocked_result,
        )? {
            let blockers_result = storage_ctx.storage.get_blockers(id);
            preserve_blocked_cache_on_error(
                &mut storage_ctx.storage,
                cache_dirty,
                "close",
                blockers_result,
            )?
        } else {
            Vec::new()
        };
        blocker_ids.extend(requested_dot_children);
        blocker_ids.sort();
        blocker_ids.dedup();
        let (internal_blockers, external_blockers): (Vec<String>, Vec<String>) = blocker_ids
            .into_iter()
            .partition(|blocker_id| requested_ids.contains(blocker_id));
        internal_blockers_by_id.insert(id.clone(), internal_blockers);
        external_blockers_by_id.insert(id.clone(), external_blockers);
        open_issues.insert(id.clone(), issue);
    }

    let active_issue_ids: HashSet<String> = open_issues.keys().cloned().collect();
    let batch_closable_ids = if args.force {
        active_issue_ids
    } else {
        compute_batch_closable_ids(
            &active_issue_ids,
            &internal_blockers_by_id,
            &external_blockers_by_id,
        )
    };

    for id in &resolved_ids {
        let Some(issue) = open_issues.get(id) else {
            continue;
        };

        if !args.force && !batch_closable_ids.contains(id) {
            let mut blocker_ids = external_blockers_by_id.get(id).cloned().unwrap_or_default();
            if let Some(internal_blockers) = internal_blockers_by_id.get(id) {
                blocker_ids.extend(
                    internal_blockers
                        .iter()
                        .filter(|blocker_id| !batch_closable_ids.contains(*blocker_id))
                        .cloned(),
                );
            }
            blocker_ids.sort();
            blocker_ids.dedup();
            tracing::debug!(blocked_by = ?blocker_ids, "Issue remains blocked in batch close");
            let reason = if blocker_ids.is_empty() {
                "blocked by dependencies".to_string()
            } else {
                format!("blocked by: {}", blocker_ids.join(", "))
            };
            let skipped = SkippedIssue {
                id: id.clone(),
                reason,
            };
            ordered_outcomes.push(CloseOutcome::Skipped(skipped.clone()));
            skipped_issues.push(skipped);
            continue;
        }

        // Closure-time policy gate evaluation (issue #274 Phase 1).
        //
        // Skipped entirely when the project has no policy.yaml or every gate
        // is disabled, preserving the current behavior for solo-dev workflows.
        // When `--bypass-policy --bypass-reason "<text>"` is supplied AND the
        // project's policy.yaml allows it, gate violations are recorded in
        // `close_metadata` (with `bypassed_policy = 1`) but do not block the
        // close.
        let gates_fired = if policy_active {
            let evaluated_gates = evaluate_close_policy(
                &policy_doc.close_policy,
                &storage_ctx.storage,
                id,
                issue,
                args,
                &actor,
            )?;
            if !evaluated_gates.violations.is_empty() && !args.bypass_policy {
                let summary = summarize_violations(&evaluated_gates.violations);
                return Err(BeadsError::PolicyViolation {
                    issue_id: id.clone(),
                    summary,
                    violations: evaluated_gates.violations,
                });
            }
            if !evaluated_gates.violations.is_empty() && args.bypass_policy {
                emit_bypass_warning(ctx, id, &evaluated_gates.violations);
            }
            evaluated_gates
                .violations
                .into_iter()
                .map(|v| v.gate)
                .collect::<Vec<String>>()
        } else {
            Vec::new()
        };

        let now = Utc::now();
        let close_reason = args.reason.clone().unwrap_or_else(|| "done".to_string());
        let update = IssueUpdate {
            status: Some(Status::Closed),
            closed_at: Some(Some(now)),
            close_reason: Some(Some(close_reason.clone())),
            closed_by_session: args.session.clone().map(Some),
            skip_cache_rebuild: true,
            ..Default::default()
        };

        let update_result = update_issue_with_recovery(
            &mut storage_ctx,
            !cache_dirty,
            "close",
            id,
            &update,
            &actor,
        );
        preserve_blocked_cache_on_error(
            &mut storage_ctx.storage,
            cache_dirty,
            "close",
            update_result,
        )?;
        cache_dirty = true;
        tracing::info!(id = %id, reason = ?args.reason, "Issue closed");

        // Best-effort persistence of attribution + bypass auditing. Failure to
        // record metadata never undoes a successful close: the close already
        // committed, and burning down a successful close because of an
        // optional auxiliary table would be a regression for solo-dev users
        // whose schema couldn't migrate. We log and move on.
        let bypass_reason = if args.bypass_policy {
            args.bypass_reason.as_deref()
        } else {
            None
        };
        let metadata_result = storage_ctx.storage.record_close_metadata(
            id,
            &attribution,
            args.bypass_policy && !gates_fired.is_empty(),
            bypass_reason,
            &gates_fired,
        );
        if let Err(error) = metadata_result {
            tracing::warn!(
                issue_id = %id,
                error = %error,
                "failed to record closure-time policy metadata; close already committed"
            );
        }

        let closed = ClosedIssue {
            id: id.clone(),
            title: issue.title.clone(),
            status: "closed".to_string(),
            closed_at: now.to_rfc3339(),
            close_reason: Some(close_reason),
        };
        ordered_outcomes.push(CloseOutcome::Closed(closed.clone()));
        closed_issues.push(closed);
    }

    if cache_dirty {
        tracing::info!(
            "Rebuilding blocked cache after closing {} issues",
            closed_issues.len()
        );
        finalize_batched_blocked_cache_refresh(&mut storage_ctx.storage, cache_dirty, "close")?;
    }

    let unblocked_issues: Vec<UnblockedIssue> = if args.suggest_next && !closed_issues.is_empty() {
        let blocked_after_result = storage_ctx.storage.get_blocked_issues();
        let blocked_after = match preserve_blocked_cache_on_error(
            &mut storage_ctx.storage,
            cache_dirty,
            "close",
            blocked_after_result,
        ) {
            Ok(blocked_after) => Some(
                blocked_after
                    .into_iter()
                    .map(|(issue, _)| issue.id)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Skipping suggest-next calculation after committed close because blocked-cache lookup failed"
                );
                None
            }
        };

        let Some(blocked_after) = blocked_after else {
            storage_ctx.flush_no_db_if_dirty()?;
            return Ok(CloseExecution {
                closed: closed_issues,
                skipped: skipped_issues,
                unblocked: Vec::new(),
                ordered_outcomes,
            });
        };

        let newly_unblocked: Vec<String> = blocked_before
            .into_iter()
            .filter(|id| !blocked_after.contains(id))
            .collect();

        tracing::debug!(unblocked = ?newly_unblocked, "Issues unblocked by close");

        let mut unblocked = Vec::new();
        for uid in newly_unblocked {
            let issue_result = storage_ctx.storage.get_issue(&uid);
            match preserve_blocked_cache_on_error(
                &mut storage_ctx.storage,
                cache_dirty,
                "close",
                issue_result,
            ) {
                Ok(Some(issue)) if issue.status.is_active() => {
                    unblocked.push(UnblockedIssue {
                        id: issue.id,
                        title: issue.title,
                        priority: issue.priority.0,
                    });
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        issue_id = %uid,
                        error = %error,
                        "Skipping suggest-next candidate after committed close because issue lookup failed"
                    );
                }
            }
        }
        unblocked
    } else {
        Vec::new()
    };

    storage_ctx.flush_no_db_if_dirty()?;
    if auto_flush_external && let Err(error) = storage_ctx.auto_flush_if_enabled() {
        report_auto_flush_failure(
            ctx,
            &storage_ctx.paths.beads_dir,
            &storage_ctx.paths.jsonl_path,
            &error,
        );
    }

    Ok(CloseExecution {
        closed: closed_issues,
        skipped: skipped_issues,
        unblocked: unblocked_issues,
        ordered_outcomes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands;
    use crate::config::CliOverrides;
    use crate::model::{DependencyType, Issue, IssueType, Priority, Status};
    use crate::output::OutputContext;
    use crate::storage::SqliteStorage;
    use chrono::Utc;
    use std::env;
    use std::path::PathBuf;

    use tempfile::TempDir;

    struct DirGuard {
        previous: PathBuf,
    }

    impl DirGuard {
        fn new(target: &std::path::Path) -> Self {
            let previous = env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"));
            env::set_current_dir(target).expect("set current dir");
            Self { previous }
        }
    }

    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.previous);
        }
    }

    fn make_issue(id: &str, title: &str) -> Issue {
        let now = Utc::now();
        Issue {
            id: id.to_string(),
            title: title.to_string(),
            status: Status::Open,
            priority: Priority::MEDIUM,
            issue_type: IssueType::Task,
            created_at: now,
            updated_at: now,
            ..Issue::default()
        }
    }

    // =========================================================================
    // CloseArgs tests
    // =========================================================================

    #[test]
    fn test_close_args_default() {
        let args = CloseArgs::default();
        assert!(args.ids.is_empty());
        assert!(args.reason.is_none());
        assert!(!args.force);
        assert!(args.session.is_none());
        assert!(!args.suggest_next);
    }

    #[test]
    fn test_close_args_with_all_fields() {
        let args = CloseArgs {
            ids: vec!["bd-abc".to_string(), "bd-xyz".to_string()],
            reason: Some("Fixed in PR #123".to_string()),
            force: true,
            session: Some("session-456".to_string()),
            suggest_next: true,
            agent_name: Some("agent-1".to_string()),
            harness: Some("codex-cli".to_string()),
            model: Some("gpt-5".to_string()),
            bypass_policy: true,
            bypass_reason: Some("Manual override approved".to_string()),
        };
        assert_eq!(args.ids.len(), 2);
        assert_eq!(args.ids[0], "bd-abc");
        assert_eq!(args.reason.as_deref(), Some("Fixed in PR #123"));
        assert!(args.force);
        assert_eq!(args.session.as_deref(), Some("session-456"));
        assert!(args.suggest_next);
        assert_eq!(args.agent_name.as_deref(), Some("agent-1"));
        assert_eq!(args.harness.as_deref(), Some("codex-cli"));
        assert_eq!(args.model.as_deref(), Some("gpt-5"));
        assert!(args.bypass_policy);
        assert_eq!(
            args.bypass_reason.as_deref(),
            Some("Manual override approved")
        );
    }

    // =========================================================================
    // CloseResult serialization tests
    // =========================================================================

    #[test]
    fn test_close_result_serialization_empty_skipped_omitted() {
        let result = CloseResult {
            closed: vec![ClosedIssue {
                id: "bd-123".to_string(),
                title: "Test issue".to_string(),
                status: "closed".to_string(),
                closed_at: "2026-01-01T00:00:00Z".to_string(),
                close_reason: None,
            }],
            skipped: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        // Empty skipped should be omitted due to skip_serializing_if
        assert!(!json.contains("\"skipped\""));
        assert!(json.contains("\"closed\""));
    }

    #[test]
    fn test_close_result_serialization_with_skipped() {
        let result = CloseResult {
            closed: vec![],
            skipped: vec![SkippedIssue {
                id: "bd-456".to_string(),
                reason: "already closed".to_string(),
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"skipped\""));
        assert!(json.contains("\"reason\":\"already closed\""));
    }

    #[test]
    fn test_close_result_roundtrip() {
        let result = CloseResult {
            closed: vec![
                ClosedIssue {
                    id: "bd-a".to_string(),
                    title: "First".to_string(),
                    status: "closed".to_string(),
                    closed_at: "2026-01-01T00:00:00Z".to_string(),
                    close_reason: Some("Done".to_string()),
                },
                ClosedIssue {
                    id: "bd-b".to_string(),
                    title: "Second".to_string(),
                    status: "closed".to_string(),
                    closed_at: "2026-01-02T00:00:00Z".to_string(),
                    close_reason: None,
                },
            ],
            skipped: vec![SkippedIssue {
                id: "bd-c".to_string(),
                reason: "blocked by: bd-d".to_string(),
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: CloseResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.closed.len(), 2);
        assert_eq!(parsed.skipped.len(), 1);
        assert_eq!(parsed.closed[0].id, "bd-a");
        assert_eq!(parsed.closed[0].close_reason.as_deref(), Some("Done"));
        assert!(parsed.closed[1].close_reason.is_none());
    }

    // =========================================================================
    // CloseWithSuggestResult serialization tests
    // =========================================================================

    #[test]
    fn test_close_with_suggest_result_serialization() {
        let result = CloseWithSuggestResult {
            closed: vec![ClosedIssue {
                id: "bd-parent".to_string(),
                title: "Parent task".to_string(),
                status: "closed".to_string(),
                closed_at: "2026-01-15T10:00:00Z".to_string(),
                close_reason: Some("Completed".to_string()),
            }],
            skipped: vec![],
            unblocked: vec![
                UnblockedIssue {
                    id: "bd-child1".to_string(),
                    title: "Child task 1".to_string(),
                    priority: 1,
                },
                UnblockedIssue {
                    id: "bd-child2".to_string(),
                    title: "Child task 2".to_string(),
                    priority: 2,
                },
            ],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"unblocked\""));
        assert!(json.contains("\"bd-child1\""));
        assert!(json.contains("\"bd-child2\""));
        assert!(json.contains("\"priority\":1"));
        assert!(json.contains("\"priority\":2"));
        // Empty skipped should be omitted
        assert!(!json.contains("\"skipped\""));
    }

    #[test]
    fn test_close_with_suggest_result_empty_unblocked() {
        let result = CloseWithSuggestResult {
            closed: vec![],
            skipped: vec![SkippedIssue {
                id: "bd-x".to_string(),
                reason: "not found".to_string(),
            }],
            unblocked: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        // unblocked is not marked skip_serializing_if, so it should appear as empty array
        assert!(json.contains("\"unblocked\":[]"));
        assert!(json.contains("\"skipped\""));
    }

    // =========================================================================
    // ClosedIssue serialization tests
    // =========================================================================

    #[test]
    fn test_closed_issue_serialization_with_reason() {
        let issue = ClosedIssue {
            id: "bd-test".to_string(),
            title: "Test issue".to_string(),
            status: "closed".to_string(),
            closed_at: "2026-01-17T08:00:00Z".to_string(),
            close_reason: Some("Fixed in commit abc123".to_string()),
        };
        let json = serde_json::to_string(&issue).unwrap();
        assert!(json.contains("\"close_reason\":\"Fixed in commit abc123\""));
    }

    #[test]
    fn test_closed_issue_serialization_without_reason() {
        let issue = ClosedIssue {
            id: "bd-test".to_string(),
            title: "Test issue".to_string(),
            status: "closed".to_string(),
            closed_at: "2026-01-17T08:00:00Z".to_string(),
            close_reason: None,
        };
        let json = serde_json::to_string(&issue).unwrap();
        // close_reason should be omitted due to skip_serializing_if
        assert!(!json.contains("close_reason"));
    }

    #[test]
    fn test_closed_issue_all_fields() {
        let issue = ClosedIssue {
            id: "beads_rust-xyz".to_string(),
            title: "Multi-word title with special chars: <>&".to_string(),
            status: "closed".to_string(),
            closed_at: "2026-12-31T23:59:59Z".to_string(),
            close_reason: Some("End of year cleanup".to_string()),
        };
        let json = serde_json::to_string(&issue).unwrap();
        let parsed: ClosedIssue = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "beads_rust-xyz");
        assert!(parsed.title.contains("<>&"));
        assert_eq!(parsed.status, "closed");
        assert!(parsed.closed_at.contains("2026-12-31"));
    }

    #[test]
    fn close_human_messages_sanitize_terminal_controls() {
        let closed = ClosedIssue {
            id: "bd-close\x1b[2J".to_string(),
            title: "bad\rtitle\x08".to_string(),
            status: "closed".to_string(),
            closed_at: "2026-12-31T23:59:59Z".to_string(),
            close_reason: Some("done\nnext\x07\u{9b}".to_string()),
        };
        let skipped = SkippedIssue {
            id: "bd-skip\x1b[2J".to_string(),
            reason: "blocked\rby\nterminal\x07".to_string(),
        };
        let unblocked = UnblockedIssue {
            id: "bd-unblock\x1b[2J".to_string(),
            title: "ready\nlater\x08".to_string(),
            priority: 1,
        };

        let close_message = close_human_message(&closed);
        let skipped_message = skipped_human_message(&skipped);
        let unblocked_line = unblocked_human_line(&unblocked);

        for text in [&close_message, &skipped_message, &unblocked_line] {
            assert!(!text.chars().any(char::is_control));
            assert!(text.contains("\\u{1b}[2J"));
        }
        assert!(close_message.contains("\\r"));
        assert!(close_message.contains("\\u{8}"));
        assert!(close_message.contains("\\n"));
        assert!(close_message.contains("\\u{7}"));
        assert!(close_message.contains("\\u{9b}"));
        assert!(skipped_message.contains("\\r"));
        assert!(skipped_message.contains("\\n"));
        assert!(skipped_message.contains("\\u{7}"));
        assert!(unblocked_line.contains("\\n"));
        assert!(unblocked_line.contains("\\u{8}"));
    }

    #[test]
    fn reorder_routed_items_sanitizes_missing_input_error() {
        let requested = vec!["bd-close\x1b[2J\nbad".to_string(), "bd-ok".to_string()];
        let routed_items = vec![(vec!["bd-ok".to_string()], vec!["ok"])];

        let err =
            reorder_routed_items_by_requested_inputs(&requested, routed_items, "close routing")
                .unwrap_err();

        assert!(
            matches!(err, BeadsError::Internal { .. }),
            "unexpected error: {err:?}"
        );
        if let BeadsError::Internal { message } = err {
            assert!(!message.chars().any(char::is_control));
            assert!(message.contains("\\u{1b}[2J"));
            assert!(message.contains("\\n"));
        }
    }

    #[test]
    fn reorder_routed_items_sanitizes_unexpected_input_error() {
        let requested = vec!["bd-ok".to_string()];
        let routed_items = vec![(vec!["bd-close\x1b[2J\nbad".to_string()], vec!["bad"])];

        let err =
            reorder_routed_items_by_requested_inputs(&requested, routed_items, "close routing")
                .unwrap_err();

        assert!(
            matches!(err, BeadsError::Internal { .. }),
            "unexpected error: {err:?}"
        );
        if let BeadsError::Internal { message } = err {
            assert!(!message.chars().any(char::is_control));
            assert!(message.contains("\\u{1b}[2J"));
            assert!(message.contains("\\n"));
        }
    }

    // =========================================================================
    // SkippedIssue serialization tests
    // =========================================================================

    #[test]
    fn test_skipped_issue_serialization() {
        let skipped = SkippedIssue {
            id: "bd-skip".to_string(),
            reason: "already closed".to_string(),
        };
        let json = serde_json::to_string(&skipped).unwrap();
        assert!(json.contains("\"id\":\"bd-skip\""));
        assert!(json.contains("\"reason\":\"already closed\""));
    }

    #[test]
    fn test_skipped_issue_blocked_reason() {
        let skipped = SkippedIssue {
            id: "bd-blocked".to_string(),
            reason: "blocked by: bd-dep1, bd-dep2".to_string(),
        };
        let json = serde_json::to_string(&skipped).unwrap();
        let parsed: SkippedIssue = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "bd-blocked");
        assert!(parsed.reason.contains("bd-dep1"));
        assert!(parsed.reason.contains("bd-dep2"));
    }

    // =========================================================================
    // UnblockedIssue serialization tests
    // =========================================================================

    #[test]
    fn test_unblocked_issue_serialization() {
        let unblocked = UnblockedIssue {
            id: "bd-next".to_string(),
            title: "Next task".to_string(),
            priority: 1,
        };
        let json = serde_json::to_string(&unblocked).unwrap();
        assert!(json.contains("\"id\":\"bd-next\""));
        assert!(json.contains("\"title\":\"Next task\""));
        assert!(json.contains("\"priority\":1"));
    }

    #[test]
    fn test_unblocked_issue_priority_boundaries() {
        for priority in [0, 1, 2, 3, 4] {
            let unblocked = UnblockedIssue {
                id: format!("bd-p{priority}"),
                title: format!("Priority {priority} task"),
                priority,
            };
            let json = serde_json::to_string(&unblocked).unwrap();
            let parsed: UnblockedIssue = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.priority, priority);
        }
    }

    // =========================================================================
    // Edge case tests
    // =========================================================================

    #[test]
    fn test_close_result_multiple_closed_multiple_skipped() {
        let result = CloseResult {
            closed: vec![
                ClosedIssue {
                    id: "bd-1".to_string(),
                    title: "Task 1".to_string(),
                    status: "closed".to_string(),
                    closed_at: "2026-01-01T00:00:00Z".to_string(),
                    close_reason: None,
                },
                ClosedIssue {
                    id: "bd-2".to_string(),
                    title: "Task 2".to_string(),
                    status: "closed".to_string(),
                    closed_at: "2026-01-01T00:00:01Z".to_string(),
                    close_reason: Some("Batch close".to_string()),
                },
            ],
            skipped: vec![
                SkippedIssue {
                    id: "bd-3".to_string(),
                    reason: "issue not found".to_string(),
                },
                SkippedIssue {
                    id: "bd-4".to_string(),
                    reason: "already tombstone".to_string(),
                },
            ],
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: CloseResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.closed.len(), 2);
        assert_eq!(parsed.skipped.len(), 2);
    }

    #[test]
    fn test_render_close_json_preserves_bare_array_for_pure_success() {
        let json = build_close_json_payload(
            &CloseArgs::default(),
            vec![ClosedIssue {
                id: "bd-1".to_string(),
                title: "Task 1".to_string(),
                status: "closed".to_string(),
                closed_at: "2026-01-01T00:00:00Z".to_string(),
                close_reason: Some("done".to_string()),
            }],
            vec![],
            vec![],
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
    }

    #[test]
    fn test_close_result_shape_with_skipped_is_wrapped() {
        let json = build_close_json_payload(
            &CloseArgs::default(),
            vec![ClosedIssue {
                id: "bd-1".to_string(),
                title: "Task 1".to_string(),
                status: "closed".to_string(),
                closed_at: "2026-01-01T00:00:00Z".to_string(),
                close_reason: Some("done".to_string()),
            }],
            vec![SkippedIssue {
                id: "bd-2".to_string(),
                reason: "blocked by: bd-3".to_string(),
            }],
            vec![],
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_object());
        assert_eq!(parsed["closed"][0]["id"], "bd-1");
        assert_eq!(parsed["skipped"][0]["id"], "bd-2");
    }

    #[test]
    fn test_close_args_clone() {
        let args = CloseArgs {
            ids: vec!["bd-clone".to_string()],
            reason: Some("Clone test".to_string()),
            force: true,
            session: Some("sess".to_string()),
            suggest_next: true,
            agent_name: Some("agent-clone".to_string()),
            harness: Some("harness-clone".to_string()),
            model: Some("model-clone".to_string()),
            bypass_policy: true,
            bypass_reason: Some("Clone bypass reason".to_string()),
        };
        let cloned = args.clone();
        assert_eq!(cloned.ids, args.ids);
        assert_eq!(cloned.reason, args.reason);
        assert_eq!(cloned.force, args.force);
        assert_eq!(cloned.session, args.session);
        assert_eq!(cloned.suggest_next, args.suggest_next);
        assert_eq!(cloned.agent_name, args.agent_name);
        assert_eq!(cloned.harness, args.harness);
        assert_eq!(cloned.model, args.model);
        assert_eq!(cloned.bypass_policy, args.bypass_policy);
        assert_eq!(cloned.bypass_reason, args.bypass_reason);
        assert_eq!(cloned.suggest_next, args.suggest_next);
    }

    #[test]
    fn test_close_args_debug_impl() {
        let args = CloseArgs::default();
        let debug_str = format!("{args:?}");
        assert!(debug_str.contains("CloseArgs"));
        assert!(debug_str.contains("ids"));
        assert!(debug_str.contains("reason"));
    }

    #[test]
    fn execute_with_args_closes_requested_blocker_chain_in_one_batch() {
        let _lock = crate::util::test_helpers::TEST_DIR_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let temp = TempDir::new().expect("tempdir");
        let ctx = OutputContext::from_flags(false, false, true);
        commands::init::execute(None, false, Some(temp.path()), &ctx).expect("init");

        let beads_dir = temp.path().join(".beads");
        let db_path = beads_dir.join("beads.db");
        let mut storage = SqliteStorage::open(&db_path).expect("storage");
        storage
            .create_issue(&make_issue("bd-blocker", "Batch blocker"), "tester")
            .expect("create blocker");
        storage
            .create_issue(&make_issue("bd-blocked", "Batch blocked"), "tester")
            .expect("create blocked");
        storage
            .add_dependency(
                "bd-blocked",
                "bd-blocker",
                DependencyType::Blocks.as_str(),
                "tester",
            )
            .expect("add dependency");
        storage.rebuild_blocked_cache(true).expect("rebuild cache");
        drop(storage);

        let _guard = DirGuard::new(temp.path());
        let args = CloseArgs {
            ids: vec!["bd-blocked".to_string(), "bd-blocker".to_string()],
            ..CloseArgs::default()
        };
        execute_with_args(&args, false, &CliOverrides::default(), &ctx).expect("close batch");

        let storage = SqliteStorage::open(&db_path).expect("reopen storage");
        let blocker = storage
            .get_issue("bd-blocker")
            .expect("get blocker")
            .expect("blocker exists");
        let blocked_issue = storage
            .get_issue("bd-blocked")
            .expect("get blocked")
            .expect("blocked exists");

        assert_eq!(blocker.status, Status::Closed);
        assert_eq!(blocked_issue.status, Status::Closed);
    }

    #[test]
    fn execute_with_args_closes_requested_dot_notation_child_with_parent() {
        let _lock = crate::util::test_helpers::TEST_DIR_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let temp = TempDir::new().expect("tempdir");
        let ctx = OutputContext::from_flags(false, false, true);
        commands::init::execute(None, false, Some(temp.path()), &ctx).expect("init");

        let beads_dir = temp.path().join(".beads");
        let db_path = beads_dir.join("beads.db");
        let mut storage = SqliteStorage::open(&db_path).expect("storage");
        storage
            .create_issue(&make_issue("bd-parent", "Legacy parent"), "tester")
            .expect("create parent");
        storage
            .create_issue(&make_issue("bd-parent.1", "Legacy child"), "tester")
            .expect("create dot child");
        drop(storage);

        let _guard = DirGuard::new(temp.path());
        let args = CloseArgs {
            ids: vec!["bd-parent".to_string(), "bd-parent.1".to_string()],
            ..CloseArgs::default()
        };
        execute_with_args(&args, false, &CliOverrides::default(), &ctx)
            .expect("close parent and dot child in one batch");

        let storage = SqliteStorage::open(&db_path).expect("reopen storage");
        let parent = storage
            .get_issue("bd-parent")
            .expect("get parent")
            .expect("parent exists");
        let child = storage
            .get_issue("bd-parent.1")
            .expect("get child")
            .expect("child exists");

        assert_eq!(parent.status, Status::Closed);
        assert_eq!(child.status, Status::Closed);
    }

    #[test]
    fn execute_with_args_keeps_parent_blocked_by_unrequested_dot_notation_child() {
        let _lock = crate::util::test_helpers::TEST_DIR_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let temp = TempDir::new().expect("tempdir");
        let ctx = OutputContext::from_flags(false, false, true);
        commands::init::execute(None, false, Some(temp.path()), &ctx).expect("init");

        let beads_dir = temp.path().join(".beads");
        let db_path = beads_dir.join("beads.db");
        let mut storage = SqliteStorage::open(&db_path).expect("storage");
        storage
            .create_issue(&make_issue("bd-parent", "Legacy parent"), "tester")
            .expect("create parent");
        storage
            .create_issue(&make_issue("bd-parent.1", "Legacy child"), "tester")
            .expect("create dot child");
        drop(storage);

        let _guard = DirGuard::new(temp.path());
        let args = CloseArgs {
            ids: vec!["bd-parent".to_string()],
            ..CloseArgs::default()
        };
        let err = execute_with_args(&args, true, &CliOverrides::default(), &ctx)
            .expect_err("parent-only close should remain blocked by dot child");
        assert!(matches!(err, BeadsError::NothingToDo { .. }));

        let storage = SqliteStorage::open(&db_path).expect("reopen storage");
        let parent = storage
            .get_issue("bd-parent")
            .expect("get parent")
            .expect("parent exists");
        let child = storage
            .get_issue("bd-parent.1")
            .expect("get child")
            .expect("child exists");

        assert_eq!(parent.status, Status::Open);
        assert_eq!(child.status, Status::Open);
    }

    #[test]
    fn execute_with_args_returns_nothing_to_do_when_all_requested_issues_are_skipped() {
        let _lock = crate::util::test_helpers::TEST_DIR_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let temp = TempDir::new().expect("tempdir");
        let ctx = OutputContext::from_flags(false, false, true);
        commands::init::execute(None, false, Some(temp.path()), &ctx).expect("init");

        let beads_dir = temp.path().join(".beads");
        let db_path = beads_dir.join("beads.db");
        let mut storage = SqliteStorage::open(&db_path).expect("storage");
        let mut issue = make_issue("bd-closed", "Already closed");
        issue.status = Status::Closed;
        issue.closed_at = Some(Utc::now());
        storage
            .create_issue(&issue, "tester")
            .expect("create closed issue");

        let _guard = DirGuard::new(temp.path());
        let args = CloseArgs {
            ids: vec!["bd-closed".to_string()],
            ..CloseArgs::default()
        };

        let err = execute_with_args(&args, true, &CliOverrides::default(), &ctx)
            .expect_err("all-skipped close should fail");
        assert!(matches!(err, BeadsError::NothingToDo { .. }));
    }
}
