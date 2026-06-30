//! CLI commands for managing custom statuses and types (Issue #5).
//!
//! These provide runtime-enumerable workflow configuration without
//! requiring code rebuilds.

use clap::{Args, Subcommand};

use crate::config;
use crate::error::{BeadsError, Result};
use crate::output::OutputContext;

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
                println!("{}", serde_json::to_string_pretty(&statuses).unwrap_or_default());
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
            let cat = args.category.to_lowercase();
            if !["active", "wip", "done", "frozen", "unspecified"].contains(&cat.as_str()) {
                return Err(BeadsError::validation(
                    "category",
                    &format!("invalid category '{cat}'; expected active, wip, done, frozen, or unspecified"),
                ));
            }
            store.add_custom_status(&args.name, &cat)?;
            println!("Added custom status '{}' (category: {})", args.name, cat);
            Ok(())
        }
        StatusCommands::Remove(args) => {
            store.remove_custom_status(&args.name)?;
            println!("Removed custom status '{}'", args.name);
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
                println!("{}", serde_json::to_string_pretty(&types).unwrap_or_default());
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
            store.add_custom_type(&args.name)?;
            println!("Added custom type '{}'", args.name);
            Ok(())
        }
        TypeCommands::Remove(args) => {
            store.remove_custom_type(&args.name)?;
            println!("Removed custom type '{}'", args.name);
            Ok(())
        }
    }
}
