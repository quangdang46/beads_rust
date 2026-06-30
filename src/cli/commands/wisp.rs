//! Wisps — ephemeral issues for inter-agent coordination.
//!
//! Wisps are issues with `Ephemeral=true` and a `wsp-` ID prefix.
//! They are stored in the main issues table but excluded from JSONL
//! export/sync (`br sync` skips ephemeral rows).
//!
//! Subcommands:
//! - `list`:   List active wisps
//! - `create`: Create a wisp
//! - `close`:  Close a wisp
//! - `gc`:     Garbage-collect old wisps

use crate::config;
use crate::config::CliOverrides;
use crate::error::BeadsError;
use crate::model::{Issue, IssueType, MolType, Priority, Status, WispType, WorkType};
use crate::output::OutputContext;
use crate::storage::SqliteStorage;
use crate::Result;
use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use std::str::FromStr;
use tracing::info;

// ---------------------------------------------------------------------------
// CLI arg types
// ---------------------------------------------------------------------------

/// Wisp subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum WispCommands {
    /// List wisps (ephemeral issues)
    List(WispListArgs),
    /// Create a new wisp
    Create(WispCreateArgs),
    /// Close one or more wisps
    Close(WispCloseArgs),
    /// Garbage-collect old wisps
    Gc(WispGcArgs),
}

/// `br wisp list` arguments
#[derive(Args, Debug, Default, Clone)]
pub struct WispListArgs {
    /// Include closed wisps
    #[arg(long)]
    pub all: bool,
    /// Show output in JSON format
    #[arg(long)]
    pub json: bool,
}

/// `br wisp create` arguments
#[derive(Args, Debug, Default, Clone)]
pub struct WispCreateArgs {
    /// Title of the wisp
    pub title: Vec<String>,
    /// Priority level (0-4 or P0-P4)
    #[arg(long, short)]
    pub priority: Option<String>,
    /// Issue type (task, bug, feature, etc.)
    #[arg(long, short = 't')]
    pub type_: Option<String>,
    /// Assignee
    #[arg(long, short)]
    pub assignee: Option<String>,
}

/// `br wisp close` arguments
#[derive(Args, Debug, Default, Clone)]
pub struct WispCloseArgs {
    /// Wisp ID(s) to close
    pub ids: Vec<String>,
    /// Close reason
    #[arg(long, default_value = "Completed")]
    pub reason: String,
}

/// `br wisp gc` arguments
#[derive(Args, Debug, Default, Clone)]
pub struct WispGcArgs {
    /// Delete wisps older than this many hours (default: 24)
    #[arg(long, default_value = "24")]
    pub max_age_hours: u64,
    /// Dry run — show what would be deleted
    #[arg(long)]
    pub dry_run: bool,
}

// ---------------------------------------------------------------------------
// Execute dispatch
// ---------------------------------------------------------------------------

/// Execute a wisp subcommand.
pub fn execute(
    command: &WispCommands,
    overrides: &CliOverrides,
    _output_ctx: &OutputContext,
) -> Result<()> {
    // Resolve beads dir and open storage
    let beads_dir = config::discover_beads_dir_with_cli(overrides)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, overrides)?;
    let store = &mut storage_ctx.storage;

    let actor = overrides.actor.as_deref().unwrap_or("wisp");

    match command {
        WispCommands::List(args) => execute_list(args, store, actor),
        WispCommands::Create(args) => execute_create(args, store, actor),
        WispCommands::Close(args) => execute_close(args, store, actor),
        WispCommands::Gc(args) => execute_gc(args, store, actor),
    }
}

// ---------------------------------------------------------------------------
// List implementation
// ---------------------------------------------------------------------------

fn execute_list(
    args: &WispListArgs,
    store: &mut SqliteStorage,
    _actor: &str,
) -> Result<()> {
    let all_issues = store.list_issues(&crate::storage::ListFilters {
        include_closed: args.all,
        include_templates: false,
        limit: Some(2000),
        ..Default::default()
    })?;

    let wisps: Vec<&Issue> = all_issues.iter().filter(|i| i.ephemeral).collect();

    if wisps.is_empty() {
        println!("No wisps found.");
        return Ok(());
    }

    if args.json {
        let output: Vec<serde_json::Value> = wisps
            .iter()
            .map(|i| {
                serde_json::json!({
                    "id": i.id,
                    "title": i.title,
                    "status": i.status.as_str(),
                    "priority": i.priority.0,
                    "issue_type": i.issue_type.as_str(),
                    "assignee": i.assignee,
                    "created_at": i.created_at.to_rfc3339(),
                    "updated_at": i.updated_at.to_rfc3339(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for wisp in &wisps {
            let age_h = (Utc::now() - wisp.created_at).num_hours();
            println!(
                "{:20} {:31} {:7} p{} {:12} created {}h ago",
                wisp.id,
                truncate(&wisp.title, 30),
                wisp.status.as_str(),
                wisp.priority.0,
                wisp.issue_type.as_str(),
                age_h,
            );
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

// ---------------------------------------------------------------------------
// Create implementation
// ---------------------------------------------------------------------------

fn execute_create(
    args: &WispCreateArgs,
    store: &mut SqliteStorage,
    actor: &str,
) -> Result<()> {
    let title = if args.title.is_empty() {
        return Err(BeadsError::validation("title", "cannot be empty"));
    } else {
        args.title.join(" ")
    };

    let priority = if let Some(ref p) = args.priority {
        Priority::from_str(p)?
    } else {
        Priority::from_str("P3")?
    };

    let issue_type = if let Some(ref t) = args.type_ {
        IssueType::from_str(t)?
    } else {
        IssueType::Task
    };

    let now = Utc::now();

    // Generate a wsp-prefixed ID
    let issue_count = store.count_issues()?;
    let id_gen = crate::util::id::IdGenerator::new(
        crate::util::id::IdConfig::with_prefix("wsp"),
    );
    let id = id_gen.generate(
        &title,
        None,
        Some(actor),
        now,
        issue_count,
        |candidate: &str| store.id_exists(candidate).unwrap_or(false),
    );

    let issue = Issue {
        id: id.clone(),
        title,
        description: None,
        status: Status::Open,
        priority,
        issue_type,
        created_at: now,
        updated_at: now,
        assignee: args.assignee.clone(),
        owner: None,
        estimated_minutes: None,
        due_at: None,
        defer_until: None,
        external_ref: None,
        ephemeral: true,
        content_hash: None,
        design: None,
        acceptance_criteria: None,
        notes: None,
        created_by: Some(actor.to_string()),
        closed_at: None,
        close_reason: None,
        closed_by_session: None,
        source_system: None,
        source_repo: None,
        source_repo_path: None,
        agent_context: None,
        deleted_at: None,
        deleted_by: None,
        delete_reason: None,
        original_type: None,
        compaction_level: None,
        compacted_at: None,
        compacted_at_commit: None,
        original_size: None,
        sender: None,
        pinned: false,
        is_template: false,
        no_history: false,
        mol_type: MolType::default(),
        work_type: WorkType::default(),
        wisp_type: WispType::default(),
        spec_id: None,
        started_at: None,
        metadata: None,
        source_formula: None,
        source_location: None,
        await_type: None,
        await_id: None,
        timeout_seconds: None,
        holder: None,
        hook_bead: None,
        role_bead: None,
        agent_state: None,
        role_type: None,
        rig: None,
        event_kind: None,
        target: None,
        payload: None,
        quality_score: None,
        crystallizes: false,
        bonded_from: Vec::new(),
        labels: vec![],
        dependencies: vec![],
        comments: vec![],
    };

    store.create_issue(&issue, actor)?;
    println!("Created wisp: {}", id);
    info!(id = %id, "wisp created");
    Ok(())
}

// ---------------------------------------------------------------------------
// Close implementation
// ---------------------------------------------------------------------------

fn execute_close(
    args: &WispCloseArgs,
    store: &mut SqliteStorage,
    actor: &str,
) -> Result<()> {
    let now = Utc::now();
    for id in &args.ids {
        let updates = crate::storage::IssueUpdate {
            status: Some(Status::Closed),
            close_reason: Some(Some(args.reason.clone())),
            closed_at: Some(Some(now)),
            ..Default::default()
        };
        store.update_issue(id, &updates, actor)?;
        println!("Closed wisp: {}", id);
        info!(id = %id, "wisp closed");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// GC implementation
// ---------------------------------------------------------------------------

fn execute_gc(
    args: &WispGcArgs,
    store: &mut SqliteStorage,
    actor: &str,
) -> Result<()> {
    let max_age = chrono::Duration::hours(args.max_age_hours as i64);
    let cutoff = Utc::now() - max_age;

    let all_issues = store.list_issues(&crate::storage::ListFilters {
        include_closed: true,
        include_templates: false,
        limit: Some(5000),
        ..Default::default()
    })?;

    // Filter to ephemeral + older than cutoff
    let stale: Vec<&Issue> = all_issues
        .iter()
        .filter(|i| i.ephemeral && i.updated_at < cutoff)
        .collect();

    if stale.is_empty() {
        println!("No stale wisps to garbage-collect.");
        return Ok(());
    }

    if args.dry_run {
        println!("Would delete {} stale wisps:", stale.len());
        for wisp in &stale {
            let age_h = (Utc::now() - wisp.created_at).num_hours();
            println!("  {}  {}  ({}h old)", wisp.id, wisp.title, age_h);
        }
        return Ok(());
    }

    let mut deleted = 0u64;
    for wisp in &stale {
        store.delete_issue(&wisp.id, actor, "wisp gc: stale", None)?;
        deleted += 1;
        info!(id = %wisp.id, "wisp gc deleted");
    }
    println!("Garbage-collected {} stale wisps.", deleted);
    Ok(())
}
