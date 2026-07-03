//! `br sql` command — execute a read-only SQL query against the beads database.
//!
//! The query is wrapped in a `BEGIN`/`ROLLBACK` transaction so that any
//! accidental data modifications are never committed.

use crate::cli::{OutputFormatBasic, SqlArgs};
use crate::config;
use crate::error::Result;
use crate::output::OutputContext;
use crate::storage::SqliteStorage;
use fsqlite_types::SqliteValue;
use serde_json::Value as JsonValue;
use std::borrow::Cow;

/// Execute `br sql` using a pre-opened [`SqliteStorage`].
///
/// # Errors
///
/// Returns an error if the query fails or the output cannot be written.
pub fn execute_with_storage(
    args: &SqlArgs,
    ctx: &OutputContext,
    storage: &SqliteStorage,
) -> Result<()> {
    execute_inner(args, ctx, storage)
}

/// Execute `br sql` (standalone — opens storage from the CLI overrides).
///
/// # Errors
///
/// Returns an error if storage cannot be opened, the query fails, or the
/// output cannot be written.
pub fn execute(
    args: &SqlArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    execute_inner(args, ctx, &storage_ctx.storage)
}

/// Shared execution logic.
fn execute_inner(args: &SqlArgs, ctx: &OutputContext, storage: &SqliteStorage) -> Result<()> {
    let (column_names, rows) = storage.execute_read_only_query(&args.query)?;

    match args.format {
        OutputFormatBasic::Json => {
            let json = rows_to_json(&column_names, &rows);
            let output = serde_json::to_string_pretty(&json)
                .unwrap_or_else(|_| "[]".to_string());
            if !ctx.is_quiet() {
                println!("{output}");
            }
        }
        OutputFormatBasic::Text | OutputFormatBasic::Toon => {
            if !ctx.is_quiet() {
                print_table(&column_names, &rows);
            }
        }
    }

    Ok(())
}

/// Convert query results into a JSON array of column-keyed objects.
fn rows_to_json(column_names: &[String], rows: &[Vec<SqliteValue>]) -> Vec<JsonValue> {
    rows.iter()
        .map(|row| {
            let mut obj = serde_json::Map::with_capacity(column_names.len());
            for (i, col) in column_names.iter().enumerate() {
                let val = row.get(i).map_or(JsonValue::Null, sqlite_value_to_json);
                obj.insert(col.clone(), val);
            }
            JsonValue::Object(obj)
        })
        .collect()
}

/// Convert a single [`SqliteValue`] to a [`serde_json::Value`].
fn sqlite_value_to_json(val: &SqliteValue) -> JsonValue {
    match val {
        SqliteValue::Null => JsonValue::Null,
        SqliteValue::Integer(n) => JsonValue::Number(serde_json::Number::from(*n)),
        SqliteValue::Float(f) => {
            // serde_json does not have f64 as a Number; use json! macro
            serde_json::json!(f)
        }
        SqliteValue::Text(s) => JsonValue::String(s.to_string()),
        SqliteValue::Blob(bytes) => {
            // Blobs are hex-encoded for JSON compatibility.
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            JsonValue::String(format!("\\x{hex}"))
        }
    }
}

/// Print a simple space-aligned table of query results.
fn print_table(column_names: &[String], rows: &[Vec<SqliteValue>]) {
    // Build string representations so we can measure column widths.
    let headers: Vec<&str> = column_names.iter().map(String::as_str).collect();
    let mut cell_strings: Vec<Vec<Cow<'_, str>>> = Vec::with_capacity(rows.len());
    for row in rows {
        let cells: Vec<Cow<'_, str>> = row
            .iter()
            .map(|v| Cow::Owned(value_to_string(v)))
            .collect();
        cell_strings.push(cells);
    }

    // Compute column widths.
    let col_count = column_names.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for cells in &cell_strings {
        for (i, cell) in cells.iter().enumerate() {
            if i < col_count {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    // Header row.
    for (i, header) in headers.iter().enumerate() {
        if i > 0 {
            print!(" | ");
        }
        print!("{:<width$}", header, width = widths[i]);
    }
    println!();

    // Separator.
    for (i, width) in widths.iter().enumerate() {
        if i > 0 {
            print!("-|-");
        }
        print!("{}", "-".repeat(*width));
    }
    println!();

    // Data rows.
    for cells in &cell_strings {
        for (i, cell) in cells.iter().enumerate() {
            if i > 0 {
                print!(" | ");
            }
            if i < col_count {
                print!("{:<width$}", cell, width = widths[i]);
            }
        }
        println!();
    }

    // Footer with row count.
    if !rows.is_empty() {
        println!("({} row{})", rows.len(), if rows.len() == 1 { "" } else { "s" });
    }
}

/// Render a [`SqliteValue`] as a human-readable string.
fn value_to_string(val: &SqliteValue) -> String {
    match val {
        SqliteValue::Null => "NULL".to_string(),
        SqliteValue::Integer(n) => n.to_string(),
        SqliteValue::Float(f) => format!("{f}"),
        SqliteValue::Text(s) => s.to_string(),
        SqliteValue::Blob(bytes) => {
            // Show first few bytes as hex with a length indicator.
            let preview: String = bytes.iter().take(8).map(|b| format!("{b:02x}")).collect();
            let suffix = if bytes.len() > 8 { "..." } else { "" };
            format!("[blob {} bytes: {preview}{suffix}]", bytes.len())
        }
    }
}
