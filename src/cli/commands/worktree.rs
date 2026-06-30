//! `br worktree` subcommand — manage git worktrees with beads state awareness.
//!
//! Delegates to [`crate::worktree`] for all core logic.

use crate::output::OutputContext;
use crate::worktree::{self, BeadsState};
use anyhow::Result;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Dispatch a [`crate::cli::WorktreeCommand`].
pub fn execute(command: &crate::cli::WorktreeCommand, ctx: &OutputContext) -> Result<()> {
    match command {
        crate::cli::WorktreeCommand::List => cmd_list(ctx),
        crate::cli::WorktreeCommand::Info => cmd_info(ctx),
        crate::cli::WorktreeCommand::Create(args) => cmd_create(args, ctx),
        crate::cli::WorktreeCommand::Remove(args) => cmd_remove(args, ctx),
    }
}

fn cmd_list(ctx: &OutputContext) -> Result<()> {
    let worktrees = worktree::list_worktrees(None)?;

    if ctx.is_json() {
        println!("{}", serde_json::to_string_pretty(&worktrees)?);
        return Ok(());
    }

    if worktrees.is_empty() {
        println!("No worktrees found");
        return Ok(());
    }

    println!("{:<20} {:<40} {:<20} {}", "NAME", "PATH", "BRANCH", "BEADS STATE");
    for wt in &worktrees {
        let name = if wt.is_main {
            "(main)".to_string()
        } else {
            wt.name.clone()
        };
        let beads_info = match &wt.beads_state {
            BeadsState::Redirect => {
                if let Some(ref target) = wt.redirect_to {
                    format!("redirect → {target}")
                } else {
                    "redirect".to_string()
                }
            }
            other => other.as_str().to_string(),
        };
        println!(
            "{:<20} {:<40} {:<20} {}",
            truncate(&name, 20),
            truncate(&wt.path, 40),
            truncate(&wt.branch, 20),
            beads_info,
        );
    }

    Ok(())
}

fn cmd_info(ctx: &OutputContext) -> Result<()> {
    // Determine beads dir from cwd
    let beads_dir = find_beads_dir().map(PathBuf::from);
    let info = worktree::current_worktree_info(beads_dir.as_deref())?;

    match info {
        None => {
            if ctx.is_json() {
                println!(r#"{{"is_worktree":false}}"#);
            } else {
                println!("Not in a git worktree (this is the main repository)");
            }
        }
        Some(wt) => {
            if ctx.is_json() {
                let mut obj = serde_json::to_value(&wt)?;
                if let Some(ref beads_dir) = beads_dir {
                    obj["main_repo"] = json!(beads_dir);
                }
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                println!("Worktree: {}", wt.path);
                println!("  Name: {}", wt.name);
                println!("  Branch: {}", wt.branch);
                println!("  Beads: {}", wt.beads_state.as_str());
                if let Some(ref target) = wt.redirect_to {
                    println!("  Redirects to: {target}");
                }
            }
        }
    }

    Ok(())
}

fn cmd_create(args: &crate::cli::WorktreeCreateArgs, ctx: &OutputContext) -> Result<()> {
    let path = Path::new(&args.path);
    let branch = args.branch.as_deref();

    let wt = worktree::create_worktree(path, branch, None)?;

    if ctx.is_json() {
        println!("{}", serde_json::to_string_pretty(&wt)?);
    } else {
        println!("Created worktree: {}", wt.path);
        println!("  Branch: {}", wt.branch);
    }

    Ok(())
}

fn cmd_remove(args: &crate::cli::WorktreeRemoveArgs, ctx: &OutputContext) -> Result<()> {
    let removed = worktree::remove_worktree(&args.name, None, args.force)?;

    if ctx.is_json() {
        println!(r#"{{"removed":"{}"}}"#, removed);
    } else {
        println!("Removed worktree: {removed}");
    }

    Ok(())
}

/// Walk up from cwd looking for `.beads/` directory.
fn find_beads_dir() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(".beads");
        if candidate.is_dir() {
            return Some(candidate.to_string_lossy().to_string());
        }
        dir = dir.parent()?;
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
