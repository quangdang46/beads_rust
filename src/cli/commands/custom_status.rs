//! CLI commands for managing custom statuses and types (Issue #5).
//!
//! These provide runtime-enumerable workflow configuration without
//! requiring code rebuilds.

use clap::{Args, Subcommand};

use crate::config;
use crate::error::{BeadsError, Result};
use crate::model::{IssueType, Status};
use crate::output::OutputContext;
use crate::storage::ListFilters;

/// Canonicalise and validate a custom status/type registry name: trims
/// whitespace, rejects empty input and embedded whitespace, and lowercases
/// (status/type parsing is case-insensitive and stores lowercase).
fn validate_registry_name(name: &str, kind: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(BeadsError::validation(kind, "name cannot be empty"));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(BeadsError::validation(
            kind,
            format!("invalid name '{trimmed}': cannot contain whitespace"),
        ));
    }
    Ok(trimmed.to_lowercase())
}

// ---------------------------------------------------------------------------
// CLI arg types
// ---------------------------------------------------------------------------

/// Manage custom statuses.
#[derive(Debug, Subcommand)]
pub enum StatusCommands {
    /// List all custom statuses.
    List(StatusListArgs),
    /// Add a custom status.
    Add(StatusAddArgs),
    /// Remove a custom status.
    Remove(StatusRemoveArgs),
}

/// List custom statuses.
#[derive(Debug, Args)]
pub struct StatusListArgs {
    /// JSON output.
    #[arg(long)]
    pub json: bool,
}

/// Add a custom status.
#[derive(Debug, Args)]
pub struct StatusAddArgs {
    /// Status name (e.g. "reviewing").
    pub name: String,
    /// Behavior category: active, wip, done, frozen, unspecified.
    #[arg(long, default_value = "unspecified")]
    pub category: String,
}

/// Remove a custom status.
#[derive(Debug, Args)]
pub struct StatusRemoveArgs {
    /// Status name to remove.
    pub name: String,
    /// Remove even when issues still use this status.
    #[arg(long)]
    pub force: bool,
}

/// Manage custom types.
#[derive(Debug, Subcommand)]
pub enum TypeCommands {
    /// List all custom types.
    List(TypeListArgs),
    /// Add a custom type.
    Add(TypeAddArgs),
    /// Remove a custom type.
    Remove(TypeRemoveArgs),
}

/// List custom types.
#[derive(Debug, Args)]
pub struct TypeListArgs {
    /// JSON output.
    #[arg(long)]
    pub json: bool,
}

/// Add a custom type.
#[derive(Debug, Args)]
pub struct TypeAddArgs {
    /// Type name (e.g. "enhancement").
    pub name: String,
}

/// Remove a custom type.
#[derive(Debug, Args)]
pub struct TypeRemoveArgs {
    /// Type name to remove.
    pub name: String,
    /// Remove even when issues still use this type.
    #[arg(long)]
    pub force: bool,
}

// ---------------------------------------------------------------------------
// Execute functions
// ---------------------------------------------------------------------------

/// Execute a custom status command.
pub fn execute_status(
    command: &StatusCommands,
    json: bool,
    overrides: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(overrides)?;
    let storage_ctx = config::open_storage_with_cli(&beads_dir, overrides)?;
    let store = storage_ctx.storage;

    match command {
        StatusCommands::List(args) => {
            let use_json = json || args.json;
            if use_json {
                let statuses = store.list_custom_statuses()?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&statuses).unwrap_or_default()
                );
            } else {
                let statuses = store.list_custom_statuses()?;
                if statuses.is_empty() {
                    ctx.warning("No custom statuses found.");
                    return Ok(());
                }
                println!("Custom Statuses:");
                println!("{:<24} {}", "Name", "Category");
                println!("{}", "-".repeat(40));
                for cs in &statuses {
                    println!("{:<24} {}", cs.name, cs.category);
                }
            }
            Ok(())
        }
        StatusCommands::Add(args) => {
            let cat = args.category.trim().to_lowercase();
            if !["active", "wip", "done", "frozen", "unspecified"].contains(&cat.as_str()) {
                return Err(BeadsError::validation(
                    "category",
                    &format!(
                        "invalid category '{cat}'; expected active, wip, done, frozen, or unspecified"
                    ),
                ));
            }
            let name = validate_registry_name(&args.name, "name")?;
            // Reject names that parse to a built-in status (including aliases
            // like `inprogress`) — they can never be `Status::Custom`.
            if !matches!(name.parse::<Status>(), Ok(Status::Custom(_))) {
                return Err(BeadsError::validation(
                    "name",
                    format!("'{name}' is a built-in status name"),
                ));
            }
            // Case-insensitive duplicate check up front: storage uses
            // INSERT OR IGNORE, so a duplicate would otherwise be reported
            // as a successful add while silently keeping the old category.
            let existing = store.list_custom_statuses()?;
            if let Some(found) = existing
                .iter()
                .find(|cs| cs.name.trim().eq_ignore_ascii_case(&name))
            {
                return Err(BeadsError::validation(
                    "name",
                    format!(
                        "custom status '{}' already exists (category: {})",
                        found.name, found.category
                    ),
                ));
            }
            if !store.add_custom_status(&name, &cat)? {
                return Err(BeadsError::internal(&format!(
                    "failed to add custom status '{name}'"
                )));
            }
            println!("Added custom status '{name}' (category: {cat})");
            Ok(())
        }
        StatusCommands::Remove(args) => {
            let name = validate_registry_name(&args.name, "name")?;
            // Built-in statuses are backfilled into the registry table by
            // schema migration; they are not removable custom statuses.
            if !matches!(name.parse::<Status>(), Ok(Status::Custom(_))) {
                return Err(BeadsError::validation(
                    "name",
                    format!("'{name}' is a built-in status name"),
                ));
            }
            // `remove_custom_status` returns false when nothing matched.
            let existing = store.list_custom_statuses()?;
            if !existing
                .iter()
                .any(|cs| cs.name.trim().eq_ignore_ascii_case(&name))
            {
                return Err(BeadsError::validation(
                    "name",
                    format!("custom status '{name}' does not exist"),
                ));
            }
            // Block removal while issues still use the status; `--force`
            // overrides (the issues keep their status string but it is no
            // longer a registered custom status).
            let in_use = store.list_issues(&ListFilters {
                statuses: Some(vec![Status::Custom(name.clone())]),
                include_closed: true,
                include_deferred: true,
                limit: Some(1),
                ..Default::default()
            })?;
            if !in_use.is_empty() && !args.force {
                return Err(BeadsError::validation(
                    "name",
                    format!(
                        "custom status '{name}' is still used by at least one issue; \
                         reassign those issues first or pass --force to remove anyway"
                    ),
                ));
            }
            if !in_use.is_empty() {
                ctx.warning(&format!(
                    "custom status '{name}' is still used by at least one issue"
                ));
            }
            if !store.remove_custom_status(&name)? {
                return Err(BeadsError::internal(&format!(
                    "failed to remove custom status '{name}'"
                )));
            }
            println!("Removed custom status '{name}'");
            Ok(())
        }
    }
}

/// Execute a custom type command.
pub fn execute_type(
    command: &TypeCommands,
    json: bool,
    overrides: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(overrides)?;
    let storage_ctx = config::open_storage_with_cli(&beads_dir, overrides)?;
    let store = storage_ctx.storage;

    match command {
        TypeCommands::List(args) => {
            let use_json = json || args.json;
            if use_json {
                let types = store.list_custom_types()?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&types).unwrap_or_default()
                );
            } else {
                let types = store.list_custom_types()?;
                if types.is_empty() {
                    ctx.warning("No custom types found.");
                    return Ok(());
                }
                println!("Custom Types:");
                for ct in &types {
                    println!("  {}", ct.name);
                }
            }
            Ok(())
        }
        TypeCommands::Add(args) => {
            let name = validate_registry_name(&args.name, "name")?;
            if !matches!(name.parse::<IssueType>(), Ok(IssueType::Custom(_))) {
                return Err(BeadsError::validation(
                    "name",
                    format!("'{name}' is a built-in type name"),
                ));
            }
            let existing = store.list_custom_types()?;
            if existing
                .iter()
                .any(|ct| ct.name.trim().eq_ignore_ascii_case(&name))
            {
                return Err(BeadsError::validation(
                    "name",
                    format!("custom type '{name}' already exists"),
                ));
            }
            if !store.add_custom_type(&name)? {
                return Err(BeadsError::internal(&format!(
                    "failed to add custom type '{name}'"
                )));
            }
            println!("Added custom type '{name}'");
            Ok(())
        }
        TypeCommands::Remove(args) => {
            let name = validate_registry_name(&args.name, "name")?;
            if !matches!(name.parse::<IssueType>(), Ok(IssueType::Custom(_))) {
                return Err(BeadsError::validation(
                    "name",
                    format!("'{name}' is a built-in type name"),
                ));
            }
            let existing = store.list_custom_types()?;
            if !existing
                .iter()
                .any(|ct| ct.name.trim().eq_ignore_ascii_case(&name))
            {
                return Err(BeadsError::validation(
                    "name",
                    format!("custom type '{name}' does not exist"),
                ));
            }
            // Parity with `custom-status remove`: block removal while
            // issues still reference the type; `--force` overrides (the
            // issues keep their type string but it is no longer a
            // registered custom type).
            let in_use = store.list_issues(&ListFilters {
                types: Some(vec![IssueType::Custom(name.clone())]),
                include_closed: true,
                include_deferred: true,
                limit: Some(1),
                ..Default::default()
            })?;
            if !in_use.is_empty() && !args.force {
                return Err(BeadsError::validation(
                    "name",
                    format!(
                        "custom type '{name}' is still used by at least one issue; \
                         reassign those issues first or pass --force to remove anyway"
                    ),
                ));
            }
            if !in_use.is_empty() {
                ctx.warning(&format!(
                    "custom type '{name}' is still used by at least one issue"
                ));
            }
            if !store.remove_custom_type(&name)? {
                return Err(BeadsError::internal(&format!(
                    "failed to remove custom type '{name}'"
                )));
            }
            println!("Removed custom type '{name}'");
            Ok(())
        }
    }
}
