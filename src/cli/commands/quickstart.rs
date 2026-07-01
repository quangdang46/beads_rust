//! Quick start command — prints an onboarding guide with common workflows.

use crate::error::Result;
use crate::output::OutputContext;

/// Execute the quickstart command.
pub fn execute(_output: &OutputContext) -> Result<()> {
    println!();
    println!("  br — Beads Rust Issue Tracker");
    println!("  Dependency-aware issue tracking for AI agents and humans.");
    println!();

    println!("  ╔══════════════════════════════════════╗");
    println!("  ║        GETTING STARTED               ║");
    println!("  ╚══════════════════════════════════════╝");
    println!();
    println!("    br init                        Initialize br in your project");
    println!("    br init --prefix api           Initialize with custom prefix (api-<hash>)");
    println!();

    println!("  ───── CREATING ISSUES ─────");
    println!();
    println!("    br create \"Fix login bug\"");
    println!("    br create \"Add auth\" -p 0 -t feature");
    println!("    br create \"Write tests\" -d \"Unit tests\" --assignee alice");
    println!("    br q \"Quick note\"            Quick capture (lightweight)");
    println!();

    println!("  ───── VIEWING ISSUES ─────");
    println!();
    println!("    br list                     List all issues");
    println!("    br list --status open       List by status");
    println!("    br list --priority 0        List by priority (0-4, 0=highest)");
    println!("    br show <id>                Show issue details");
    println!("    br search \"query\"           Search by text");
    println!();

    println!("  ───── MANAGING DEPENDENCIES ─────");
    println!();
    println!("    br dep add <a> <b>          B blocks A");
    println!("    br dep remove <a> <b>       Remove dependency");
    println!("    br graph <id>               Visualize dependency tree");
    println!("    br dep cycles               Detect circular dependencies");
    println!();

    println!("  ───── READY WORK (agent-friendly) ─────");
    println!();
    println!("    br ready                    Show issues ready to work on (no blockers)");
    println!("    br ready --json             Machine-readable (for programmatic parsing)");
    println!();

    println!("  ───── UPDATING ISSUES ─────");
    println!();
    println!("    br update <id> --claim");
    println!("    br update <id> --priority 0");
    println!("    br update <id> --status in_progress");
    println!("    br update <id> --add-label backend");
    println!();

    println!("  ───── CLOSING ISSUES ─────");
    println!();
    println!("    br close <id>");
    println!("    br close <id1> <id2> --reason \"Fixed in PR #42\"");
    println!();

    println!("  ───── SYNC & STORAGE ─────");
    println!();
    println!("  br stores issues in local SQLite with JSONL export for git-based sync:");
    println!("    br sync --flush-only        Export to JSONL (does NOT run git)");
    println!("    git add .beads/ && git commit   Commit beads changes");
    println!();

    println!("  ───── AGENT INTEGRATION ─────");
    println!();
    println!("  br is designed for AI-supervised workflows:");
    println!("    * Agents create issues when discovering new work");
    println!("    * `br ready` shows unblocked work ready to claim");
    println!("    * Use --json flags for programmatic parsing");
    println!("    * Dependencies prevent agents from duplicating effort");
    println!();

    println!("  ───── ONLINE HELP ─────");
    println!();
    println!("    br --help                List all commands");
    println!("    br <command> --help      Command-specific help");
    println!();

    println!("  Ready to start!");
    println!("  Run `br create \"My first issue\"` to create your first issue.");
    println!();

    Ok(())
}
