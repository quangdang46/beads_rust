//! Admin command implementation.
//!
//! Administrative commands for database maintenance and diagnostics.

use crate::cli::{AdminCommands, AdminDoctorArgs, AdminVacuumArgs, AdminStatsArgs};
use crate::config;
use crate::error::Result;
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
