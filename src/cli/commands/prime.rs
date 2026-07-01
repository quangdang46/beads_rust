//! `br prime` — AI session context with persistent memory injection.
//!
//! Outputs a canonical context blob (~1-2k tokens CLI, ~50 tokens MCP/hook) for
//! AI agents at session start. Injects persistent memories from kv.memory.*.

use clap::Args;

use crate::config::{discover_beads_dir_with_cli, open_storage_with_cli, CliOverrides};
use crate::error::BeadsError;
use crate::output::OutputContext;

/// Key prefix for persistent agent memories — mirrors `memory.rs`.
const MEMORY_KEY_PREFIX: &str = "kv.memory.";

/// Extract the slug from a full memory key (strip prefix).
fn slug_from_key(key: &str) -> Option<&str> {
    key.strip_prefix(MEMORY_KEY_PREFIX)
}

/// Arguments for `br prime`.
#[derive(Args, Debug, Clone)]
pub struct PrimeArgs {
    /// Emit the full workflow reference + memories (default when not piped)
    #[arg(long, conflicts_with = "memories_only")]
    pub full: bool,

    /// Compact mode (MCP-friendly, ~50 tokens of reminders)
    #[arg(long, conflicts_with_all = &["full", "memories_only"])]
    pub mcp: bool,

    /// Output only the memories section (for pre-compact / post-compaction hooks)
    #[arg(long, conflicts_with_all = &["full", "mcp"])]
    pub memories_only: bool,

    /// Wrap output in a SessionStart hook JSON envelope
    #[arg(long)]
    pub hook_json: bool,

    /// Dump the default prime template to stdout (ignore .beads/PRIME.md)
    #[arg(long)]
    pub export: bool,

    /// Omit git commit/push steps from close protocol
    #[arg(long)]
    pub stealth: bool,
}

/// Default prime template content (returned by `--export`).
const DEFAULT_PRIME_TEMPLATE: &str = r#"# br — Dependency-Aware Issue Tracker

## Key commands

- `br ready --json` — Show unblocked work (use this first)
- `br list --status=open --json` — All open issues
- `br show <id> --json` — Full issue details
- `br create --title="..." --type=task --priority=2`
- `br update <id> --status=in_progress`
- `br close <id> --reason "Completed"`
- `br sync --flush-only` — Export to JSONL (NO git operations)
- `br remember <text>` — Store a persistent memory

## Workflow

1. `br ready --json` → pick highest priority, no blockers
2. `br update <id> --status=in_progress --assignee "$AGENT_NAME"`
3. Implement the task
4. `br close <id> --reason "Completed"`
5. `br sync --flush-only`

## Session end

```bash
git status
git add <files>
br sync --flush-only
git add .beads/
git commit -m "..."
git push
```

## Persistent memories

Below are your stored memories from previous sessions.
Use `br remember "..."` to add more, `br forget <key>` to remove one.
"#;

/// Default MCP-mode prime content (compact).
const DEFAULT_MCP_PRIME: &str = "br: dependency-aware issue tracker.\n\
    br ready --json → pick work | br show <id> → details | \
    br close <id> → finish | br sync --flush-only → export (NO git)\n";

/// Execute the prime command.
pub fn execute(
    args: &PrimeArgs,
    json_mode: bool,
    overrides: &CliOverrides,
    ctx: &OutputContext,
) -> Result<(), BeadsError> {
    // --export: dump default template and exit
    if args.export {
        let template = if args.mcp {
            DEFAULT_MCP_PRIME
        } else {
            DEFAULT_PRIME_TEMPLATE
        };
        println!("{template}");
        return Ok(());
    }

    // Try to load memories, but don't fail if no beads dir / DB
    let memories = load_memories(overrides);

    // Check for .beads/PRIME.md override (full mode only, not memories-only)
    let beads_dir = discover_optional_beads_dir(overrides);
    let prime_md_override = if !args.memories_only && !args.mcp {
        beads_dir.as_ref().map(|d| d.join("PRIME.md"))
            .filter(|p| p.exists())
            .and_then(|p| std::fs::read_to_string(p).ok())
    } else {
        None
    };

    let output = if args.memories_only {
        format_memories_block(&memories)
    } else if args.mcp {
        format_mcp_prime(&memories)
    } else if let Some(md_content) = prime_md_override {
        // If the PRIME.md exists, append memories after it
        let mem_block = format_memories_block(&memories);
        if mem_block.is_empty() {
            md_content
        } else {
            format!("{md_content}\n\n{mem_block}")
        }
    } else {
        format_default_prime(&memories, args.stealth)
    };

    if args.hook_json {
        let envelope = serde_json::json!({
            "type": "session_start",
            "version": 1,
            "content": output,
        });
        if json_mode || ctx.is_json() {
            println!("{}", serde_json::to_string_pretty(&envelope)?);
        } else {
            println!("{output}");
        }
    } else if json_mode || ctx.is_json() {
        let json_output = serde_json::json!({
            "prime": output,
            "memories_count": memories.len(),
            "mode": if args.memories_only { "memories_only" }
                else if args.mcp { "mcp" }
                else { "full" },
        });
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!("{output}");
    }

    Ok(())
}

/// Load memories from the config table.
fn load_memories(overrides: &CliOverrides) -> Vec<(String, String)> {
    let beads_dir = match discover_optional_beads_dir(overrides) {
        Some(d) => d,
        None => return vec![],
    };
    let storage_result = match open_storage_with_cli(&beads_dir, overrides) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let all_config = storage_result.storage.get_all_config().unwrap_or_default();

    let mut memories: Vec<(String, String)> = all_config
        .into_iter()
        .filter_map(|(k, v)| slug_from_key(&k).map(|slug| (slug.to_string(), v)))
        .collect();

    memories.sort_by(|a, b| a.0.cmp(&b.0));
    memories
}

/// Format the memories section (empty string if no memories).
fn format_memories_block(memories: &[(String, String)]) -> String {
    if memories.is_empty() {
        return String::new();
    }
    let mut block = String::from("## Persistent Memories\n\n");
    for (slug, val) in memories {
        // Use first line of value as summary
        let summary = val.lines().next().unwrap_or("");
        let truncated = if summary.len() > 80 {
            format!("{}…", &summary[..77])
        } else {
            summary.to_string()
        };
        block.push_str(&format!("- **{slug}**: {truncated}\n"));
    }
    block
}

/// Build the default full prime output.
fn format_default_prime(memories: &[(String, String)], _stealth: bool) -> String {
    let mem_block = format_memories_block(memories);
    if mem_block.is_empty() {
        // Strip the "## Persistent memories" placeholder from the default template
        let trimmed = DEFAULT_PRIME_TEMPLATE
            .split("\n## Persistent memories")
            .next()
            .unwrap_or(DEFAULT_PRIME_TEMPLATE)
            .trim_end();
        format!("{trimmed}\n")
    } else {
        format!("{}\n\n{mem_block}", DEFAULT_PRIME_TEMPLATE)
    }
}

/// Build the MCP-mode prime output.
fn format_mcp_prime(memories: &[(String, String)]) -> String {
    let mut out = String::from(DEFAULT_MCP_PRIME);
    if !memories.is_empty() {
        out.push_str("memories: ");
        let slugs: Vec<&str> = memories.iter().map(|(s, _)| s.as_str()).collect();
        out.push_str(&slugs.join(", "));
        out.push('\n');
    }
    out
}

/// Discover beads dir without erroring if none exists.
fn discover_optional_beads_dir(overrides: &CliOverrides) -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(".beads");
        if candidate.is_dir() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slug_from_key() {
        assert_eq!(slug_from_key("kv.memory.foo"), Some("foo"));
        assert_eq!(slug_from_key("kv.memory.my-key"), Some("my-key"));
        assert_eq!(slug_from_key("config.value"), None);
    }

    #[test]
    fn test_format_memories_block_empty() {
        assert_eq!(format_memories_block(&[]), "");
    }

    #[test]
    fn test_format_memories_block_with_entries() {
        let memories = vec![
            ("quickstart".to_string(), "Run br ready first".to_string()),
            ("db-schema".to_string(), "Schema is at src/storage/schema.rs".to_string()),
        ];
        let block = format_memories_block(&memories);
        assert!(block.contains("quickstart"));
        assert!(block.contains("db-schema"));
        assert!(block.contains("Run br ready first"));
    }

    #[test]
    fn test_format_mcp_prime_with_memories() {
        let memories = vec![
            ("key1".to_string(), "val1".to_string()),
            ("key2".to_string(), "val2".to_string()),
        ];
        let output = format_mcp_prime(&memories);
        assert!(output.contains("memories: key1, key2"));
    }

    #[test]
    fn test_format_mcp_prime_empty() {
        let output = format_mcp_prime(&[]);
        assert!(!output.contains("memories:"));
        assert!(output.contains("br ready"));
    }
}
