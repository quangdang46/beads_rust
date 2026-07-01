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

    let mut formula = parser
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
                    serde_json::json!({
                        "id": s.id,
                        "title": s.title,
                        "type": s.r#type,
                        "priority": s.priority,
                        "labels": s.labels,
                        "depends_on": s.depends_on,
                        "needs": s.needs,
                        "assignee": s.assignee,
                    })
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

    // Phase 1: Create all issues
    for step in &steps {
        let title = step
            .title
            .clone()
            .unwrap_or_else(|| format!("Step {}", step.id));
        let issue_type = parse_issue_type(step.r#type.as_deref());
        let priority = step.priority.map(Priority).unwrap_or(Priority::MEDIUM);

        let mut new_issue = Issue {
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
            ..Default::default()
        };

        // Validate before creating
        IssueValidator::validate(&new_issue).map_err(BeadsError::from_validation_errors)?;

        storage.create_issue(&new_issue, actor)?;

        step_to_issue.insert(step.id.clone(), new_issue);
    }

    // Phase 2: Create dependencies between issues that reference each other
    let mut dep_count = 0usize;
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
        let output = serde_json::json!({
            "formula": resolved.formula,
            "issues_created": step_to_issue.len(),
            "dependencies_created": dep_count,
            "issue_ids": step_to_issue
                .values()
                .map(|i| i.id.clone())
                .collect::<Vec<_>>(),
        });
        output_ctx.print(&serde_json::to_string_pretty(&output).unwrap_or_default());
    } else {
        output_ctx.print(&format!(
            "Applied formula '{}': created {} issue(s) and {} dependency(ies)",
            resolved.formula,
            step_to_issue.len(),
            dep_count,
        ));
        for (step_id, issue) in &step_to_issue {
            output_ctx.print(&format!("  {step_id} -> {}", issue.id));
        }
    }

    Ok(())
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
