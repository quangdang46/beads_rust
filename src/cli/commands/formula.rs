//! Formula Language CLI commands.
//!
//! Commands:
//! - `br formula validate <file>` — Validate a formula file
//! - `br formula expand <file>` — Preview the steps that would be created

use crate::error::BeadsError;
use crate::formula::Parser;
use crate::output::OutputContext;
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
}

/// Arguments for `br formula validate <file>`
#[derive(clap::Args, Debug)]
pub struct FormulaValidateArgs {
    /// Path to the formula file (.formula.json or .formula.toml)
    pub file: PathBuf,
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

// ---------------------------------------------------------------------------
// Execute functions
// ---------------------------------------------------------------------------

/// Execute a formula subcommand.
pub fn execute(
    command: &FormulaCommands,
    _overrides: &crate::config::CliOverrides,
    output_ctx: &OutputContext,
) -> crate::Result<()> {
    #[allow(clippy::wildcard_enum_match_arm)]
    match command {
        FormulaCommands::Validate(args) => execute_validate(args, output_ctx),
        FormulaCommands::Expand(args) => execute_expand(args, output_ctx),
    }
}

/// Validate a formula file.
fn execute_validate(args: &FormulaValidateArgs, output_ctx: &OutputContext) -> crate::Result<()> {
    let mut parser = Parser::new(vec![]);
    let formula = parser
        .parse_file(&args.file)
        .map_err(|e| BeadsError::Config(format!("Failed to parse formula: {}", e)))?;

    formula
        .validate()
        .map_err(|e| BeadsError::Config(format!("Formula validation failed: {}", e)))?;

    let resolved = if formula.extends.is_empty() {
        formula.clone()
    } else {
        parser
            .resolve(&formula)
            .map_err(|e| BeadsError::Config(format!("Formula resolution failed: {}", e)))?
    };

    let summary = format!(
        "Formula \"{}\" is valid\n  Type: {:?}\n  Vars: {}\n  Steps: {}\n  Source: {}",
        formula.formula,
        formula.r#type,
        resolved.vars.as_ref().map_or(0, |v| v.len()),
        resolved.steps.as_ref().map_or(0, |s| s.len()),
        formula.source.as_deref().unwrap_or("(memory)"),
    );

    output_ctx.print(&summary);
    Ok(())
}

/// Preview the steps that would be created from a formula.
fn execute_expand(args: &FormulaExpandArgs, output_ctx: &OutputContext) -> crate::Result<()> {
    // Parse variable overrides
    let mut vars = HashMap::new();
    for kv in &args.vars {
        if let Some((key, value)) = kv.split_once('=') {
            vars.insert(key.to_string(), value.to_string());
        } else {
            return Err(BeadsError::Config(format!(
                "Invalid variable override {:?}: expected KEY=VALUE format",
                kv
            )));
        }
    }

    let mut parser = Parser::new(vec![]);
    let formula = parser
        .parse_file(&args.file)
        .map_err(|e| BeadsError::Config(format!("Failed to parse formula: {}", e)))?;

    formula
        .validate()
        .map_err(|e| BeadsError::Config(format!("Formula validation failed: {}", e)))?;

    let resolved = if formula.extends.is_empty() {
        formula.clone()
    } else {
        parser
            .resolve(&formula)
            .map_err(|e| BeadsError::Config(format!("Formula resolution failed: {}", e)))?
    };

    let steps = resolved.steps.as_deref().unwrap_or_default();

    // Determine output mode
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
        output_ctx.print(
            &serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".to_string()),
        );
    } else {
        let mut lines = Vec::new();
        lines.push(format!("Formula: {}", resolved.formula));
        if let Some(desc) = &resolved.description {
            lines.push(format!("  Description: {}", desc));
        }
        lines.push(format!("  Type: {:?}", resolved.r#type));
        lines.push(format!("  Steps: {}", steps.len()));
        lines.push(String::new());
        for s in steps {
            let title = s.title.as_deref().unwrap_or(&s.id);
            lines.push(format!("  \u{2514}\u{2500} {}: {}", s.id, title));
            if let Some(t) = &s.r#type {
                lines.push(format!("  \u{2502}    Type: {}", t));
            }
            if let Some(p) = s.priority {
                lines.push(format!("  \u{2502}    Priority: {}", p));
            }
            if !s.labels.is_empty() {
                lines.push(format!("  \u{2502}    Labels: {}", s.labels.join(", ")));
            }
            if !s.depends_on.is_empty() {
                lines.push(format!("  \u{2502}    Depends on: {}", s.depends_on.join(", ")));
            }
            if !s.needs.is_empty() {
                lines.push(format!("  \u{2502}    Needs: {}", s.needs.join(", ")));
            }
            if let Some(children) = &s.children {
                for child in children {
                    lines.push(format!(
                        "  \u{2502}    \u{2514}\u{2500} [child] {}: {}",
                        child.id,
                        child.title.as_deref().unwrap_or(&child.id)
                    ));
                }
            }
        }
        output_ctx.print(&lines.join("\n"));
    }

    Ok(())
}
