//! `br sql` command — execute a read-only SQL query against the beads database.
//!
//! The query is wrapped in a `BEGIN`/`ROLLBACK` transaction so that any
//! accidental data modifications are never committed.

use crate::cli::{OutputFormatBasic, SqlArgs};
use crate::config;
use crate::error::{BeadsError, Result};
use crate::output::OutputContext;
use crate::storage::SqliteStorage;
use crate::storage::db::SqlValue;
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
pub fn execute(args: &SqlArgs, cli: &config::CliOverrides, ctx: &OutputContext) -> Result<()> {
    if cli.no_db == Some(true) {
        return Err(BeadsError::validation(
            "--no-db",
            "br sql requires a database; --no-db is not supported for this command",
        ));
    }
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;

    // `br sql` is read-only: it must never create a database file. When an
    // explicit `--db` override (or resolved DB path) does not exist, fail
    // instead of silently materializing an empty database.
    let db_path = config::resolve_paths(&beads_dir, cli.db.as_ref())?.db_path;
    if !db_path.exists() {
        return Err(BeadsError::validation(
            "--db",
            format!(
                "database does not exist: {} (br sql never creates databases)",
                db_path.display()
            ),
        ));
    }

    let storage_ctx = config::open_storage_with_cli(&beads_dir, cli)?;
    execute_inner(args, ctx, &storage_ctx.storage)
}

/// Shared execution logic.
fn execute_inner(args: &SqlArgs, ctx: &OutputContext, storage: &SqliteStorage) -> Result<()> {
    ensure_read_only_query(&args.query)?;
    let (column_names, rows) = storage.execute_read_only_query(&args.query)?;

    match args.format {
        OutputFormatBasic::Json => {
            let json = rows_to_json(&column_names, &rows);
            let output = serde_json::to_string_pretty(&json).unwrap_or_else(|_| "[]".to_string());
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
fn rows_to_json(column_names: &[String], rows: &[Vec<SqlValue>]) -> Vec<JsonValue> {
    rows.iter()
        .map(|row| {
            let mut obj = serde_json::Map::with_capacity(column_names.len());
            for (i, col) in column_names.iter().enumerate() {
                let val = match row.get(i) {
                    Some(v) => sqlite_value_to_json(v),
                    None => JsonValue::Null,
                };
                obj.insert(col.clone(), val);
            }
            JsonValue::Object(obj)
        })
        .collect()
}

/// Convert a single stored value to a [`serde_json::Value`].
fn sqlite_value_to_json(val: &SqlValue) -> JsonValue {
    match val.value() {
        rusqlite::types::Value::Null => JsonValue::Null,
        rusqlite::types::Value::Integer(n) => JsonValue::Number(serde_json::Number::from(*n)),
        rusqlite::types::Value::Real(f) => {
            // serde_json does not have f64 as a Number; use json! macro
            serde_json::json!(f)
        }
        rusqlite::types::Value::Text(s) => JsonValue::String(s.to_string()),
        rusqlite::types::Value::Blob(bytes) => {
            // Blobs are hex-encoded for JSON compatibility.
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            JsonValue::String(format!("\\x{hex}"))
        }
    }
}

/// Print a simple space-aligned table of query results.
fn print_table(column_names: &[String], rows: &[Vec<SqlValue>]) {
    // Build string representations so we can measure column widths.
    let headers: Vec<&str> = column_names.iter().map(String::as_str).collect();
    let mut cell_strings: Vec<Vec<Cow<'_, str>>> = Vec::with_capacity(rows.len());
    for row in rows {
        let cells: Vec<Cow<'_, str>> = row.iter().map(|v| Cow::Owned(value_to_string(v))).collect();
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
        println!(
            "({} row{})",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        );
    }
}

/// Read-only pragmas that only inspect schema/state and cannot mutate the
/// database. Everything else under `PRAGMA` is rejected: several pragmas
/// (`writable_schema`, `journal_mode`, `wal_checkpoint`, `synchronous`, …)
/// either write directly or escape the `BEGIN`/`ROLLBACK` wrapper because
/// they are not transactional.
const READ_ONLY_PRAGMAS: &[&str] = &[
    "table_info",
    "table_xinfo",
    "index_list",
    "index_info",
    "index_xinfo",
    "foreign_key_list",
    "foreign_key_check",
    "integrity_check",
    "quick_check",
    "database_list",
    "table_list",
    "compile_options",
    "function_list",
    "module_list",
    "pragma_list",
    "collation_list",
    "encoding",
    "schema_version",
];

/// Keywords that must never appear in a `br sql` statement. The storage
/// layer wraps the query in `BEGIN`/`ROLLBACK`, but that does not stop
/// non-transactional statements (`PRAGMA writable_schema`, `VACUUM`,
/// `ATTACH`, transaction control) from taking effect, so they are rejected
/// here before the query reaches SQLite.
const FORBIDDEN_KEYWORDS: &[&str] = &[
    "insert",
    "update",
    "delete",
    "replace",
    "create",
    "drop",
    "alter",
    "attach",
    "detach",
    "vacuum",
    "reindex",
    "begin",
    "commit",
    "rollback",
    "savepoint",
    "release",
    "load_extension",
];

/// Validate that `query` is a single read-only SQL statement.
///
/// Returns a [`BeadsError::validation`] error for empty input, multiple
/// statements, mutating keywords, transaction control, or non-read-only
/// pragmas.
fn ensure_read_only_query(query: &str) -> Result<()> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(BeadsError::validation("query", "SQL query cannot be empty"));
    }

    for statement in split_sql_statements(trimmed) {
        let code = blank_sql_noise(&statement);
        let words = sql_words(&code);
        let Some(first) = words.first().map(String::as_str) else {
            continue;
        };
        let first_upper = first.to_ascii_uppercase();

        // EXPLAIN only describes a statement's bytecode; it never executes
        // the underlying statement, so it is always safe.
        if first_upper == "EXPLAIN" {
            continue;
        }

        match first_upper.as_str() {
            "SELECT" | "VALUES" | "WITH" | "PRAGMA" => {}
            other => {
                return Err(BeadsError::validation(
                    "query",
                    format!(
                        "br sql only accepts read-only statements (SELECT, WITH, VALUES, read-only PRAGMA); got '{other}'"
                    ),
                ));
            }
        }

        if first_upper == "PRAGMA" {
            // The pragma name is the second word: `PRAGMA name` or
            // `PRAGMA schema.name`.
            let pragma_name = words
                .get(1)
                .map(|word| word.rsplit('.').next().unwrap_or(word))
                .map(|word| word.trim_start_matches('@').to_ascii_lowercase());
            let Some(name) = pragma_name else {
                return Err(BeadsError::validation(
                    "query",
                    "PRAGMA statement is missing a pragma name",
                ));
            };
            if !READ_ONLY_PRAGMAS.contains(&name.as_str()) {
                return Err(BeadsError::validation(
                    "query",
                    format!(
                        "PRAGMA {name} is not allowed: br sql is read-only and only permits read-only pragmas ({})",
                        READ_ONLY_PRAGMAS.join(", ")
                    ),
                ));
            }
            continue;
        }

        for word in &words {
            let lowered = word.to_ascii_lowercase();
            if FORBIDDEN_KEYWORDS.contains(&lowered.as_str()) {
                return Err(BeadsError::validation(
                    "query",
                    format!("br sql is read-only; '{lowered}' is not allowed"),
                ));
            }
        }
    }

    Ok(())
}

/// Split a SQL string into statements on top-level `;` separators, ignoring
/// semicolons inside string literals, quoted identifiers, and comments.
fn split_sql_statements(query: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut chars = query.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' | '`' => {
                current.push(c);
                // Consume the quoted section verbatim (doubled quote = escape).
                while let Some(inner) = chars.next() {
                    current.push(inner);
                    if inner == c {
                        if chars.peek() == Some(&c) {
                            current.push(chars.next().unwrap_or(c));
                        } else {
                            break;
                        }
                    }
                }
            }
            '[' => {
                current.push(c);
                for inner in chars.by_ref() {
                    current.push(inner);
                    if inner == ']' {
                        break;
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                // Line comment: consume to end of line.
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        current.push(inner);
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for inner in chars.by_ref() {
                    if prev == '*' && inner == '/' {
                        break;
                    }
                    prev = inner;
                }
            }
            ';' => {
                statements.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    statements.push(current);
    statements
}

/// Blank out string literals, quoted identifiers, and comments so keyword
/// scanning cannot be fooled (or broken) by their contents.
fn blank_sql_noise(statement: &str) -> String {
    let mut out = String::with_capacity(statement.len());
    let mut chars = statement.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' | '`' => {
                out.push(' ');
                while let Some(inner) = chars.next() {
                    if inner == c {
                        if chars.peek() == Some(&c) {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
            }
            '[' => {
                out.push(' ');
                for inner in chars.by_ref() {
                    if inner == ']' {
                        break;
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        out.push('\n');
                        break;
                    }
                    out.push(' ');
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for inner in chars.by_ref() {
                    if prev == '*' && inner == '/' {
                        break;
                    }
                    prev = inner;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Extract whitespace-separated words from sanitized SQL code.
fn sql_words(code: &str) -> Vec<String> {
    code.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Render a stored value as a human-readable string.
///
/// The storage classes are matched through `SqlValue::value()` because `SqlValue` is a newtype
/// over `rusqlite::types::Value`, where the float variant is called `Real` rather than `Float`.
fn value_to_string(val: &SqlValue) -> String {
    match val.value() {
        rusqlite::types::Value::Null => "NULL".to_string(),
        rusqlite::types::Value::Integer(n) => n.to_string(),
        rusqlite::types::Value::Real(f) => format!("{f}"),
        rusqlite::types::Value::Text(s) => s.to_string(),
        rusqlite::types::Value::Blob(bytes) => {
            // Show first few bytes as hex with a length indicator.
            let preview: String = bytes.iter().take(8).map(|b| format!("{b:02x}")).collect();
            let suffix = if bytes.len() > 8 { "..." } else { "" };
            format!("[blob {} bytes: {preview}{suffix}]", bytes.len())
        }
    }
}
