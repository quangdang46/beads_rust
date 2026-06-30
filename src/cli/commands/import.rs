//! Import command implementation.
//!
//! Imports issues from JSON, CSV, or markdown files into the database.
//! Uses existing sync infrastructure for JSONL imports via `br sync --import-only`.

use crate::cli::ImportArgs;
use crate::cli::ExportFormat;
use crate::config;
use crate::error::{BeadsError, Result};
use crate::model::*;
use crate::output::OutputContext;
use crate::util::markdown_import;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Execute the import command.
#[allow(clippy::too_many_lines)]
pub fn execute(
    args: &ImportArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let mut storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    execute_inner(args, cli, ctx, &mut storage_ctx)
}

/// Execute import with already-opened storage.
#[allow(clippy::too_many_lines)]
pub fn execute_with_storage(
    args: &ImportArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
    storage_ctx: &mut config::OpenStorageResult,
) -> Result<()> {
    execute_inner(args, cli, ctx, storage_ctx)
}

#[allow(clippy::too_many_lines)]
fn execute_inner(
    args: &ImportArgs,
    _cli: &config::CliOverrides,
    ctx: &OutputContext,
    storage_ctx: &mut config::OpenStorageResult,
) -> Result<()> {
    let storage = &mut storage_ctx.storage;

    // Parse import format
    let import_format = ExportFormat::from_str(&args.format).map_err(|_| {
        BeadsError::Config(format!(
            "Unknown import format: '{}'. Must be one of: jsonl, json, csv, obsidian",
            args.format
        ))
    })?;

    // Resolve input path
    let input_path = if let Some(ref path) = args.input {
        path.clone()
    } else {
        let mut default_path = storage_ctx.paths.beads_dir.clone();
        default_path.push("issues.jsonl");
        default_path
    };

    if !input_path.exists() {
        return Err(BeadsError::Config(format!(
            "Input file not found: {}",
            input_path.display()
        )));
    }

    match import_format {
        ExportFormat::Jsonl => {
            // JSONL import: delegate to sync's import_from_jsonl
            let import_config = crate::sync::ImportConfig {
                skip_prefix_validation: false,
                rename_on_import: true,
                clear_duplicate_external_refs: true,
                orphan_mode: crate::sync::OrphanMode::Strict,
                force_upsert: args.force,
                beads_dir: Some(storage_ctx.paths.beads_dir.clone()),
                allow_external_jsonl: true,
                show_progress: false,
            };

            let result =
                crate::sync::import_from_jsonl(storage, &input_path, &import_config, args.rename_prefix.as_deref())?;

            if ctx.is_json() {
                ctx.json_pretty(&serde_json::json!({
                    "imported": result.imported_count,
                    "skipped": result.skipped_count,
                    "conflict_markers": result.conflict_markers.len(),
                }));
            } else if ctx.is_toon() {
                ctx.toon(&serde_json::json!({
                    "imported": result.imported_count,
                    "skipped": result.skipped_count,
                }));
            } else if ctx.is_quiet() {
                return Ok(());
            } else {
                println!(
                    "Imported {} issues from {} ({} skipped)",
                    result.imported_count,
                    input_path.display(),
                    result.skipped_count,
                );
                if !result.conflict_markers.is_empty() {
                    eprintln!("  {} conflict markers found", result.conflict_markers.len());
                }
            }
        }
        ExportFormat::Json => {
            let content = std::fs::read_to_string(&input_path)?;
            let issues: Vec<Issue> = serde_json::from_str(&content).map_err(|e| {
                BeadsError::Config(format!("Invalid JSON in '{}': {e}", input_path.display()))
            })?;
            let imported = import_issues(storage, &issues)?;
            report_count(imported, &input_path, ctx);
        }
        ExportFormat::Csv => {
            let issues = read_issues_from_csv(&input_path)?;
            let imported = import_issues(storage, &issues)?;
            report_count(imported, &input_path, ctx);
        }
        ExportFormat::Obsidian => {
            let issues = read_issues_from_markdown(&input_path)?;
            let imported = import_issues(storage, &issues)?;
            report_count(imported, &input_path, ctx);
        }
    }

    // Flush to JSONL
    storage_ctx.flush_no_db_if_dirty()?;

    Ok(())
}

/// Print import count results.
fn report_count(imported: usize, input_path: &Path, ctx: &OutputContext) {
    if ctx.is_json() {
        ctx.json_pretty(&serde_json::json!({ "imported": imported }));
    } else if ctx.is_toon() {
        ctx.toon(&serde_json::json!({ "imported": imported }));
    } else if ctx.is_quiet() {
        return;
    } else {
        println!("Imported {imported} issues from {}", input_path.display());
    }
}

/// Import issues into storage, trying create then update on conflict.
fn import_issues(
    storage: &mut crate::storage::SqliteStorage,
    issues: &[Issue],
) -> Result<usize> {
    let mut count = 0;
    for issue in issues {
        match storage.create_issue(issue, "br-import") {
            Ok(()) => count += 1,
            Err(e) => {
                eprintln!("Warning: could not import issue {}: {e}", issue.id);
            }
        }
    }
    Ok(count)
}

/// Read issues from a CSV file.
fn read_issues_from_csv(path: &Path) -> Result<Vec<Issue>> {
    use csv::ReaderBuilder;
    use std::collections::HashMap;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|e| BeadsError::Config(format!("Failed to open CSV '{}': {e}", path.display())))?;

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| BeadsError::Config(format!("Invalid CSV headers: {e}")))?
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();

    let mut issues = Vec::new();

    for (row_idx, result) in reader.records().enumerate() {
        let record = result.map_err(|e| {
            BeadsError::Config(format!("CSV parse error at row {}: {e}", row_idx + 2))
        })?;

        let mut fields: HashMap<String, String> = HashMap::new();
        for (i, value) in record.iter().enumerate() {
            if let Some(header) = headers.get(i) {
                fields.insert(header.clone(), value.to_string());
            }
        }

        let issue = issue_from_csv_row(&fields, row_idx)?;
        issues.push(issue);
    }

    Ok(issues)
}

/// Build an Issue from a CSV row map.
fn issue_from_csv_row(
    fields: &std::collections::HashMap<String, String>,
    row_idx: usize,
) -> Result<Issue> {
    let now = Utc::now();

    let mut issue = Issue::default();

    issue.id = fields
        .get("id")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("csv-import-{row_idx}"));

    issue.title = fields
        .get("title")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            BeadsError::Config(format!("Row {}: missing required 'title' field", row_idx + 2))
        })?;

    issue.description = fields
        .get("description")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    issue.status = fields
        .get("status")
        .and_then(|s| Status::from_str(s.trim()).ok())
        .unwrap_or(Status::Open);

    issue.priority = fields
        .get("priority")
        .and_then(|s| parse_priority_value(s.trim()))
        .unwrap_or(Priority(3));

    issue.issue_type = fields
        .get("issue_type")
        .and_then(|s| IssueType::from_str(s.trim()).ok())
        .unwrap_or(IssueType::Task);

    issue.created_at = fields
        .get("created_at")
        .and_then(|s| parse_datetime(s.trim()))
        .unwrap_or(now);

    issue.updated_at = now;

    issue.closed_at = fields
        .get("closed_at")
        .and_then(|s| parse_datetime(s.trim()));

    issue.due_at = fields
        .get("due_at")
        .and_then(|s| parse_datetime(s.trim()));

    issue.defer_until = fields
        .get("defer_until")
        .and_then(|s| parse_datetime(s.trim()));

    issue.assignee = fields
        .get("assignee")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    issue.owner = fields
        .get("owner")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    issue.external_ref = fields
        .get("external_ref")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    issue.notes = fields
        .get("notes")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    issue.created_by = Some("br-import".to_string());

    Ok(issue)
}

/// Read issues from a markdown file using the existing parser.
fn read_issues_from_markdown(path: &Path) -> Result<Vec<Issue>> {
    let parsed = markdown_import::parse_markdown_file(path)?;
    let mut issues = Vec::new();
    for (idx, p) in parsed.iter().enumerate() {
        let now = Utc::now();
        let mut issue = Issue::default();

        issue.id = p
            .stand_in_id
            .clone()
            .unwrap_or_else(|| format!("md-import-{idx}"));

        issue.title = p.title.clone();

        issue.description = p.description.clone();
        issue.design = p.design.clone();
        issue.acceptance_criteria = p.acceptance_criteria.clone();

        issue.status = Status::Open;

        issue.priority = p
            .priority
            .as_deref()
            .and_then(parse_priority_value)
            .unwrap_or(Priority(3));

        issue.issue_type = p
            .issue_type
            .as_deref()
            .and_then(|s| IssueType::from_str(s).ok())
            .unwrap_or(IssueType::Task);

        issue.assignee = p.assignee.clone();

        issue.created_at = now;
        issue.updated_at = now;
        issue.created_by = Some("br-import".to_string());
        issue.agent_context = p.agent_context.clone();

        if !p.labels.is_empty() {
            issue.labels = p.labels.clone();
        }

        issues.push(issue);
    }
    Ok(issues)
}

/// Parse a priority value (numeric string 0-4).
fn parse_priority_value(s: &str) -> Option<Priority> {
    let cleaned = s.to_ascii_uppercase();
    let num_str = cleaned.strip_prefix('P').unwrap_or(&cleaned);
    let val: i32 = num_str.parse().ok()?;
    if (0..=4).contains(&val) {
        Some(Priority(val))
    } else {
        None
    }
}

/// Parse a datetime string (various formats).
fn parse_datetime(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    for fmt in &[
        "%Y-%m-%dT%H:%M:%S%.fZ",
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
    ] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.and_utc());
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            if let Some(dt) = d.and_hms_opt(0, 0, 0) {
                return Some(dt.and_utc());
            }
        }
    }
    None
}
