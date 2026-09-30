//! `br prime` — AI session context.
//!
//! Outputs a canonical context blob (~1-2k tokens CLI, ~50 tokens MCP/hook) for
//! AI agents at session start. A `.beads/PRIME.md` file, when present,
//! replaces the default template.

use clap::Args;

use crate::config::CliOverrides;
use crate::error::BeadsError;
use crate::output::OutputContext;

/// Arguments for `br prime`.
#[derive(Args, Debug, Clone)]
pub struct PrimeArgs {
    /// Emit the full workflow reference (default when not piped)
    #[arg(long)]
    pub full: bool,

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
"#;

/// Execute the prime command.
pub fn execute(
    args: &PrimeArgs,
    json_mode: bool,
    _overrides: &CliOverrides,
    ctx: &OutputContext,
) -> Result<(), BeadsError> {
    // --export: dump default template and exit
    if args.export {
        println!("{DEFAULT_PRIME_TEMPLATE}");
        return Ok(());
    }

    // Check for a .beads/PRIME.md override (full mode only)
    let beads_dir = discover_optional_beads_dir();
    let prime_md_override = beads_dir
        .as_ref()
        .map(|d| d.join("PRIME.md"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(p).ok());

    let output = prime_md_override.unwrap_or_else(|| format_default_prime(args.stealth));

    if args.hook_json {
        let envelope = serde_json::json!({
            "type": "session_start",
            "version": 1,
            "content": output,
        });
        println!("{}", serde_json::to_string_pretty(&envelope)?);
    } else if json_mode || ctx.is_json() {
        let json_output = serde_json::json!({
            "prime": output,
            "mode": "full",
        });
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!("{output}");
    }

    Ok(())
}

/// Build the default full prime output.
fn format_default_prime(_stealth: bool) -> String {
    DEFAULT_PRIME_TEMPLATE.to_string()
}

/// Discover beads dir without erroring if none exists.
fn discover_optional_beads_dir() -> Option<std::path::PathBuf> {
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
    fn test_default_prime_template_has_no_memory_commands() {
        assert!(!DEFAULT_PRIME_TEMPLATE.contains("br remember"));
        assert!(!DEFAULT_PRIME_TEMPLATE.contains("br forget"));
        assert!(!DEFAULT_PRIME_TEMPLATE.contains("br memory"));
    }
}
