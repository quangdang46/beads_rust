//! `br merge-slot` subcommand — exclusive access gate for serialized conflict resolution.
//!
//! Implements the merge slot primitive: a special-purpose bead used as an exclusive
//! access token. Only one agent can hold the slot at a time.

use crate::merge_slot;
use crate::output::OutputContext;
use crate::storage::SqliteStorage;
use anyhow::{Context as _, Result};
use std::path::Path;
use std::time::Duration;

/// Dispatch a [`crate::cli::MergeSlotCommand`].
pub fn execute(
    command: &crate::cli::MergeSlotCommand,
    ctx: &OutputContext,
) -> Result<()> {
    let actor = std::env::var("BR_ACTOR")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "merge-slot".to_string());
    let db_path = std::env::var("BR_DATABASE_PATH")
        .or_else(|_| std::env::var("BEADS_DATABASE_PATH"))
        .unwrap_or_else(|_| ".beads/beads.db".to_string());
    let mut storage =
        SqliteStorage::open(Path::new(&db_path)).with_context(|| {
            format!("merge-slot: open database {db_path}")
        })?;

    let prefix = storage
        .get_config("issue_prefix")
        .unwrap_or_default()
        .unwrap_or_default();

    match command {
        crate::cli::MergeSlotCommand::Create => cmd_create(&mut storage, &actor, &prefix, ctx),
        crate::cli::MergeSlotCommand::Check => cmd_check(&mut storage, &prefix, ctx),
        crate::cli::MergeSlotCommand::Acquire(args) => {
            cmd_acquire(&mut storage, &actor, &prefix, args, ctx)
        }
        crate::cli::MergeSlotCommand::Wait(args) => {
            cmd_wait(&mut storage, &actor, &prefix, args, ctx)
        }
        crate::cli::MergeSlotCommand::Release => {
            cmd_release(&mut storage, &actor, &prefix, ctx)
        }
    }
}

fn cmd_create(
    storage: &mut SqliteStorage,
    actor: &str,
    prefix: &str,
    ctx: &OutputContext,
) -> Result<()> {
    let slot = storage
        .merge_slot_create(actor, prefix)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "slot_id": slot.id,
                "status": slot.status.to_string(),
                "created": true,
            }))?
        );
    } else {
        println!("Merge slot created: {}", slot.id);
        println!("  Status: {}", slot.status);
    }
    Ok(())
}

fn cmd_check(storage: &mut SqliteStorage, prefix: &str, ctx: &OutputContext) -> Result<()> {
    let status = storage
        .merge_slot_check(prefix)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "slot_id": status.slot_id,
                "available": status.available,
                "holder": status.holder,
                "waiters": status.waiters,
            }))?
        );
    } else {
        println!("Slot: {}", status.slot_id);
        if status.available {
            println!("  Status: AVAILABLE");
        } else {
            println!("  Status: HELD by {}", status.holder);
            if !status.waiters.is_empty() {
                println!("  Waiters: {}", status.waiters.join(", "));
            }
        }
    }
    Ok(())
}

fn cmd_acquire(
    storage: &mut SqliteStorage,
    actor: &str,
    prefix: &str,
    args: &crate::cli::MergeSlotAcquireArgs,
    ctx: &OutputContext,
) -> Result<()> {
    let result = storage
        .merge_slot_acquire(actor, actor, args.wait, prefix)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "slot_id": result.slot_id,
                "acquired": result.acquired,
                "waiting": result.waiting,
                "holder": result.holder,
                "position": result.position,
            }))?
        );
    } else if result.acquired {
        println!("Acquired merge slot: {}", result.slot_id);
    } else if result.waiting {
        println!(
            "Added to wait queue for {} (position {})",
            result.slot_id,
            result.position.unwrap_or(0)
        );
        println!("  Current holder: {}", result.holder);
    } else {
        println!("Slot {} is held by {}", result.slot_id, result.holder);
        if args.wait {
            println!("  (use --wait to join the wait queue)");
        }
    }
    Ok(())
}

fn cmd_wait(
    storage: &mut SqliteStorage,
    actor: &str,
    prefix: &str,
    args: &crate::cli::MergeSlotWaitArgs,
    ctx: &OutputContext,
) -> Result<()> {
    let poll_interval = Duration::from_secs(args.poll.unwrap_or(2).max(1));

    if !ctx.is_json() {
        println!(
            "Waiting for merge slot (polling every {}s)...",
            poll_interval.as_secs()
        );
    }

    loop {
        let result = storage
            .merge_slot_acquire(actor, actor, true, prefix)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        if result.acquired {
            if ctx.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "slot_id": result.slot_id,
                        "acquired": true,
                        "holder": result.holder,
                    }))?
                );
            } else {
                println!("Acquired merge slot: {}", result.slot_id);
            }
            return Ok(());
        }

        if !ctx.is_json() {
            println!(
                "  [waiting] slot held by {} (position {})",
                result.holder,
                result.position.unwrap_or(0)
            );
        }

        std::thread::sleep(poll_interval);
    }
}

fn cmd_release(
    storage: &mut SqliteStorage,
    actor: &str,
    prefix: &str,
    ctx: &OutputContext,
) -> Result<()> {
    storage
        .merge_slot_release(actor, actor, prefix)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if ctx.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "slot_id": merge_slot::merge_slot_id(prefix),
                "released": true,
            }))?
        );
    } else {
        println!("Released merge slot");
    }
    Ok(())
}
