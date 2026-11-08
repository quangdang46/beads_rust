//! Rename command implementation.
//!
//! Renames an issue ID, updating all dependency edges and text references
//! throughout the database.

use crate::cli::RenameArgs;
use crate::config;
use crate::error::{BeadsError, Result};
use crate::output::{OutputContext, OutputMode};
use crate::util::id::{IdResolver, ResolverConfig};
use rich_rust::prelude::*;
use serde::Serialize;
use tracing::info;

/// Result of a rename operation for JSON/Toon output.
#[derive(Debug, Serialize)]
pub struct RenameResult {
    pub old_id: String,
    pub new_id: String,
    pub title: String,
    pub updated_at: String,
}

/// Execute the rename command.
pub fn execute(args: &RenameArgs, cli: &config::CliOverrides, ctx: &OutputContext) -> Result<()> {
    let old_id = &args.old_id;
    let new_id = &args.new_id;

    if old_id == new_id {
        if ctx.is_json() {
            let result = RenameResult {
                old_id: old_id.clone(),
                new_id: new_id.clone(),
                title: String::new(),
                updated_at: String::new(),
            };
            ctx.json_pretty(&result);
        } else if ctx.is_toon() {
            let result = RenameResult {
                old_id: old_id.clone(),
                new_id: new_id.clone(),
                title: String::new(),
                updated_at: String::new(),
            };
            ctx.toon(&result);
        } else if ctx.is_quiet() {
            return Ok(());
        } else if matches!(ctx.mode(), OutputMode::Rich) {
            let console = Console::default();
            let theme = ctx.theme();
            let mut text = Text::new("");
            text.append_styled("\u{26a0} No change: ", theme.warning.clone());
            text.append_styled("old and new IDs are the same", theme.dimmed.clone());
            console.print_renderable(&text);
        } else {
            println!("No change: old and new IDs are the same");
        }
        return Ok(());
    }

    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    let config_layer = storage_ctx.load_config(cli)?;
    let actor = config::resolve_actor(&config_layer);
    let storage = &mut storage_ctx.storage;

    // Validate the new ID up front: prefix-suffix format. The prefix charset
    // mirrors `util::id::normalize_prefix` (lowercase alnum plus `_ - . : #`,
    // with at least one alphanumeric) so digit/underscore prefixes produced
    // by `init --prefix` or directory-derived names remain renamable.
    let new_id_format =
        regex::Regex::new(r"^[a-z0-9_.:#-]*[a-z0-9][a-z0-9_.:#-]*-[a-zA-Z0-9._-]+$")
            .expect("static regex is valid");
    if !new_id_format.is_match(new_id) {
        return Err(BeadsError::validation(
            "new_id",
            format!(
                "'{new_id}' must be prefix-suffix format with a lowercase prefix (e.g., acme-dolt)"
            ),
        ));
    }

    // Resolve the old ID (exact, prefix-short, or content hash). Unresolvable
    // or ambiguous input fails here with a clear error instead of a raw
    // not-found deep in storage.
    let id_config = config::id_config_from_layer(&config_layer);
    let resolver = IdResolver::new(ResolverConfig::with_prefix(id_config.prefix));
    let resolved_old_id = super::resolve_issue_id(storage, &resolver, old_id)?;

    info!(old = %resolved_old_id, new = %new_id, "Renaming issue ID");

    let renamed = storage.update_issue_id(&resolved_old_id, new_id, &actor)?;

    storage_ctx.flush_no_db_if_dirty()?;

    if ctx.is_json() {
        let result = RenameResult {
            old_id: resolved_old_id.clone(),
            new_id: new_id.clone(),
            title: renamed.title,
            updated_at: renamed.updated_at.to_rfc3339(),
        };
        ctx.json_pretty(&result);
    } else if ctx.is_toon() {
        let result = RenameResult {
            old_id: resolved_old_id.clone(),
            new_id: new_id.clone(),
            title: renamed.title,
            updated_at: renamed.updated_at.to_rfc3339(),
        };
        ctx.toon(&result);
    } else if ctx.is_quiet() {
        return Ok(());
    } else if matches!(ctx.mode(), OutputMode::Rich) {
        let console = Console::default();
        let theme = ctx.theme();
        let mut text = Text::new("");
        text.append_styled("\u{2713} Renamed: ", theme.success.clone());
        text.append_styled(&resolved_old_id, theme.issue_id.clone());
        text.append(" → ");
        text.append_styled(new_id, theme.accent.clone());
        console.print_renderable(&text);
    } else {
        println!("Renamed: {} → {}", resolved_old_id, new_id);
    }

    Ok(())
}
