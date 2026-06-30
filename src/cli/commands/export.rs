//! Export command implementation.
//!
//! Exports issues from the database in JSONL, JSON, or CSV format.
//! Supports optional filename output (default stdout) and the same
//! filter flags as `br list`.

use crate::cli::{ExportFormat, ListArgs};
use crate::cli::commands::list::{build_filters, validate_list_args};
use crate::config;
use crate::error::{BeadsError, Result};
use crate::format::csv;
use crate::model::Issue;
use crate::output::OutputContext;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::str::FromStr;

/// Arguments for the export command.
#[derive(clap::Args, Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ExportArgs {
    /// Output format: jsonl (default), csv, json
    #[arg(long, short = 'f', default_value = "jsonl")]
    pub format: String,

    /// Output file path (default: stdout)
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Reuse all filter flags from `br list` (status, type, label, etc.)
    #[command(flatten)]
    pub filters: ListArgs,
}

/// Execute the export command.
///
/// # Errors
///
/// Returns an error if the database cannot be opened, the query fails,
/// or writing output fails.
#[allow(clippy::too_many_lines)]
pub fn execute(
    args: &ExportArgs,
    cli: &config::CliOverrides,
    outer_ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    execute_inner(args, cli, outer_ctx, &storage_ctx)
}

/// Execute export using storage that was already opened by the caller.
///
/// # Errors
///
/// Returns an error if the query or output fails.
#[allow(clippy::too_many_lines)]
pub fn execute_with_storage(
    args: &ExportArgs,
    cli: &config::CliOverrides,
    outer_ctx: &OutputContext,
    storage_ctx: &config::OpenStorageResult,
) -> Result<()> {
    execute_inner(args, cli, outer_ctx, storage_ctx)
}

#[allow(clippy::too_many_lines)]
fn execute_inner(
    args: &ExportArgs,
    _cli: &config::CliOverrides,
    _outer_ctx: &OutputContext,
    storage_ctx: &config::OpenStorageResult,
) -> Result<()> {
    let storage = &storage_ctx.storage;

    // Parse the export format
    let export_format = ExportFormat::from_str(&args.format).map_err(|_| {
        BeadsError::Config(format!(
            "Unknown export format: '{}'. Must be one of: jsonl, json, csv, obsidian",
            args.format
        ))
    })?;

    // Validate list args before building filters
    validate_list_args(&args.filters)?;

    // Build list filters from the flattened ListArgs
    // Use limit=0 (unlimited) for export so we get all matching issues
    let mut filters = build_filters(&args.filters)?;
    filters.limit = Some(0);
    filters.offset = Some(0);

    // Query issues
    let issues = storage.list_issues(&filters)?;

    // Resolve CSV fields if needed
    let csv_fields = if matches!(export_format, ExportFormat::Csv) {
        csv::parse_fields(args.filters.fields.as_deref())
    } else {
        vec![]
    };

    match export_format {
        ExportFormat::Jsonl => {
            // JSONL output: one JSON object per line
            if let Some(ref output_path) = args.output {
                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let file = File::create(output_path)?;
                let mut writer = BufWriter::new(file);
                for issue in &issues {
                    let line = serde_json::to_string(issue)
                        .map_err(|e| BeadsError::Config(format!("Serialization error: {e}")))?;
                    writeln!(writer, "{line}")?;
                }
            } else {
                // Write to stdout
                let stdout = io::stdout();
                let mut out = stdout.lock();
                for issue in &issues {
                    let line = serde_json::to_string(issue)
                        .map_err(|e| BeadsError::Config(format!("Serialization error: {e}")))?;
                    writeln!(out, "{line}")?;
                }
            }
        }
        ExportFormat::Json => {
            // JSON output: pretty-printed JSON array
            let json = serde_json::to_string_pretty(&issues)
                .map_err(|e| BeadsError::Config(format!("Serialization error: {e}")))?;
            if let Some(ref output_path) = args.output {
                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(output_path, json)?;
            } else {
                println!("{json}");
            }
        }
        ExportFormat::Csv => {
            // CSV output
            if let Some(ref output_path) = args.output {
                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let file = File::create(output_path)?;
                let mut writer = BufWriter::new(file);
                csv::write_csv(&mut writer, &issues, &csv_fields)?;
            } else {
                let stdout = io::stdout();
                let mut out = stdout.lock();
                csv::write_csv(&mut out, &issues, &csv_fields)?;
            }
        }
        ExportFormat::Obsidian => {
            write_obsidian_export(&issues, &args.output)?;
        }
    }

    Ok(())
}

/// Write issues in Obsidian Markdown format (Tasks plugin compatible).
///
/// Each issue becomes a task with checkbox, priority emoji, labels as tags,
/// and a link to the issue ID.
fn write_obsidian_export(issues: &[Issue], output: &Option<PathBuf>) -> Result<()> {
    // obsidianCheckbox maps status to Tasks checkbox syntax
    fn checkbox_for(status: &str) -> &'static str {
        match status {
            "open" => "- [ ]",
            "in_progress" => "- [/]",
            "blocked" => "- [c]",
            "closed" => "- [x]",
            "deferred" => "- [-]",
            "pinned" => "- [n]",
            _ => "- [ ]",
        }
    }

    // obsidianPriority maps priority (0-4) to emoji
    fn priority_emoji(priority: i32) -> &'static str {
        match priority {
            0 => "\u{1F6A6}", // red triangle (critical)
            1 => "\u{23EB}",  // up arrow (high)
            2 => "\u{1F53C}", // up-tiny arrow (medium)
            3 => "\u{1F53D}", // down-tiny arrow (low)
            _ => "\u{23F4}",  // down arrow (backlog)
        }
    }

    // Type tag mapping
    fn type_tag(issue_type: &str) -> Option<&'static str> {
        match issue_type {
            "bug" => Some("#Bug"),
            "feature" => Some("#Feature"),
            "task" => Some("#Task"),
            "epic" => Some("#Epic"),
            "chore" => Some("#Chore"),
            "question" => Some("#Question"),
            "docs" => Some("#Docs"),
            _ => None,
        }
    }

    let mut lines: Vec<String> = Vec::new();

    // Group issues by status for section headers
    let mut open: Vec<&Issue> = Vec::new();
    let mut in_progress: Vec<&Issue> = Vec::new();
    let mut closed: Vec<&Issue> = Vec::new();
    let mut other: Vec<&Issue> = Vec::new();

    for issue in issues {
        match issue.status.as_str() {
            "open" => open.push(issue),
            "in_progress" => in_progress.push(issue),
            "closed" => closed.push(issue),
            _ => other.push(issue),
        }
    }

    lines.push("# Beads Tasks\n".to_string());
    lines.push(format!("Generated {} issues\n", issues.len()));

    for section_name in ["In Progress", "Open", "Other", "Closed"] {
        let section_issues: Vec<&Issue> = match section_name {
            "In Progress" => std::mem::take(&mut in_progress),
            "Open" => std::mem::take(&mut open),
            "Other" => std::mem::take(&mut other),
            "Closed" => std::mem::take(&mut closed),
            _ => continue,
        };

        if section_issues.is_empty() {
            continue;
        }

        lines.push(format!("\n## {section_name}\n"));
        for issue in section_issues {
            let checkbox = checkbox_for(&issue.status);
            let priority = priority_emoji(issue.priority);
            let type_tag_str = type_tag(&issue.issue_type).unwrap_or("");
            let label_tags: String = issue
                .labels
                .iter()
                .map(|l| format!("#{}", l.replace(' ', "-")))
                .collect::<Vec<_>>()
                .join(" ");

            let mut parts: Vec<String> = vec![
                checkbox.to_string(),
                issue.title.clone(),
                format!("\u{1F194} {}", issue.id),
                priority.to_string(),
            ];
            if !type_tag_str.is_empty() {
                parts.push(type_tag_str.to_string());
            }
            if !label_tags.is_empty() {
                parts.push(label_tags);
            }

            lines.push(parts.join(" ") + "\n");
        }
    }

    let content = lines.join("");

    if let Some(ref output_path) = output {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output_path, &content)?;
    } else {
        print!("{content}");
    }

    Ok(())
}
