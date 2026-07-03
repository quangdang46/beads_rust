//! Rename prefix command implementation.
//!
//! Renames the issue ID prefix across all issues in the database,
//! updating IDs, dependency edges, text references, and the stored
//! `issue_prefix` configuration.

use crate::cli::RenamePrefixArgs;
use crate::config;
use crate::error::{BeadsError, Result};
use crate::output::{OutputContext, OutputMode};
use crate::util::id::{normalize_prefix, split_prefix_remainder};
use rich_rust::prelude::*;
use serde::Serialize;
use tracing::info;

/// Result entry for a single ID rename during prefix rename.
#[derive(Debug, Serialize, Clone)]
pub struct PrefixRenameEntry {
    pub old_id: String,
    pub new_id: String,
}

/// Result of the rename-prefix operation for JSON/Toon output.
#[derive(Debug, Serialize)]
pub struct RenamePrefixResult {
    pub old_prefix: String,
    pub new_prefix: String,
    pub renamed_count: usize,
    pub entries: Vec<PrefixRenameEntry>,
}

/// Execute the rename-prefix command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn execute(
    args: &RenamePrefixArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let new_prefix = normalize_prefix(&args.new_prefix);
    if new_prefix.is_empty() {
        return Err(BeadsError::validation(
            "new_prefix",
            "Prefix must not be empty after normalization",
        ));
    }

    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    let config_layer = storage_ctx.load_config(cli)?;
    let old_prefix = config::id_config_from_layer(&config_layer).prefix;
    let actor = config::resolve_actor(&config_layer);
    let storage = &mut storage_ctx.storage;

    if old_prefix == new_prefix {
        print_same_prefix(ctx, &old_prefix, &new_prefix);
        return Ok(());
    }

    // Collect all issues and compute their new IDs.
    let all_issues = storage.get_all_issues_for_export()?;
    let mut renames: Vec<(String, String)> = Vec::new();

    for issue in &all_issues {
        if let Some((prefix, remainder)) = split_prefix_remainder(&issue.id) {
            if prefix == old_prefix {
                let new_id = format!("{new_prefix}-{remainder}");
                renames.push((issue.id.clone(), new_id));
            }
        }
    }

    if renames.is_empty() {
        print_no_matches(ctx, &old_prefix);
        return Ok(());
    }

    if args.dry_run {
        print_dry_run(ctx, &old_prefix, &new_prefix, &renames);
        return Ok(());
    }

    // Perform the renames sequentially.
    info!(
        old = %old_prefix,
        new = %new_prefix,
        count = renames.len(),
        "Renaming issue ID prefix"
    );

    let mut entries: Vec<PrefixRenameEntry> = Vec::with_capacity(renames.len());
    for (old_id, new_id) in &renames {
        storage.update_issue_id(old_id, new_id, &actor)?;
        entries.push(PrefixRenameEntry {
            old_id: old_id.clone(),
            new_id: new_id.clone(),
        });
    }

    // Update the stored prefix in the DB config.
    storage.set_config("issue_prefix", &new_prefix)?;

    storage_ctx.flush_no_db_if_dirty()?;

    print_result(ctx, &old_prefix, &new_prefix, &entries);

    Ok(())
}

fn print_same_prefix(ctx: &OutputContext, old_prefix: &str, new_prefix: &str) {
    if ctx.is_json() {
        let result = RenamePrefixResult {
            old_prefix: old_prefix.to_string(),
            new_prefix: new_prefix.to_string(),
            renamed_count: 0,
            entries: vec![],
        };
        ctx.json_pretty(&result);
    } else if ctx.is_toon() {
        let result = RenamePrefixResult {
            old_prefix: old_prefix.to_string(),
            new_prefix: new_prefix.to_string(),
            renamed_count: 0,
            entries: vec![],
        };
        ctx.toon(&result);
    } else if !ctx.is_quiet() {
        if matches!(ctx.mode(), OutputMode::Rich) {
            let console = Console::default();
            let theme = ctx.theme();
            let mut text = Text::new("");
            text.append_styled("\u{26a0} No change: ", theme.warning.clone());
            text.append_styled(
                "old and new prefix are the same",
                theme.dimmed.clone(),
            );
            console.print_renderable(&text);
        } else {
            println!("No change: old and new prefix are the same");
        }
    }
}

fn print_no_matches(ctx: &OutputContext, old_prefix: &str) {
    if ctx.is_json() {
        let result = RenamePrefixResult {
            old_prefix: old_prefix.to_string(),
            new_prefix: old_prefix.to_string(),
            renamed_count: 0,
            entries: vec![],
        };
        ctx.json_pretty(&result);
    } else if ctx.is_toon() {
        let result = RenamePrefixResult {
            old_prefix: old_prefix.to_string(),
            new_prefix: old_prefix.to_string(),
            renamed_count: 0,
            entries: vec![],
        };
        ctx.toon(&result);
    } else if !ctx.is_quiet() {
        if matches!(ctx.mode(), OutputMode::Rich) {
            let console = Console::default();
            let theme = ctx.theme();
            let mut text = Text::new("");
            text.append_styled("No issues with prefix \"", theme.dimmed.clone());
            text.append_styled(old_prefix, theme.issue_id.clone());
            text.append_styled("\" found", theme.dimmed.clone());
            console.print_renderable(&text);
        } else {
            println!("No issues with prefix \"{}\" found", old_prefix);
        }
    }
}

fn print_dry_run(ctx: &OutputContext, old_prefix: &str, new_prefix: &str, renames: &[(String, String)]) {
    let entries: Vec<PrefixRenameEntry> = renames
        .iter()
        .map(|(old, new)| PrefixRenameEntry {
            old_id: old.clone(),
            new_id: new.clone(),
        })
        .collect();

    if ctx.is_json() {
        let result = RenamePrefixResult {
            old_prefix: old_prefix.to_string(),
            new_prefix: new_prefix.to_string(),
            renamed_count: entries.len(),
            entries,
        };
        ctx.json_pretty(&result);
    } else if ctx.is_toon() {
        let result = RenamePrefixResult {
            old_prefix: old_prefix.to_string(),
            new_prefix: new_prefix.to_string(),
            renamed_count: entries.len(),
            entries,
        };
        ctx.toon(&result);
    } else if !ctx.is_quiet() {
        if matches!(ctx.mode(), OutputMode::Rich) {
            let console = Console::default();
            let theme = ctx.theme();
            let mut text = Text::new("");
            text.append_styled("Would rename ", theme.dimmed.clone());
            let count_str = renames.len().to_string();
            text.append_styled(&count_str, theme.accent.clone());
            text.append_styled(" issue(s) from prefix \"", theme.dimmed.clone());
            text.append_styled(old_prefix, theme.issue_id.clone());
            text.append_styled("\" to \"", theme.dimmed.clone());
            text.append_styled(new_prefix, theme.issue_id.clone());
            text.append_styled("\"", theme.dimmed.clone());
            console.print_renderable(&text);
            for (old_id, new_id) in renames {
                let mut line = Text::new("");
                line.append("  ");
                line.append_styled(old_id, theme.issue_id.clone());
                line.append(" ");
                line.append_styled("->", theme.dimmed.clone());
                line.append(" ");
                line.append_styled(new_id, theme.accent.clone());
                console.print_renderable(&line);
            }
        } else {
            println!(
                "Would rename {} issue(s) from prefix \"{}\" to \"{}\"",
                renames.len(),
                old_prefix,
                new_prefix
            );
            for (old_id, new_id) in renames {
                println!("  {} -> {}", old_id, new_id);
            }
        }
    }
}

fn print_result(ctx: &OutputContext, old_prefix: &str, new_prefix: &str, entries: &[PrefixRenameEntry]) {
    if ctx.is_json() {
        let result = RenamePrefixResult {
            old_prefix: old_prefix.to_string(),
            new_prefix: new_prefix.to_string(),
            renamed_count: entries.len(),
            entries: entries.to_vec(),
        };
        ctx.json_pretty(&result);
    } else if ctx.is_toon() {
        let result = RenamePrefixResult {
            old_prefix: old_prefix.to_string(),
            new_prefix: new_prefix.to_string(),
            renamed_count: entries.len(),
            entries: entries.to_vec(),
        };
        ctx.toon(&result);
    } else if ctx.is_quiet() {
        return;
    } else if matches!(ctx.mode(), OutputMode::Rich) {
        let console = Console::default();
        let theme = ctx.theme();
        let mut text = Text::new("");
        text.append_styled("\u{2713} Renamed prefix: ", theme.success.clone());
        text.append_styled(old_prefix, theme.issue_id.clone());
        text.append(" -> ");
        text.append_styled(new_prefix, theme.accent.clone());
        text.append(" (");
        let count_str = entries.len().to_string();
        text.append_styled(&count_str, theme.accent.clone());
        text.append(" issue(s))");
        console.print_renderable(&text);
        for entry in entries {
            let mut line = Text::new("");
            line.append("  ");
            line.append_styled(&entry.old_id, theme.issue_id.clone());
            line.append(" ");
            line.append_styled("->", theme.dimmed.clone());
            line.append(" ");
            line.append_styled(&entry.new_id, theme.accent.clone());
            console.print_renderable(&line);
        }
    } else {
        println!(
            "Renamed prefix: {} -> {} ({} issue(s))",
            old_prefix,
            new_prefix,
            entries.len()
        );
        for entry in entries {
            println!("  {} -> {}", entry.old_id, entry.new_id);
        }
    }
}
