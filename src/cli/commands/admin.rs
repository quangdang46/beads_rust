//! Admin command implementation.
//!
//! Administrative commands for database maintenance and diagnostics.

use crate::cli::{
    AdminCommands, AdminDoctorArgs, AdminResetArgs, AdminStatsArgs, AdminVacuumArgs,
};
use crate::config;
use crate::error::{BeadsError, Result};
use crate::output::OutputContext;

/// Execute the admin command, dispatching to the appropriate subcommand.
///
/// # Errors
///
/// Returns an error if the subcommand fails.
pub fn execute(
    command: &AdminCommands,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    match command {
        AdminCommands::Doctor(args) => execute_doctor(args, cli, ctx),
        AdminCommands::Vacuum(args) => execute_vacuum(args, cli, ctx),
        AdminCommands::Stats(args) => execute_stats(args, cli, ctx),
        AdminCommands::Reset(args) => execute_reset(args, cli, ctx),
    }
}

/// Execute `br admin doctor` — delegates to `br doctor`.
fn execute_doctor(
    _args: &AdminDoctorArgs,
    _cli: &config::CliOverrides,
    _ctx: &OutputContext,
) -> Result<()> {
    // Delegate to the doctor command's execute function.
    let doctor_args = crate::cli::DoctorArgs {
        repair: false,
        ..crate::cli::DoctorArgs::default()
    };
    super::doctor::execute(&doctor_args, _cli, _ctx)
}

/// Execute `br admin vacuum` — runs VACUUM on the SQLite database.
fn execute_vacuum(
    _args: &AdminVacuumArgs,
    cli: &config::CliOverrides,
    _ctx: &OutputContext,
) -> Result<()> {
    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let paths = config::ConfigPaths::resolve(&beads_dir, cli.db.as_ref())?;

    if !paths.db_path.is_file() {
        return Err(crate::error::BeadsError::validation(
            "db_path",
            &format!("Database file not found at {}", paths.db_path.display()),
        ));
    }

    if !_ctx.is_quiet() {
        println!("Vacuuming database: {}", paths.db_path.display());
    }

    let storage = crate::storage::SqliteStorage::open(&paths.db_path)?;
    storage.execute_raw("VACUUM")?;

    if !_ctx.is_quiet() {
        println!("VACUUM completed successfully");
    }

    Ok(())
}

/// Execute `br admin stats` — prints database statistics.
fn execute_stats(
    _args: &AdminStatsArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    // Delegate to the stats command for issue counts by status.
    super::stats::execute(
        &crate::cli::StatsArgs {
            by_type: false,
            by_priority: false,
            by_assignee: false,
            by_label: false,
            activity: false,
            no_activity: true,
            activity_hours: 24,
            format: None,
            stats: false,
            robot: false,
        },
        false,
        cli,
        ctx,
    )
}

/// Execute `br admin reset` — factory reset the database.
///
/// Clears all issues, dependencies, labels, comments, events, and resets
/// sequences. Preserves the database file and workspace structure.
/// Requires `--force` as a safety guard.
fn execute_reset(
    _args: &AdminResetArgs,
    cli: &config::CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    if !_args.force {
        return Err(BeadsError::validation(
            "force",
            "Factory reset requires --force to confirm. This will permanently delete all issues, \
             dependencies, labels, comments, events, and related data. Use `br admin reset --force` \
             to proceed.",
        ));
    }

    let beads_dir = config::discover_beads_dir_with_cli(cli)?;
    let paths = config::ConfigPaths::resolve(&beads_dir, cli.db.as_ref())?;

    if !paths.db_path.is_file() {
        return Err(BeadsError::validation(
            "db_path",
            &format!("Database file not found at {}", paths.db_path.display()),
        ));
    }

    if !ctx.is_quiet() {
        println!("Factory resetting database: {}", paths.db_path.display());
    }

    let mut storage = crate::storage::SqliteStorage::open(&paths.db_path)?;

    // Save the issue prefix before clearing so we can restore it.
    let issue_prefix: String = storage.get_config("issue_prefix")?.unwrap_or_default();

    // Clear all user data tables in dependency-safe order (children first).
    // Tables with FOREIGN KEY constraints referencing issues() must be
    // cleared before issues itself. Disable FK enforcement for the
    // transaction to guarantee clean deletion order.
    storage.execute_raw("PRAGMA foreign_keys = OFF")?;

    // Clear child tables first
    storage.execute_raw("DELETE FROM dependencies")?;
    storage.execute_raw("DELETE FROM labels")?;
    storage.execute_raw("DELETE FROM comments")?;
    storage.execute_raw("DELETE FROM events")?;
    storage.execute_raw("DELETE FROM dirty_issues")?;
    storage.execute_raw("DELETE FROM export_hashes")?;
    storage.execute_raw("DELETE FROM blocked_issues_cache")?;
    storage.execute_raw("DELETE FROM child_counters")?;
    storage.execute_raw("DELETE FROM close_metadata")?;
    storage.execute_raw("DELETE FROM gate_results")?;
    storage.execute_raw("DELETE FROM issue_snapshots")?;
    storage.execute_raw("DELETE FROM compaction_snapshots")?;
    storage.execute_raw("DELETE FROM interactions")?;
    storage.execute_raw("DELETE FROM routes")?;
    storage.execute_raw("DELETE FROM issue_counter")?;
    storage.execute_raw("DELETE FROM repo_mtimes")?;

    // Clear issues last
    storage.execute_raw("DELETE FROM issues")?;

    // Reset sequence/counter tables
    storage.execute_raw("DELETE FROM sqlite_sequence")?;

    // Clear config and metadata — but re-initialize the issue_prefix afterwards
    storage.execute_raw("DELETE FROM config")?;
    storage.execute_raw("DELETE FROM metadata")?;

    // Re-enable FK enforcement
    storage.execute_raw("PRAGMA foreign_keys = ON")?;

    // Re-initialize essential config
    if !issue_prefix.is_empty() {
        storage.set_config("issue_prefix", &issue_prefix)?;
    }

    if !ctx.is_quiet() {
        println!("Factory reset complete. Database is ready for new issues.");
        if !issue_prefix.is_empty() {
            println!("Preserved issue prefix: {issue_prefix}");
        }
    }

    Ok(())
}
