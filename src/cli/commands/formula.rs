//! Formula Language CLI commands.
//!
//! Commands:
//! - `br formula validate <file>` — Validate a formula file
//! - `br formula expand <file>` — Preview the steps that would be created
//! - `br formula apply <file>` — Create issues from formula steps

use crate::config;
use crate::error::{BeadsError, Result};
use crate::formula::Parser;
use crate::formula::types::FormulaType;
use crate::model::{Issue, IssueType, Priority, Status};
use crate::output::OutputContext;
use crate::validation::IssueValidator;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::vec::Vec;

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
    /// List available formulas from search paths
    List(FormulaListArgs),
    /// Show formula details, steps, and composition rules
    Show(FormulaShowArgs),
    /// Convert formula from JSON to TOML format
    Convert(FormulaConvertArgs),
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

/// Arguments for `br formula list`
#[derive(clap::Args, Debug)]
pub struct FormulaListArgs {
    /// Filter by type (workflow, expansion, aspect, convoy)
    #[arg(long)]
    pub r#type: Option<String>,

    /// Output raw JSON
    #[arg(long)]
    pub json: bool,

    /// Output machine-readable JSON
    #[arg(long)]
    pub robot: bool,
}

/// Arguments for `br formula show <name>`
#[derive(clap::Args, Debug)]
pub struct FormulaShowArgs {
    /// Formula name to show
    pub name: String,

    /// Output raw JSON
    #[arg(long)]
    pub json: bool,

    /// Output machine-readable JSON
    #[arg(long)]
    pub robot: bool,
}

/// Arguments for `br formula convert <file>`
#[derive(clap::Args, Debug)]
pub struct FormulaConvertArgs {
    /// Formula name or file path to convert
    pub target: Option<String>,

    /// Convert all JSON formulas found in search paths
    #[arg(long)]
    pub all: bool,

    /// Print TOML to stdout instead of writing a file
    #[arg(long)]
    pub stdout: bool,

    /// Delete the JSON file after successful conversion
    #[arg(long)]
    pub delete: bool,

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
        FormulaCommands::List(args) => execute_list(args, output_ctx),
        FormulaCommands::Show(args) => execute_show(args, output_ctx),
        FormulaCommands::Convert(args) => execute_convert(args, output_ctx),
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
                output_ctx.print(&format!(
                    "       Depends on: {}",
                    step.depends_on.join(", ")
                ));
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
                    output_ctx.print(&format!(
                        "       Depends on: {}",
                        step.depends_on.join(", ")
                    ));
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
                let tid = gate.timeout.as_deref().and_then(parse_timeout_seconds);
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
            IssueValidator::validate(&gate_issue).map_err(BeadsError::from_validation_errors)?;
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
        let mut issue_ids: Vec<String> = step_to_issue.values().map(|i| i.id.clone()).collect();
        let gate_ids: Vec<String> = step_to_gate_issue.values().map(|i| i.id.clone()).collect();
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

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

/// List available formulas from search paths.
fn execute_list(args: &FormulaListArgs, output_ctx: &OutputContext) -> crate::Result<()> {
    let use_json = args.json || args.robot || output_ctx.is_json();
    let search_paths = Parser::default_search_paths();

    #[derive(serde::Serialize)]
    struct ListEntry {
        name: String,
        r#type: String,
        description: String,
        source: String,
        steps: usize,
        vars: usize,
    }

    let mut seen: HashMap<String, bool> = HashMap::new();
    let mut entries: Vec<ListEntry> = Vec::new();

    for dir in &search_paths {
        if !dir.exists() {
            continue;
        }
        let dir_entries = match fs::read_dir(dir) {
            Ok(d) => d,
            Err(_) => continue,
        };

        for entry in dir_entries.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();

            if !fname.ends_with(".formula.toml") && !fname.ends_with(".formula.json") {
                continue;
            }

            // Derive formula name from filename (strip extension prefixes)
            let formula_name = fname
                .strip_suffix(".formula.toml")
                .or_else(|| fname.strip_suffix(".formula.json"))
                .unwrap_or(&fname)
                .to_string();

            if seen.contains_key(&formula_name) {
                continue; // Earlier paths shadow later ones
            }
            seen.insert(formula_name.clone(), true);

            // Parse the formula
            let mut parser = Parser::new(search_paths.clone());
            let formula = match parser.parse_file(&path) {
                Ok(f) => f,
                Err(_) => continue, // Skip invalid formulas
            };

            // Filter by type if specified
            if let Some(ref type_filter) = args.r#type {
                let ft: FormulaType = match type_filter.as_str() {
                    "workflow" => FormulaType::Workflow,
                    "expansion" => FormulaType::Expansion,
                    "aspect" => FormulaType::Aspect,
                    "convoy" => FormulaType::Convoy,
                    _ => {
                        return Err(BeadsError::Config(format!(
                            "Invalid type filter {:?}: must be workflow, expansion, aspect, or convoy",
                            type_filter
                        )));
                    }
                };
                if formula.r#type != ft {
                    continue;
                }
            }

            let type_str = format!("{:?}", formula.r#type).to_lowercase();
            let step_count = count_steps(formula.steps.as_deref().unwrap_or_default());
            let var_count = formula.vars.as_ref().map_or(0, |v| v.len());

            entries.push(ListEntry {
                name: formula_name,
                r#type: type_str,
                description: truncate_description(formula.description.as_deref().unwrap_or(""), 60),
                source: formula.source.unwrap_or_default(),
                steps: step_count,
                vars: var_count,
            });
        }
    }

    // Sort by name
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    if use_json {
        output_ctx.print(&serde_json::to_string_pretty(&entries).unwrap_or_default());
        return Ok(());
    }

    if entries.is_empty() {
        output_ctx.print("No formulas found.");
        output_ctx.print("Search paths:");
        for p in &search_paths {
            output_ctx.print(&format!("  {}", p.display()));
        }
        return Ok(());
    }

    output_ctx.print(&format!("Formulas ({} found)", entries.len()));

    // Group by type
    let mut by_type: HashMap<String, Vec<&ListEntry>> = HashMap::new();
    for e in &entries {
        by_type.entry(e.r#type.clone()).or_default().push(e);
    }

    let type_order = ["workflow", "expansion", "aspect", "convoy"];
    for t in &type_order {
        let Some(type_entries) = by_type.get(*t) else {
            continue;
        };
        output_ctx.print(&format!("  {}:", t));
        for e in type_entries {
            let var_info = if e.vars > 0 {
                format!(" ({} vars)", e.vars)
            } else {
                String::new()
            };
            output_ctx.print(&format!("    {:<25} {}{}", e.name, e.description, var_info));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

/// Show formula details.
fn execute_show(args: &FormulaShowArgs, output_ctx: &OutputContext) -> crate::Result<()> {
    let use_json = args.json || args.robot || output_ctx.is_json();
    let search_paths = Parser::default_search_paths();
    let mut parser = Parser::new(search_paths.clone());

    let formula = parser
        .load_by_name(&args.name)
        .map_err(|e| BeadsError::Config(format!("Formula {:?} not found: {}", args.name, e)))?;

    if use_json {
        output_ctx.print(&serde_json::to_string_pretty(&formula).unwrap_or_default());
        return Ok(());
    }

    output_ctx.print(&format!("Formula: {}", formula.formula));
    output_ctx.print(&format!("  Type: {:?}", formula.r#type));
    if let Some(ref desc) = formula.description {
        output_ctx.print(&format!("  Description: {}", desc));
    }
    if let Some(ref source) = formula.source {
        output_ctx.print(&format!("  Source: {}", source));
    }

    // Print extends
    if !formula.extends.is_empty() {
        output_ctx.print("  Extends:");
        for ext in &formula.extends {
            output_ctx.print(&format!("    - {}", ext));
        }
    }

    // Print variables
    if let Some(vars) = &formula.vars {
        if !vars.is_empty() {
            output_ctx.print("  Variables:");
            for v in vars {
                let mut attrs: Vec<String> = Vec::new();
                if v.required {
                    attrs.push("required".to_string());
                }
                if let Some(ref default) = v.default {
                    attrs.push(format!("default={:?}", default));
                }
                if !v.r#enum.is_empty() {
                    attrs.push(format!("enum=[{}]", v.r#enum.join(",")));
                }
                let attr_str = if attrs.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", attrs.join(", "))
                };
                let desc = v
                    .description
                    .as_ref()
                    .map(|d| format!(": {}", d))
                    .unwrap_or_default();
                output_ctx.print(&format!("    {{{{{}}}}}{}{}", v.name, desc, attr_str));
            }
        }
    }

    // Print steps
    if let Some(steps) = &formula.steps {
        if !steps.is_empty() {
            let count = count_steps(steps);
            output_ctx.print(&format!("  Steps ({}):", count));
            print_steps_tree(steps, "    ", output_ctx);
        }
    }

    // Print template (for expansion formulas)
    if let Some(template) = &formula.template {
        if !template.is_empty() {
            output_ctx.print(&format!("  Template ({} steps):", template.len()));
            print_steps_tree(template, "    ", output_ctx);
        }
    }

    // Print compose rules
    if let Some(ref compose) = formula.compose {
        let has_rules = compose.expand.as_ref().map_or(false, |e| !e.is_empty())
            || compose.r#map.as_ref().map_or(false, |m| !m.is_empty())
            || compose
                .bond_points
                .as_ref()
                .map_or(false, |b| !b.is_empty())
            || compose.aspects.as_ref().map_or(false, |a| !a.is_empty());

        if has_rules {
            output_ctx.print("  Composition:");

            if let Some(bond_points) = &compose.bond_points {
                if !bond_points.is_empty() {
                    output_ctx.print("    Bond Points:");
                    for bp in bond_points {
                        let loc = if let Some(ref after) = bp.after_step {
                            format!("after {}", after)
                        } else if let Some(ref before) = bp.before_step {
                            format!("before {}", before)
                        } else {
                            "standalone".to_string()
                        };
                        output_ctx.print(&format!("      - {} ({})", bp.id, loc));
                    }
                }
            }

            if let Some(expand_rules) = &compose.expand {
                if !expand_rules.is_empty() {
                    output_ctx.print("    Expansions:");
                    for e in expand_rules {
                        output_ctx.print(&format!("      - {} -> {}", e.target, e.with));
                    }
                }
            }

            if let Some(map_rules) = &compose.r#map {
                if !map_rules.is_empty() {
                    output_ctx.print("    Maps:");
                    for m in map_rules {
                        output_ctx.print(&format!("      - {} -> {}", m.select, m.with));
                    }
                }
            }

            if let Some(aspects) = &compose.aspects {
                if !aspects.is_empty() {
                    output_ctx.print(&format!("    Aspects: {}", aspects.join(", ")));
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// convert
// ---------------------------------------------------------------------------

/// Convert formula from JSON to TOML format.
fn execute_convert(args: &FormulaConvertArgs, output_ctx: &OutputContext) -> crate::Result<()> {
    let use_json = args.json || args.robot || output_ctx.is_json();

    if args.all {
        return convert_all_formulas(output_ctx, use_json);
    }

    let target = args
        .target
        .as_deref()
        .ok_or_else(|| BeadsError::Config("formula name or path required".to_string()))?;

    // Determine the JSON path
    let json_path: PathBuf;
    if target.ends_with(".formula.json") {
        json_path = PathBuf::from(target);
    } else if target.ends_with(".formula.toml") {
        return Err(BeadsError::Config(format!(
            "{:?} is already a TOML file",
            target
        )));
    } else {
        // Search for the JSON file by name
        let search_paths = Parser::default_search_paths();
        let found = search_paths.iter().find_map(|dir| {
            let path = dir.join(format!("{}.formula.json", target));
            if path.exists() { Some(path) } else { None }
        });
        json_path = found.ok_or_else(|| {
            BeadsError::Config(format!(
                "JSON formula {:?} not found in search paths",
                target
            ))
        })?;
    }

    let search_paths = Parser::default_search_paths();
    let mut parser = Parser::new(search_paths);

    let formula = parser
        .parse_file(&json_path)
        .map_err(|e| BeadsError::Config(format!("parsing {}: {}", json_path.display(), e)))?;

    // Convert to TOML
    let toml_string = toml::to_string_pretty(&formula)
        .map_err(|e| BeadsError::Config(format!("converting to TOML: {}", e)))?;

    if args.stdout {
        output_ctx.print(&toml_string);
        return Ok(());
    }

    let toml_path = json_path.with_extension("formula.toml");
    fs::write(&toml_path, &toml_string)
        .map_err(|e| BeadsError::Config(format!("writing {}: {}", toml_path.display(), e)))?;

    if use_json {
        let output = serde_json::json!({
            "converted": true,
            "from": json_path.to_string_lossy(),
            "to": toml_path.to_string_lossy(),
        });
        output_ctx.print(&serde_json::to_string_pretty(&output).unwrap_or_default());
    } else {
        output_ctx.print(&format!("Converted: {}", toml_path.display()));
    }

    if args.delete {
        fs::remove_file(&json_path)
            .map_err(|e| BeadsError::Config(format!("deleting {}: {}", json_path.display(), e)))?;
        if !use_json {
            output_ctx.print(&format!("Deleted: {}", json_path.display()));
        }
    }

    Ok(())
}

/// Convert all JSON formulas in search paths to TOML.
fn convert_all_formulas(output_ctx: &OutputContext, use_json: bool) -> crate::Result<()> {
    let search_paths = Parser::default_search_paths();
    let mut converted = 0u32;
    let mut errors = 0u32;

    for dir in &search_paths {
        if !dir.exists() {
            continue;
        }
        let dir_entries = match fs::read_dir(dir) {
            Ok(d) => d,
            Err(_) => continue,
        };

        for entry in dir_entries.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();

            if !fname.ends_with(".formula.json") {
                continue;
            }

            let toml_path = path.with_extension("formula.toml");
            if toml_path.exists() {
                if !use_json {
                    output_ctx.print(&format!("Skipped (TOML exists): {}", fname));
                }
                continue;
            }

            let mut parser = Parser::new(search_paths.clone());
            let formula = match parser.parse_file(&path) {
                Ok(f) => f,
                Err(e) => {
                    if !use_json {
                        output_ctx.print(&format!("Error parsing {}: {}", fname, e));
                    }
                    errors += 1;
                    continue;
                }
            };

            let toml_string = match toml::to_string_pretty(&formula) {
                Ok(s) => s,
                Err(e) => {
                    if !use_json {
                        output_ctx.print(&format!("Error converting {}: {}", fname, e));
                    }
                    errors += 1;
                    continue;
                }
            };

            if let Err(e) = fs::write(&toml_path, &toml_string) {
                if !use_json {
                    output_ctx.print(&format!("Error writing {}: {}", toml_path.display(), e));
                }
                errors += 1;
                continue;
            }

            if !use_json {
                output_ctx.print(&format!("Converted: {}", toml_path.display()));
            }
            converted += 1;
        }
    }

    if use_json {
        let output = serde_json::json!({
            "converted": converted,
            "errors": errors,
        });
        output_ctx.print(&serde_json::to_string_pretty(&output).unwrap_or_default());
    } else {
        output_ctx.print(&format!("Converted {} formulas", converted));
        if errors > 0 {
            output_ctx.print(&format!(" ({} errors)", errors));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Count steps recursively including children.
fn count_steps(steps: &[crate::formula::Step]) -> usize {
    let mut count = steps.len();
    for s in steps {
        if let Some(ref children) = s.children {
            count += count_steps(children);
        }
    }
    count
}

/// Truncate a description to max_len characters.
fn truncate_description(desc: &str, max_len: usize) -> String {
    let desc = desc.lines().next().unwrap_or(desc);
    if desc.len() > max_len {
        format!("{}...", &desc[..max_len.saturating_sub(3)])
    } else {
        desc.to_string()
    }
}

/// Print steps in a tree format.
fn print_steps_tree(steps: &[crate::formula::Step], indent: &str, output_ctx: &OutputContext) {
    for (i, step) in steps.iter().enumerate() {
        let connector = if i == steps.len() - 1 {
            "└──"
        } else {
            "├──"
        };

        // Collect dependency info
        let mut dep_parts: Vec<String> = Vec::new();
        if !step.depends_on.is_empty() {
            dep_parts.push(format!("depends: {}", step.depends_on.join(", ")));
        }
        if !step.needs.is_empty() {
            dep_parts.push(format!("needs: {}", step.needs.join(", ")));
        }
        let dep_str = if dep_parts.is_empty() {
            String::new()
        } else {
            format!(" [{}]", dep_parts.join(", "))
        };

        let type_str = if step.r#type.as_deref().map_or(true, |t| t == "task") {
            String::new()
        } else {
            format!(" ({})", step.r#type.as_deref().unwrap_or("task"))
        };

        output_ctx.print(&format!(
            "{}{} {}: {}{}{}",
            indent,
            connector,
            step.id,
            step.title.as_deref().unwrap_or("(untitled)"),
            type_str,
            dep_str
        ));

        // Print children
        if let Some(ref children) = step.children {
            let child_indent = if i == steps.len() - 1 {
                format!("{}    ", indent)
            } else {
                format!("{}│   ", indent)
            };
            print_steps_tree(children, &child_indent, output_ctx);
        }
    }
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
