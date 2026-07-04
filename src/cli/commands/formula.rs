//! Formula Language CLI commands.
//!
//! Commands:
//! - `br formula validate <file>` — Validate a formula file
//! - `br formula expand <file>` — Preview the steps that would be created
//! - `br formula apply <file>` — Create issues from formula steps

use crate::config;
use crate::error::{BeadsError, Result};
use crate::formula::Parser;
use crate::model::{Issue, IssueType, Priority, Status};
use std::borrow::Cow;
use crate::output::OutputContext;
use crate::validation::IssueValidator;
use std::collections::HashMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// FormulaCommands — clap args for `br formula` subcommand group
// ---------------------------------------------------------------------------

/// Formula Language commands
#[derive(clap::Subcommand, Debug)]
pub enum FormulaCommands {
    /// Validate a .formula.json or .formula.toml file
    Validate(FormulaValidateArgs),
    /// Show what issues would be created (dry-run preview)
    Expand(FormulaExpandArgs),
    /// Create issues from a formula (apply)
    Apply(FormulaApplyArgs),
}

/// Arguments for `br formula validate <file>`
#[derive(clap::Args, Debug)]
pub struct FormulaValidateArgs {
    /// Path to the formula file (.formula.json or .formula.toml)
    pub file: PathBuf,

    /// Variable overrides in key=value format (can be repeated)
    #[arg(long = "var", short = 'v', value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

    /// Output format (text, json)
    #[arg(long, short)]
    pub format: Option<crate::cli::OutputFormatBasic>,

    /// Output raw JSON
    #[arg(long)]
    pub json: bool,

    /// Output machine-readable JSON
    #[arg(long)]
    pub robot: bool,
}

/// Arguments for `br formula expand <file>`
#[derive(clap::Args, Debug)]
pub struct FormulaExpandArgs {
    /// Path to the formula file
    pub file: PathBuf,

    /// Variable overrides in key=value format (can be repeated)
    #[arg(long = "var", short = 'v', value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

    /// Output format (text, json)
    #[arg(long, short)]
    pub format: Option<crate::cli::OutputFormatBasic>,

    /// Output raw JSON
    #[arg(long)]
    pub json: bool,

    /// Output machine-readable JSON
    #[arg(long)]
    pub robot: bool,
}

/// Arguments for `br formula apply <file>`
#[derive(clap::Args, Debug)]
pub struct FormulaApplyArgs {
    /// Path to the formula file to apply
    pub file: PathBuf,

    /// Variable overrides in key=value format (can be repeated)
    #[arg(long = "var", short = 'v', value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

    /// Dry-run: show what would be created without writing to the database
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (text, json)
    #[arg(long, short)]
    pub format: Option<crate::cli::OutputFormatBasic>,

    /// Output raw JSON
    #[arg(long)]
    pub json: bool,

    /// Output machine-readable JSON
    #[arg(long)]
    pub robot: bool,
}

// ---------------------------------------------------------------------------
// Execute functions
// ---------------------------------------------------------------------------

/// Execute a formula subcommand.
pub fn execute(
    command: &FormulaCommands,
    overrides: &crate::config::CliOverrides,
    output_ctx: &OutputContext,
) -> crate::Result<()> {
    #[allow(clippy::wildcard_enum_match_arm)]
    match command {
        FormulaCommands::Validate(args) => execute_validate(args, output_ctx),
        FormulaCommands::Expand(args) => execute_expand(args, output_ctx),
        FormulaCommands::Apply(args) => execute_apply(args, overrides, output_ctx),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (parse, resolve, substitute)
// ---------------------------------------------------------------------------

/// Parse the formula from file, validate, resolve extends, and return the
/// resolved formula + steps.  Returns a `BeadsError::Config` on any failure.
fn parse_and_resolve(
    file: &PathBuf,
    var_overrides: &[String],
) -> std::result::Result<(crate::formula::Formula, Vec<crate::formula::Step>), BeadsError> {
    let mut parser = Parser::new(vec![]);

    let formula = parser
        .parse_file(file)
        .map_err(|e| BeadsError::Config(format!("Failed to parse formula: {e}")))?;

    formula
        .validate()
        .map_err(|e| BeadsError::Config(format!("Formula validation failed: {e}")))?;

    let formula = if formula.extends.is_empty() {
        formula
    } else {
        parser
            .resolve(&formula)
            .map_err(|e| BeadsError::Config(format!("Formula resolution failed: {e}")))?
    };

    // Apply variable overrides
    let mut vars: HashMap<String, String> = HashMap::new();
    for kv in var_overrides {
        if let Some((key, value)) = kv.split_once('=') {
            vars.insert(key.to_string(), value.to_string());
        } else {
            return Err(BeadsError::Config(format!(
                "Invalid variable override {kv:?}: expected KEY=VALUE format"
            )));
        }
    }

    let steps = formula.steps.as_deref().unwrap_or_default().to_vec();

    // Create substituted steps
    let steps = steps
        .into_iter()
        .map(|s| substitute_step(s, &vars))
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok((formula, steps))
}

/// Perform `{{variable}}` substitution in a step's string fields.
fn substitute_step(
    step: crate::formula::Step,
    vars: &HashMap<String, String>,
) -> std::result::Result<crate::formula::Step, BeadsError> {
    let sub = |s: Option<String>| -> Result<Option<String>> {
        match s {
            None => Ok(None),
            Some(val) => Ok(Some(substitute_str(&val, vars)?)),
        }
    };

    Ok(crate::formula::Step {
        title: sub(step.title)?,
        description: sub(step.description)?,
        notes: sub(step.notes)?,
        assignee: sub(step.assignee)?,
        ..step
    })
}

fn substitute_str(s: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut result = s.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{{{key}}}}}"), value);
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

/// Validate a formula file.
fn execute_validate(args: &FormulaValidateArgs, output_ctx: &OutputContext) -> crate::Result<()> {
    let (_formula, steps) = parse_and_resolve(&args.file, &args.vars)?;

    let use_json = args.json || args.robot || output_ctx.is_json();

    if use_json {
        let output = serde_json::json!({
            "valid": true,
            "step_count": steps.len(),
        });
        output_ctx.print(&serde_json::to_string_pretty(&output).unwrap_or_default());
    } else {
        output_ctx.print(&format!(
            "Formula is valid: {} step(s) would be created",
            steps.len()
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// expand (preview)
// ---------------------------------------------------------------------------

/// Preview the steps that would be created from a formula.
fn execute_expand(args: &FormulaExpandArgs, output_ctx: &OutputContext) -> crate::Result<()> {
    let (resolved, steps) = parse_and_resolve(&args.file, &args.vars)?;

    let use_json = args.json || args.robot || output_ctx.is_json();

    if use_json {
        let output = serde_json::json!({
            "formula": resolved.formula,
            "type": format!("{:?}", resolved.r#type),
            "description": resolved.description,
            "step_count": steps.len(),
            "steps": steps.iter().map(|s| {
                let v = serde_json::to_value(s).unwrap_or_default();
                v
            }).collect::<Vec<_>>(),
            "vars": resolved.vars.unwrap_or_default(),
        });
        output_ctx.print(&serde_json::to_string_pretty(&output).unwrap_or_default());
    } else {
        output_ctx.print(&format!(
            "Formula: {} ({} steps)",
            resolved.formula,
            steps.len()
        ));
        if let Some(desc) = resolved.description.as_deref() {
            output_ctx.print(&format!("  Description: {desc}"));
        }
        for (i, step) in steps.iter().enumerate() {
            output_ctx.print(&format!(
                "  {}.{}: {}",
                i + 1,
                step.id,
                step.title.as_deref().unwrap_or("(untitled)")
            ));
            if !step.depends_on.is_empty() {
                output_ctx.print(&format!("       Depends on: {}", step.depends_on.join(", ")));
            }
            if !step.needs.is_empty() {
                output_ctx.print(&format!("       Needs: {}", step.needs.join(", ")));
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// apply
// ---------------------------------------------------------------------------

/// Create issues from a formula.
fn execute_apply(
    args: &FormulaApplyArgs,
    overrides: &crate::config::CliOverrides,
    output_ctx: &OutputContext,
) -> crate::Result<()> {
    let (resolved, steps) = parse_and_resolve(&args.file, &args.vars)?;

    let use_json = args.json || args.robot || output_ctx.is_json();
    let now = chrono::Utc::now();

    // In dry-run mode, just preview what would be created.
    if args.dry_run {
        if use_json {
            let output = serde_json::json!({
                "dry_run": true,
                "formula": resolved.formula,
                "description": resolved.description,
                "step_count": steps.len(),
                "steps": steps.iter().map(|s| {
                    let mut obj = serde_json::json!({
                        "id": s.id,
                        "title": s.title,
                        "type": s.r#type,
                        "priority": s.priority,
                        "labels": s.labels,
                        "depends_on": s.depends_on,
                        "needs": s.needs,
                        "assignee": s.assignee,
                    });
                    if let Some(gate) = &s.gate {
                        obj["await_type"] = serde_json::json!(gate.r#type);
                        if let Some(ref aid) = gate.await_id {
                            obj["await_id"] = serde_json::json!(aid);
                        }
                        if let Some(ref t) = gate.timeout {
                            obj["timeout"] = serde_json::json!(t);
                        }
                    }
                    obj
                }).collect::<Vec<_>>(),
            });
            output_ctx.print(&serde_json::to_string_pretty(&output).unwrap_or_default());
        } else {
            output_ctx.print(&format!(
                "[DRY-RUN] Would create {} issue(s) from '{}'",
                steps.len(),
                resolved.formula
            ));
            for (i, step) in steps.iter().enumerate() {
                output_ctx.print(&format!(
                    "  {}.{}: {}",
                    i + 1,
                    step.id,
                    step.title.as_deref().unwrap_or("(untitled)")
                ));
                if !step.depends_on.is_empty() {
                    output_ctx
                        .print(&format!("       Depends on: {}", step.depends_on.join(", ")));
                }
                if let Some(gate) = &step.gate {
                    output_ctx.print(&format!(
                        "       Gate: await_type={}, await_id={}, timeout={}",
                        gate.r#type,
                        gate.await_id.as_deref().unwrap_or("-"),
                        gate.timeout.as_deref().unwrap_or("-"),
                    ));
                }
            }
        }
        return Ok(());
    }

    // --- Real execution: open storage and create issues ---

    let beads_dir = config::discover_beads_dir_with_cli(overrides)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, overrides)?;
    let storage = &mut storage_ctx.storage;

    // Resolve actor for audit trail
    let actor = "formula";

    // Build a map of step ID -> created issue ID
    let mut step_to_issue: HashMap<String, Issue> = HashMap::new();
    // Map of step ID -> created gate issue (only for steps with a gate).
    let mut step_to_gate_issue: HashMap<String, Issue> = HashMap::new();

    // Phase 1: Create all issues
    for step in &steps {
        let title = step
            .title
            .clone()
            .unwrap_or_else(|| format!("Step {}", step.id));
        let issue_type = parse_issue_type(step.r#type.as_deref());
        let priority = step.priority.map(Priority).unwrap_or(Priority::MEDIUM);

        // Gate fields: when a step has a `gate` defined, propagate the async
        // wait information onto the created issue so downstream consumers
        // (gate list, close policy, etc.) can see it.
        let (await_type, await_id, timeout_seconds) = match &step.gate {
            Some(gate) => {
                let tid = gate
                    .timeout
                    .as_deref()
                    .and_then(parse_timeout_seconds);
                (Some(gate.r#type.clone()), gate.await_id.clone(), tid)
            }
            None => (None, None, None),
        };

        // Issue #70: When a step has a gate, also create a separate
        // IssueType::Gate issue so the gate is a first-class work item that
        // can be tracked, assigned, and reported on independently.
        if step.gate.is_some() {
            let gate_title: Cow<'_, str> = match step.title.as_deref() {
                Some(t) => Cow::Owned(format!("Gate: {t}")),
                None => Cow::Borrowed("Gate"),
            };
            let gate_desc = step.description.clone().or_else(|| {
                step.gate.as_ref().map(|g| {
                    let mut d = format!("Async gate ({})", g.r#type);
                    if let Some(ref id) = g.await_id {
                        d.push_str(&format!(" — {id}"));
                    }
                    if let Some(ref t) = g.timeout {
                        d.push_str(&format!(" (timeout: {t})"));
                    }
                    d
                })
            });
            let gate_issue = Issue {
                id: String::new(),
                title: gate_title.into_owned(),
                description: gate_desc,
                status: Status::Open,
                priority,
                issue_type: IssueType::Gate,
                labels: step.labels.clone(),
                created_at: now,
                updated_at: now,
                await_type: await_type.clone(),
                await_id: await_id.clone(),
                timeout_seconds,
                ..Default::default()
            };
            IssueValidator::validate(&gate_issue)
                .map_err(BeadsError::from_validation_errors)?;
            storage.create_issue(&gate_issue, actor)?;
            step_to_gate_issue.insert(step.id.clone(), gate_issue);
        }

        let new_issue = Issue {
            id: String::new(), // storage will assign
            title,
            description: step.description.clone(),
            notes: step.notes.clone(),
            status: Status::Open,
            priority,
            issue_type,
            labels: step.labels.clone(),
            assignee: step.assignee.clone(),
            created_at: now,
            updated_at: now,
            await_type,
            await_id,
            timeout_seconds,
            ..Default::default()
        };

        // Validate before creating
        IssueValidator::validate(&new_issue).map_err(BeadsError::from_validation_errors)?;

        storage.create_issue(&new_issue, actor)?;

        step_to_issue.insert(step.id.clone(), new_issue);
    }

    // Phase 2: Create dependencies between issues that reference each other
    let mut dep_count = 0usize;

    // 2a. Create waits-for edges from gated step issues to their gate issues.
    for (step_id, gate_issue) in &step_to_gate_issue {
        let Some(step_issue) = step_to_issue.get(step_id.as_str()) else {
            continue;
        };
        storage.add_dependency(&step_issue.id, &gate_issue.id, "waits-for", actor)?;
        dep_count += 1;
    }

    // 2b. Create blocks edges for step-level depends_on and needs.
    for step in &steps {
        let Some(issue) = step_to_issue.get(&step.id) else {
            continue;
        };
        let deps: Vec<&String> = step.depends_on.iter().chain(step.needs.iter()).collect();

        for dep_id in deps {
            if step_to_issue.contains_key(dep_id) {
                storage.add_dependency(&issue.id, dep_id, "blocks", actor)?;
                dep_count += 1;
            }
        }
    }

    // Report results
    if use_json {
        let gate_count = step_to_gate_issue.len();
        let mut issue_ids: Vec<String> = step_to_issue
            .values()
            .map(|i| i.id.clone())
            .collect();
        let gate_ids: Vec<String> = step_to_gate_issue
            .values()
            .map(|i| i.id.clone())
            .collect();
        issue_ids.extend(gate_ids.iter().cloned());
        let output = serde_json::json!({
            "formula": resolved.formula,
            "issues_created": step_to_issue.len(),
            "gate_issues_created": gate_count,
            "dependencies_created": dep_count,
            "gated_issues": gate_count,
            "issue_ids": issue_ids,
        });
        output_ctx.print(&serde_json::to_string_pretty(&output).unwrap_or_default());
    } else {
        let gate_count = step_to_gate_issue.len();
        output_ctx.print(&format!(
            "Applied formula '{}': created {} issue(s) and {} dependency(ies) ({} gated)",
            resolved.formula,
            step_to_issue.len(),
            dep_count,
            gate_count,
        ));
        for (step_id, issue) in &step_to_issue {
            let gate_info = step_to_gate_issue
                .get(step_id)
                .map(|g| format!(" [gate: {}]", g.id))
                .unwrap_or_default();
            output_ctx.print(&format!("  {step_id} -> {}{gate_info}", issue.id));
        }
    }

    Ok(())
}

/// Parse a duration shorthand like "1h", "30m", "2d" into seconds.
fn parse_timeout_seconds(s: &str) -> Option<i64> {
    let s = s.trim();
    // Try to parse a numeric-only string as seconds directly.
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    let (num_str, unit) = s.split_at(s.len() - 1);
    let n: i64 = num_str.parse().ok()?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        "w" => n * 604800,
        _ => return None,
    };
    Some(secs)
}

/// Parse an optional type string into an IssueType.
fn parse_issue_type(s: Option<&str>) -> IssueType {
    match s {
        Some("task") | None => IssueType::Task,
        Some("bug") => IssueType::Bug,
        Some("feature") => IssueType::Feature,
        Some("epic") => IssueType::Epic,
        Some("chore") => IssueType::Chore,
        Some("question") => IssueType::Question,
        Some("docs") => IssueType::Docs,
        Some(other) => IssueType::Custom(other.to_string()),
    }
}
