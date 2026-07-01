//! Recipes command — list and install AI tool integration recipes.
//!
//! Recipes define where beads workflow instructions should be placed
//! for various AI coding tools (Cursor, Claude Code, Copilot, etc.).

use crate::cli::{RecipesCommands, RecipesInstallArgs, RecipesListArgs};
use crate::config::CliOverrides;
use crate::error::{BeadsError, Result};
use crate::output::OutputContext;
use crate::recipes;
use std::env;
use std::path::{Path, PathBuf};

/// Execute a recipes subcommand.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn execute(
    command: &RecipesCommands,
    _json: bool,
    _cli: &CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    match command {
        RecipesCommands::List(args) => execute_list(args, ctx),
        RecipesCommands::Install(args) => execute_install(args, ctx),
    }
}

/// List all available recipes.
fn execute_list(args: &RecipesListArgs, ctx: &OutputContext) -> Result<()> {
    let recipe_list = recipes::list_recipes();

    if ctx.is_json() {
        let entries: Vec<serde_json::Value> = recipe_list
            .iter()
            .map(|(name, desc, rtype)| {
                serde_json::json!({
                    "name": name,
                    "description": desc,
                    "type": rtype.as_str(),
                })
            })
            .collect();
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "count": recipe_list.len(),
            "recipes": entries,
        }))?;
        println!("{output}");
        return Ok(());
    }

    if recipe_list.is_empty() {
        ctx.print_line("No recipes available.");
        return Ok(());
    }

    ctx.print_line(&format!("Available recipes ({}):\n", recipe_list.len()));
    for (name, desc, rtype) in &recipe_list {
        if args.details {
            ctx.print_line(&format!(
                "  {:<20}  [{:<10}]  {}",
                name,
                rtype.as_str(),
                desc
            ));
        } else {
            ctx.print_line(&format!("  {:<20}  {}", name, desc));
        }
    }
    ctx.print_line(&format!(
        "\nInstall a recipe: br recipes install <name>"
    ));

    Ok(())
}

/// Install a recipe for a specific AI tool.
fn execute_install(args: &RecipesInstallArgs, ctx: &OutputContext) -> Result<()> {
    let project_dir = resolve_project_dir(args.project_dir.as_deref())?;

    let recipe = recipes::find_recipe(&args.recipe).ok_or_else(|| {
        BeadsError::Internal {
            message: format!(
                "Unknown recipe '{}'. Use `br recipes list` to see available recipes.",
                args.recipe
            ),
        }
    })?;

    let written = recipes::install_recipe(recipe, &project_dir)?;

    ctx.print_line(&format!(
        "Installed recipe '{}' ({} file(s)):",
        recipe.name,
        written.len()
    ));
    for path in &written {
        ctx.print_line(&format!("  \u{2713} {}", path.display()));
    }

    // Report which type of recipe was installed
    match recipe.recipe_type {
        crate::recipes::RecipeType::File => {
            ctx.print_line(
                "\nFile-based recipe installed. The AI tool should pick up the rules file automatically.",
            );
        }
        crate::recipes::RecipeType::MultiFile => {
            ctx.print_line(
                "\nMulti-file recipe installed. Each file serves a specific purpose in the tool's configuration.",
            );
        }
        crate::recipes::RecipeType::Hooks => {
            ctx.print_line(
                "\nHooks-based recipe installed. The AI tool needs to be configured to run `br agents` on session start.",
            );
        }
        crate::recipes::RecipeType::Section => {
            ctx.print_line(
                "\nSection-based recipe. Run `br agents` to manage AGENTS.md integration.",
            );
        }
    }

    Ok(())
}

/// Resolve the project directory, defaulting to the current working directory.
fn resolve_project_dir(dir: Option<&str>) -> Result<PathBuf> {
    match dir {
        Some(d) => Ok(Path::new(d).to_path_buf()),
        None => Ok(env::current_dir()?),
    }
}
