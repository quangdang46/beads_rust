//! Memory commands — persistent agent memory (br remember / memories / recall / forget).
//!
//! Memories are stored in the config table with `kv.memory.<key>` prefix,
//! matching Go bd's `kvkeys.MemoryConfigKeyPrefix` pattern.

use clap::Subcommand;

use crate::config::{discover_beads_dir_with_cli, open_storage_with_cli, CliOverrides};
use crate::error::BeadsError;
use crate::output::OutputContext;

/// Key prefix for persistent agent memories.
const MEMORY_KEY_PREFIX: &str = "kv.memory.";

/// Build the full storage key for a memory slug.
fn memory_key(slug: &str) -> String {
    format!("{MEMORY_KEY_PREFIX}{slug}")
}

/// Extract the slug from a full memory key (strip prefix).
fn slug_from_key(key: &str) -> Option<&str> {
    key.strip_prefix(MEMORY_KEY_PREFIX)
}

/// Slugify a string for use as a memory key.
fn slugify(text: &str) -> String {
    text.chars()
        .take(40)
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' {
            c
        } else if c.is_whitespace() {
            '-'
        } else {
            '_'
        })
        .collect::<String>()
        .trim_matches('-')
        .trim_matches('_')
        .to_lowercase()
}

/// Memory subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum MemoryCommands {
    /// Store a persistent memory (upsert)
    Remember {
        /// The memory content/text to store
        text: String,

        /// Optional key/slug (auto-generated from content if omitted)
        #[arg(long, short = 'k')]
        key: Option<String>,
    },

    /// List all stored memories (optional search filter)
    #[command(alias = "list")]
    Memories {
        /// Optional search text to filter memories
        search: Option<String>,
    },

    /// Recall a specific memory by key
    Recall {
        /// Memory key/slug
        key: String,
    },

    /// Delete a stored memory by key
    #[command(alias = "delete")]
    Forget {
        /// Memory key/slug
        key: String,
    },
}

/// Execute a memory subcommand.
pub fn execute(
    command: &MemoryCommands,
    json_mode: bool,
    overrides: &CliOverrides,
    output_ctx: &OutputContext,
) -> Result<(), BeadsError> {
    match command {
        MemoryCommands::Remember { text, key } => remember(text, key, overrides, output_ctx),
        MemoryCommands::Memories { search } => memories(search.as_deref(), json_mode, overrides, output_ctx),
        MemoryCommands::Recall { key } => recall(key, json_mode, overrides, output_ctx),
        MemoryCommands::Forget { key } => forget(key, json_mode, overrides, output_ctx),
    }
}

fn get_storage(overrides: &CliOverrides) -> Result<(crate::storage::SqliteStorage, std::path::PathBuf), BeadsError> {
    let beads_dir = discover_beads_dir_with_cli(overrides)?;
    let storage_result = open_storage_with_cli(&beads_dir, overrides)?;
    Ok((storage_result.storage, beads_dir))
}

fn remember(
    text: &str,
    key: &Option<String>,
    overrides: &CliOverrides,
    ctx: &OutputContext,
) -> Result<(), BeadsError> {
    let slug = match key {
        Some(k) => slugify(k),
        None => slugify(text),
    };
    let full_key = memory_key(&slug);

    let (mut storage, _beads_dir) = get_storage(overrides)?;
    storage.set_config(&full_key, text)?;

    if ctx.is_json() {
        let msg = serde_json::json!({
            "status": "stored",
            "key": slug,
            "memory_key": full_key,
        });
        println!("{msg}");
    } else {
        eprintln!("Memory stored as '{slug}'");
    }
    Ok(())
}

fn memories(
    search: Option<&str>,
    json_mode: bool,
    overrides: &CliOverrides,
    ctx: &OutputContext,
) -> Result<(), BeadsError> {
    let (storage, _beads_dir) = get_storage(overrides)?;
    let all_config = storage.get_all_config()?;

    // Filter to only kv.memory.* keys
    let mut memories: Vec<(&str, &str)> = all_config
        .iter()
        .filter_map(|(k, v)| slug_from_key(k).map(|slug| (slug, v.as_str())))
        .collect();

    // Optional search filter
    if let Some(query) = search {
        let q = query.to_lowercase();
        memories.retain(|(slug, val)| slug.contains(&q) || val.to_lowercase().contains(&q));
    }

    // Sort by slug for deterministic output
    memories.sort_by(|a, b| a.0.cmp(b.0));

    if json_mode || ctx.is_json() {
        let entries: Vec<serde_json::Value> = memories
            .iter()
            .map(|(slug, val)| {
                serde_json::json!({
                    "key": slug,
                    "value": val,
                })
            })
            .collect();
        let msg = if let Some(query) = search {
            serde_json::json!({
                "count": entries.len(),
                "search": query,
                "memories": entries,
            })
        } else {
            serde_json::json!({
                "count": entries.len(),
                "memories": entries,
            })
        };
        println!("{msg}");
    } else {
        if memories.is_empty() {
            let suffix = search.map(|q| format!(" matching '{q}'")).unwrap_or_default();
            println!("No memories stored{suffix}.");
            return Ok(());
        }
        for (slug, val) in &memories {
            // Truncate value for display
            let truncated = if val.len() > 80 {
                format!("{}…", &val[..77])
            } else {
                val.to_string()
            };
            println!("  {slug:<28} {truncated}");
        }
    }
    Ok(())
}

fn recall(
    key: &str,
    json_mode: bool,
    overrides: &CliOverrides,
    ctx: &OutputContext,
) -> Result<(), BeadsError> {
    let full_key = memory_key(key);
    let (storage, _beads_dir) = get_storage(overrides)?;

    match storage.get_config(&full_key)? {
        Some(value) => {
            if json_mode || ctx.is_json() {
                let msg = serde_json::json!({
                    "key": key,
                    "value": value,
                });
                println!("{msg}");
            } else {
                println!("{value}");
            }
            Ok(())
        }
        None => Err(BeadsError::Config(format!(
            "Memory not found: '{key}'"
        ))),
    }
}

fn forget(
    key: &str,
    json_mode: bool,
    overrides: &CliOverrides,
    ctx: &OutputContext,
) -> Result<(), BeadsError> {
    let full_key = memory_key(key);
    let (mut storage, _beads_dir) = get_storage(overrides)?;

    let deleted = storage.delete_config(&full_key)?;
    if deleted {
        if json_mode || ctx.is_json() {
            let msg = serde_json::json!({
                "status": "deleted",
                "key": key,
            });
            println!("{msg}");
        } else {
            eprintln!("Memory '{key}' deleted.");
        }
        Ok(())
    } else {
        Err(BeadsError::Config(format!(
            "Memory not found: '{key}'"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_key() {
        assert_eq!(memory_key("foo"), "kv.memory.foo");
        assert_eq!(memory_key("my-insight"), "kv.memory.my-insight");
    }

    #[test]
    fn test_slug_from_key() {
        assert_eq!(slug_from_key("kv.memory.foo"), Some("foo"));
        assert_eq!(slug_from_key("kv.memory.my-insight"), Some("my-insight"));
        assert_eq!(slug_from_key("config.key"), None);
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("ALLCAPS"), "allcaps");
        assert_eq!(slugify("special chars!@#$"), "special-chars");
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("a".repeat(100).as_str()).len(), 40);
    }
}
