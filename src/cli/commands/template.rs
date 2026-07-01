//! Template CRUD commands.
//!
//! Templates are issues with `is_template=true`. They reuse the Issue table,
//! so creating, listing, showing, and deleting templates maps to the same
//! storage operations with the `is_template` flag set.

use super::{retry_mutation_with_jsonl_recovery, report_auto_flush_failure};
use crate::cli::commands::create::create_issue_impl;
use crate::cli::{
    config, CreateArgs, TemplateCommands, TemplateCreateArgs, TemplateDeleteArgs, TemplateListArgs,
    TemplateShowArgs,
};
use crate::config::{CliOverrides, OpenStorageResult};
use crate::error::{BeadsError, Result};
use crate::format::sanitize_terminal_inline;
use crate::model::Issue;
use crate::output::OutputContext;
use crate::storage::{ListFilters, SqliteStorage};
use crate::util::id::{IdResolver, ResolverConfig};
use chrono::Utc;

// ---------------------------------------------------------------------------
// Top-level dispatch
// ---------------------------------------------------------------------------

/// Execute a template subcommand.
///
/// # Errors
///
/// Returns an error if storage operations or rendering fails.
pub fn execute(
    command: &TemplateCommands,
    _json: bool,
    cli: &CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    match command {
        TemplateCommands::Create(args) => execute_create(args, cli, ctx),
        TemplateCommands::List(args) => execute_list(args, cli, ctx),
        TemplateCommands::Show(args) => execute_show(args, cli, ctx),
        TemplateCommands::Delete(args) => execute_delete(args, cli, ctx),
    }
}

/// Execute a template command using a pre-opened storage context.
///
/// # Errors
///
/// Returns `Err` if storage operations fail.
pub fn execute_with_storage_ctx(
    command: &TemplateCommands,
    _json: bool,
    cli: &CliOverrides,
    ctx: &OutputContext,
    storage_ctx: &OpenStorageResult,
) -> Result<bool> {
    match command {
        TemplateCommands::List(args) => {
            execute_list_with_storage(args, cli, ctx, storage_ctx)?;
            Ok(true)
        }
        TemplateCommands::Show(args) => {
            execute_show_with_storage(args, cli, ctx, storage_ctx)?;
            Ok(true)
        }
        TemplateCommands::Create(_) | TemplateCommands::Delete(_) => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// Create
// ---------------------------------------------------------------------------

fn execute_create(args: &TemplateCreateArgs, cli: &CliOverrides, ctx: &OutputContext) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    let layer = storage_ctx.load_config(cli)?;

    let create_config = super::create::CreateConfig {
        id_config: config::id_config_from_layer(&layer),
        default_priority: config::default_priority_from_layer(&layer)?,
        default_issue_type: config::default_issue_type_from_layer(&layer)?,
        actor: config::resolve_actor(&layer),
        source_repo: super::create::canonical_source_repo(&storage_ctx.paths.beads_dir),
        source_repo_path: super::create::canonical_source_repo_path(&storage_ctx.paths.beads_dir),
    };

    // Build a CreateArgs from the template create args.
    let create_args = CreateArgs {
        title: Some(args.title.clone()),
        description: args.description.clone(),
        type_: args.type_.clone(),
        priority: args.priority.clone(),
        labels: args.labels.clone(),
        assignee: args.assignee.clone(),
        owner: args.owner.clone(),
        status: None,
        estimate: None,
        due: None,
        defer: None,
        external_ref: None,
        parent: None,
        deps: vec![],
        slug: None,
        file: None,
        title_flag: None,
        silent: false,
        dry_run: false,
        ephemeral: false,
        agent_name: None,
        harness: None,
        model: None,
    };

    let issue = retry_mutation_with_jsonl_recovery(
        &mut storage_ctx,
        true,
        "template create",
        None,
        |storage| {
            let mut issue = create_issue_impl(storage, &create_args, &create_config)?;
            // Set is_template via storage
            storage.set_issue_template_flag(&issue.id, true)?;
            issue.is_template = true;
            Ok(issue)
        },
    )?;

    let created_id = issue.id.clone();

    // Output
    if ctx.is_json() {
        ctx.json_pretty(&serde_json::json!({
            "id": created_id,
            "title": issue.title,
            "is_template": true,
        }));
    } else if ctx.is_toon() {
        ctx.toon(&issue);
    } else {
        ctx.success(&format!(
            "Created template {}: {}",
            sanitize_terminal_inline(&created_id),
            sanitize_terminal_inline(&issue.title),
        ));
    }

    storage_ctx.auto_flush_if_enabled().unwrap_or_else(|error| {
        report_auto_flush_failure(
            ctx,
            &storage_ctx.paths.beads_dir,
            &storage_ctx.paths.jsonl_path,
            &error,
        );
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

fn execute_list(args: &TemplateListArgs, cli: &CliOverrides, ctx: &OutputContext) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    execute_list_with_storage(args, cli, ctx, &storage_ctx)
}

fn execute_list_with_storage(
    args: &TemplateListArgs,
    cli: &CliOverrides,
    ctx: &OutputContext,
    storage_ctx: &OpenStorageResult,
) -> Result<()> {
    let storage = &storage_ctx.storage;

    let mut filters = ListFilters {
        include_templates: true,
        limit: args.limit,
        offset: args.offset,
        sort: Some("title".to_string()),
        reverse: false,
        ..Default::default()
    };

    let issues = storage.list_issues(&filters)?;

    // Filter to only templates (is_template=true)
    let templates: Vec<&Issue> = issues.iter().filter(|i| i.is_template).collect();

    if args.json || ctx.is_json() {
        let json_list: Vec<serde_json::Value> = templates
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "title": t.title,
                    "issue_type": t.issue_type.as_str(),
                    "description": t.description,
                    "status": t.status.as_str(),
                    "labels": t.labels,
                })
            })
            .collect();
        ctx.json_pretty(&json_list);
        return Ok(());
    }

    if templates.is_empty() {
        ctx.info("No templates found.");
        return Ok(());
    }

    // Text output
    for t in &templates {
        let type_str = t.issue_type.as_str();
        let labels_str = if t.labels.is_empty() {
            String::new()
        } else {
            format!(" [{}]", t.labels.join(", "))
        };
        println!(
            "{}  {}  ({}){}",
            sanitize_terminal_inline(&t.id),
            sanitize_terminal_inline(&t.title),
            type_str,
            labels_str,
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Show
// ---------------------------------------------------------------------------

fn execute_show(args: &TemplateShowArgs, cli: &CliOverrides, ctx: &OutputContext) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    execute_show_with_storage(args, cli, ctx, &storage_ctx)
}

fn execute_show_with_storage(
    args: &TemplateShowArgs,
    cli: &CliOverrides,
    ctx: &OutputContext,
    storage_ctx: &OpenStorageResult,
) -> Result<()> {
    let storage = &storage_ctx.storage;
    let layer = storage_ctx.load_config(cli)?;
    let id_config = config::id_config_from_layer(&layer);
    let resolver = IdResolver::new(ResolverConfig::with_prefix(id_config.prefix));

    let resolution = resolver.resolve_fallible(
        &args.id,
        |id| storage.id_exists(id),
        |hash| storage.find_ids_by_hash(hash),
    )?;
    let resolved_id = resolution.id;

    let issue = storage
        .get_issue(&resolved_id)?
        .ok_or_else(|| BeadsError::IssueNotFound {
            id: resolved_id.clone(),
        })?;

    if !issue.is_template {
        return Err(BeadsError::validation(
            "id",
            format!("issue {resolved_id} is not a template"),
        ));
    }

    if ctx.is_json() {
        ctx.json_pretty(&issue);
    } else if ctx.is_toon() {
        ctx.toon(&issue);
    } else {
        // Simple text output similar to `br show` style but template-focused
        println!("ID:          {}", sanitize_terminal_inline(&issue.id));
        println!("Title:       {}", sanitize_terminal_inline(&issue.title));
        println!("Type:        {}", issue.issue_type.as_str());
        println!("Priority:    {}", issue.priority);
        println!("Status:      {}", issue.status.as_str());
        if let Some(ref desc) = issue.description {
            if !desc.is_empty() {
                println!("Description: {}", sanitize_terminal_inline(desc));
            }
        }
        if !issue.labels.is_empty() {
            println!("Labels:      {}", issue.labels.join(", "));
        }
        if let Some(ref assignee) = issue.assignee {
            println!("Assignee:    {}", sanitize_terminal_inline(assignee));
        }
        if let Some(ref owner) = issue.owner {
            println!("Owner:       {}", sanitize_terminal_inline(owner));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Delete (tombstone)
// ---------------------------------------------------------------------------

fn execute_delete(
    args: &TemplateDeleteArgs,
    cli: &CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    let layer = storage_ctx.load_config(cli)?;
    let id_config = config::id_config_from_layer(&layer);
    let resolver = IdResolver::new(ResolverConfig::with_prefix(id_config.prefix));
    let actor = config::resolve_actor(&layer);

    let resolution = resolver.resolve_fallible(
        &args.id,
        |id| storage_ctx.storage.id_exists(id),
        |hash| storage_ctx.storage.find_ids_by_hash(hash),
    )?;
    let resolved_id = resolution.id;

    // Verify it's a template
    let issue = storage_ctx
        .storage
        .get_issue(&resolved_id)?
        .ok_or_else(|| BeadsError::IssueNotFound {
            id: resolved_id.clone(),
        })?;

    if !issue.is_template {
        return Err(BeadsError::validation(
            "id",
            format!("issue {resolved_id} is not a template"),
        ));
    }

    if matches!(issue.status, crate::model::Status::Tombstone) {
        return Err(BeadsError::validation(
            "id",
            format!("template {resolved_id} is already deleted"),
        ));
    }

    // Tombstone the issue: set status to Tombstone and deleted_at
    let now = Utc::now();
    let update = crate::storage::IssueUpdate {
        status: Some(crate::model::Status::Tombstone),
        deleted_at: Some(Some(now)),
        deleted_by: Some(Some(actor.clone())),
        delete_reason: Some(Some(args.reason.clone().unwrap_or_default())),
        ..Default::default()
    };

    retry_mutation_with_jsonl_recovery(
        &mut storage_ctx,
        true,
        "template delete",
        Some(&resolved_id),
        |storage| storage.update_issue(&resolved_id, &update, &actor),
    )?;

    if ctx.is_json() {
        ctx.json_pretty(&serde_json::json!({
            "deleted": resolved_id,
            "is_template": true,
        }));
    } else if ctx.is_toon() {
        ctx.toon(&serde_json::json!({"deleted": resolved_id}));
    } else {
        ctx.success(&format!(
            "Deleted template {}",
            sanitize_terminal_inline(&resolved_id),
        ));
    }

    storage_ctx.auto_flush_if_enabled().unwrap_or_else(|error| {
        report_auto_flush_failure(
            ctx,
            &storage_ctx.paths.beads_dir,
            &storage_ctx.paths.jsonl_path,
            &error,
        );
    });

    Ok(())
}
