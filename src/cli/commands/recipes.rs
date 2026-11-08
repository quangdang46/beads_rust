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
    ctx.print_line(&format!("\nInstall a recipe: br recipes install <name>"));

    Ok(())
}

/// Install a recipe for a specific AI tool.
fn execute_install(args: &RecipesInstallArgs, ctx: &OutputContext) -> Result<()> {
    let project_dir = resolve_project_dir(args.project_dir.as_deref())?;

    let recipe = recipes::find_recipe(&args.recipe).ok_or_else(|| {
        BeadsError::validation(
            "recipe",
            format!(
                "unknown recipe '{}'. Use `br recipes list` to see available recipes.",
                args.recipe
            ),
        )
    })?;

    // Hooks-type recipes (Claude Code, Gemini CLI) have no single `path`;
    // `install_recipe` would try to write to the project directory itself.
    // Write a SessionStart settings file for each configured settings path.
    let written = if recipe.recipe_type == crate::recipes::RecipeType::Hooks {
        install_hooks_recipe(recipe, &project_dir)?
    } else {
        recipes::install_recipe(recipe, &project_dir)?
    };

    if ctx.is_json() {
        let files: Vec<serde_json::Value> = written
            .iter()
            .map(|p| serde_json::Value::String(p.display().to_string()))
            .collect();
        let output = serde_json::to_string_pretty(&serde_json::json!({
            "recipe": recipe.name,
            "type": recipe.recipe_type.as_str(),
            "files": files,
        }))?;
        println!("{output}");
        return Ok(());
    }

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

/// Settings JSON that registers `br prime` as a SessionStart hook.
///
/// Claude Code (`settings.json` / `settings.local.json`) and Gemini CLI
/// (`settings.json`) both accept this hooks shape.
const SESSION_START_HOOKS_JSON: &str = r#"{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "br prime",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
"#;

/// Install a hooks-type recipe (Claude Code, Gemini CLI).
///
/// These recipes declare `paths` (e.g. `.claude/settings.local.json`,
/// `~/.claude/settings.json`) rather than a single `path`. For each entry
/// we expand a leading `~/` against `$HOME`, create parent directories,
/// and write the SessionStart hooks settings — merging into an existing
/// JSON object when possible so we don't clobber user settings.
fn install_hooks_recipe(
    recipe: &crate::recipes::Recipe,
    project_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for raw in recipe.paths {
        let target = if let Some(stripped) = raw.strip_prefix("~/") {
            let home = env::var("HOME").map_err(|_| {
                BeadsError::validation("HOME", "cannot expand '~/' recipe path: HOME is not set")
            })?;
            Path::new(&home).join(stripped)
        } else {
            project_dir.join(raw)
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        merge_or_write_settings(&target)?;
        written.push(target);
    }
    Ok(written)
}

/// Write hooks settings to `target`, preserving an existing JSON object.
fn merge_or_write_settings(target: &Path) -> Result<()> {
    let mut doc: serde_json::Map<String, serde_json::Value> = match std::fs::read_to_string(target)
    {
        Ok(existing) => match serde_json::from_str::<serde_json::Value>(&existing) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        },
        Err(_) => serde_json::Map::new(),
    };
    if let Ok(serde_json::Value::Object(hooks)) =
        serde_json::from_str::<serde_json::Value>(SESSION_START_HOOKS_JSON)
            .map(|v| v.get("hooks").cloned().unwrap_or(serde_json::Value::Null))
    {
        doc.insert("hooks".to_string(), serde_json::Value::Object(hooks));
    }
    let out = serde_json::to_string_pretty(&serde_json::Value::Object(doc))?;
    std::fs::write(target, format!("{out}\n"))?;
    Ok(())
}
