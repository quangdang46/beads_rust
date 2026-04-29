use beads_rust::cli::commands;
use beads_rust::cli::{Cli, Commands, OutputFormat, command_requests_robot_json};
use beads_rust::config;
use beads_rust::logging::init_logging;
use beads_rust::output::OutputContext;
use beads_rust::sync::{
    auto_flush, auto_import_if_stale, auto_import_probe, auto_import_probe_refreshing_witnesses,
};
use beads_rust::{BeadsError, Result, StructuredError};
use clap::{CommandFactory, Parser};
use clap_complete::CompleteEnv;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

#[allow(clippy::too_many_lines)]
fn main() {
    CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();
    let json_error_mode = should_render_errors_as_json(&cli);
    let color_error_mode = should_color_human_errors_for_cli(&cli);
    let output_ctx = OutputContext::from_args(&cli);
    let is_mutating = is_mutating_command(&cli.command);
    let command_supports_auto_import = should_auto_import(&cli.command);

    // Initialize logging
    if let Err(e) = init_logging(cli.verbose, cli.quiet, None) {
        eprintln!("Failed to initialize logging: {e}");
    }

    let overrides = build_cli_overrides(&cli);

    // Phase 1: Startup & Discovery (One-time)
    let mut ctx = match StartupContext::init(&overrides) {
        Ok(ctx) => ctx,
        Err(e) => {
            if command_supports_auto_import {
                handle_error(&e, json_error_mode, color_error_mode);
            }
            StartupContext::empty(overrides.clone())
        }
    };

    let storage_enabled = ctx.is_initialized() && !ctx.no_db();
    let should_auto_import_now =
        command_supports_auto_import && !cli.allow_stale && !ctx.no_auto_import();
    let should_auto_flush_now = is_mutating && !ctx.no_auto_flush();
    let needs_preopened_storage_context = should_auto_import_now || should_auto_flush_now;
    let should_preopen_storage =
        should_preopen_storage(storage_enabled, needs_preopened_storage_context);
    let command_needs_write_lock = needs_write_lock(&cli.command);

    // Phase 1.5: Acquire exclusive write lock before any DB-family open that
    // may apply schema, recover, quarantine sidecars, write metadata, or read
    // from fsqlite while another process is in a write transaction.
    //
    // Issue #243: frankensqlite deadlocks when multiple processes attempt
    // concurrent writes to the same database file. Serialize all mutating
    // operations through a blocking flock on `.beads/.write.lock`. Normal
    // storage open is not guaranteed read-only in recovery/schema paths, and
    // fsqlite can still return SQLITE_BUSY for read-only SELECTs during
    // concurrent writers, so every preopened DB command acquires the advisory
    // lock before Phase 2.
    let write_lock =
        if should_acquire_startup_write_lock(command_needs_write_lock, should_preopen_storage)
            && ctx.is_initialized()
        {
            let lock_timeout = ctx.write_lock_timeout();
            match ctx.beads_dir.as_deref().map(|beads_dir| {
                beads_rust::sync::blocking_write_lock_with_timeout(beads_dir, lock_timeout)
            }) {
                Some(Ok(lock)) => Some(lock),
                Some(Err(e)) => handle_error(&e, json_error_mode, color_error_mode),
                None => None,
            }
        } else {
            None
        };

    // Phase 2: Open Storage (One-time)
    let mut storage_result = if should_preopen_storage {
        match open_storage_from_ctx(&mut ctx) {
            Ok(res) => Some(res),
            Err(e) => {
                if should_auto_import_now {
                    handle_error(&e, json_error_mode, color_error_mode);
                }
                None
            }
        }
    } else {
        None
    };

    // Phase 3: Auto-Import. Normal staleness probes can opportunistically
    // refresh JSONL witness metadata. Read-only startup probes skip that
    // refresh and reopen writable storage only when an import is actually
    // needed.
    if let Some(paths) = ctx.paths.as_ref()
        && should_auto_import_now
        && storage_result.is_some()
    {
        let mut auto_import_write_lock = None;
        if !ctx.overrides.read_only_fast_open && write_lock.is_none() {
            let lock_timeout = ctx.write_lock_timeout();
            auto_import_write_lock = match ctx.beads_dir.as_deref().map(|beads_dir| {
                beads_rust::sync::blocking_write_lock_with_timeout(beads_dir, lock_timeout)
            }) {
                Some(Ok(lock)) => Some(lock),
                Some(Err(e)) => handle_error(&e, json_error_mode, color_error_mode),
                None => None,
            };
        }
        let should_attempt_auto_import = {
            match storage_result.as_mut() {
                Some(res) if ctx.overrides.read_only_fast_open => {
                    auto_import_probe(&res.storage, &paths.jsonl_path).unwrap_or(true)
                }
                Some(res) => {
                    auto_import_probe_refreshing_witnesses(&mut res.storage, &paths.jsonl_path)
                        .unwrap_or(true)
                }
                None => false,
            }
        };

        if should_attempt_auto_import {
            if ctx.overrides.read_only_fast_open && write_lock.is_none() {
                let lock_timeout = ctx.write_lock_timeout();
                auto_import_write_lock = match ctx.beads_dir.as_deref().map(|beads_dir| {
                    beads_rust::sync::blocking_write_lock_with_timeout(beads_dir, lock_timeout)
                }) {
                    Some(Ok(lock)) => Some(lock),
                    Some(Err(e)) => handle_error(&e, json_error_mode, color_error_mode),
                    None => None,
                };
            }

            if ctx.overrides.read_only_fast_open {
                let mut writable_overrides = ctx.overrides.clone();
                writable_overrides.read_only_fast_open = false;
                drop(storage_result.take());
                match config::open_storage_with_cli(&paths.beads_dir, &writable_overrides) {
                    Ok(writable_res) => storage_result = Some(writable_res),
                    Err(e) => handle_error(&e, json_error_mode, color_error_mode),
                }
            }

            let _ = auto_import_write_lock.as_ref();
            let sync_lock = match ctx
                .beads_dir
                .as_deref()
                .map(beads_rust::sync::try_sync_lock)
            {
                Some(Ok(Some(lock))) => Some(lock),
                Some(Ok(None)) => {
                    tracing::debug!("Auto-import skipped because .sync.lock is held");
                    None
                }
                Some(Err(e)) => handle_error(&e, json_error_mode, color_error_mode),
                None => None,
            };
            if sync_lock.is_some()
                && let Some(res) = storage_result.as_mut()
            {
                let allow_external_jsonl = config::implicit_external_jsonl_allowed(
                    &paths.beads_dir,
                    &paths.db_path,
                    &paths.jsonl_path,
                );
                let expected_prefix = match resolve_auto_import_expected_prefix(res, &ctx.overrides)
                {
                    Ok(prefix) => Some(prefix),
                    Err(e) => {
                        handle_error(&e, json_error_mode, color_error_mode);
                    }
                };
                let outcome = auto_import_if_stale(
                    &mut res.storage,
                    &paths.beads_dir,
                    &paths.jsonl_path,
                    expected_prefix.as_deref(),
                    allow_external_jsonl,
                    false,
                    false,
                );
                if let Err(e) = outcome {
                    handle_error(&e, json_error_mode, color_error_mode);
                }
            }
            // sync_lock drops here, releasing the advisory lock before command execution
        }
    }

    // Phase 4: Command Execution
    let result = match cli.command {
        Commands::Init {
            prefix,
            force,
            backend: _,
        } => commands::init::execute(prefix, force, None, &output_ctx),
        Commands::Create(args) => {
            execute_create_command(&args, &overrides, &output_ctx, &mut storage_result)
        }
        Commands::Update(args) => commands::update::execute(&args, &overrides, &output_ctx),
        Commands::Delete(args) => {
            commands::delete::execute(&args, cli.json, &overrides, &output_ctx)
        }
        Commands::List(args) => {
            if let Some(res) = storage_result.as_ref() {
                commands::list::execute_with_storage(&args, &overrides, &output_ctx, res)
            } else {
                commands::list::execute(&args, cli.json, &overrides, &output_ctx)
            }
        }
        Commands::Comments(args) => {
            commands::comments::execute(&args, cli.json, &overrides, &output_ctx)
        }
        Commands::Search(args) => {
            if let Some(res) = storage_result.as_ref() {
                commands::search::execute_with_storage_ctx(&args, &overrides, &output_ctx, res)
            } else {
                commands::search::execute(&args, cli.json, &overrides, &output_ctx)
            }
        }
        Commands::Show(args) => {
            if let (Some(res), Some(beads_dir)) = (storage_result.as_ref(), ctx.beads_dir.as_ref())
            {
                commands::show::execute_with_storage_ctx(
                    &args,
                    &overrides,
                    &output_ctx,
                    beads_dir,
                    res,
                )
            } else {
                commands::show::execute(&args, cli.json, &overrides, &output_ctx)
            }
        }
        Commands::Close(args) => {
            commands::close::execute_cli(&args, cli.json || args.robot, &overrides, &output_ctx)
        }
        Commands::Reopen(args) => {
            commands::reopen::execute(&args, cli.json || args.robot, &overrides, &output_ctx)
        }
        Commands::Q(args) => commands::q::execute(args, &overrides, &output_ctx),
        Commands::Dep { command } => {
            commands::dep::execute(&command, cli.json, &overrides, &output_ctx)
        }
        Commands::Epic { command } => {
            commands::epic::execute(&command, cli.json, &overrides, &output_ctx)
        }
        Commands::Label { command } => {
            if let Some(res) = storage_result.as_ref() {
                match commands::label::execute_with_storage(
                    &command,
                    cli.json,
                    &output_ctx,
                    &res.storage,
                ) {
                    Ok(true) => Ok(()),
                    Ok(false) => {
                        commands::label::execute(&command, cli.json, &overrides, &output_ctx)
                    }
                    Err(err) => Err(err),
                }
            } else {
                commands::label::execute(&command, cli.json, &overrides, &output_ctx)
            }
        }
        Commands::Count(args) => {
            if let Some(res) = storage_result.as_ref() {
                commands::count::execute_with_storage(&args, &output_ctx, &res.storage)
            } else {
                commands::count::execute(&args, cli.json, &overrides, &output_ctx)
            }
        }
        Commands::Stale(args) => storage_result.as_ref().map_or_else(
            || commands::stale::execute(&args, &overrides, &output_ctx),
            |res| commands::stale::execute_with_storage(&args, &output_ctx, &res.storage),
        ),
        Commands::Lint(args) => commands::lint::execute(&args, cli.json, &overrides, &output_ctx),
        Commands::Ready(args) => {
            if let (Some(res), Some(beads_dir)) = (storage_result.as_ref(), ctx.beads_dir.as_ref())
            {
                commands::ready::execute_with_storage_ctx(
                    &args,
                    &overrides,
                    &output_ctx,
                    beads_dir,
                    res,
                )
            } else {
                commands::ready::execute(&args, cli.json, &overrides, &output_ctx)
            }
        }
        Commands::Blocked(args) => {
            if let (Some(res), Some(beads_dir)) = (storage_result.as_ref(), ctx.beads_dir.as_ref())
            {
                commands::blocked::execute_with_storage_ctx(
                    &args,
                    &overrides,
                    &output_ctx,
                    beads_dir,
                    res,
                )
            } else {
                commands::blocked::execute(&args, cli.json || args.robot, &overrides, &output_ctx)
            }
        }
        Commands::Sync(args) => commands::sync::execute(&args, cli.json, &overrides, &output_ctx),
        Commands::Doctor(args) => commands::doctor::execute(&args, &overrides, &output_ctx),
        Commands::Info(args) => commands::info::execute(&args, &overrides, &output_ctx),
        Commands::Schema(args) => commands::schema::execute(&args, &overrides, &output_ctx),
        Commands::Where => commands::r#where::execute(&overrides, &output_ctx),
        Commands::Version(args) => commands::version::execute(&args, &output_ctx),

        #[cfg(feature = "mcp")]
        Commands::Serve(args) => beads_rust::mcp::run_serve(&args, &overrides),

        #[cfg(feature = "self_update")]
        Commands::Upgrade(args) => commands::upgrade::execute(&args, &output_ctx),
        Commands::Completions(args) => commands::completions::execute(&args, &output_ctx),
        Commands::Audit { command } => {
            commands::audit::execute(&command, cli.json, &overrides, &output_ctx)
        }
        Commands::Stats(args) | Commands::Status(args) => {
            if let (Some(res), Some(beads_dir)) = (storage_result.as_ref(), ctx.beads_dir.as_ref())
            {
                commands::stats::execute_with_storage_ctx(
                    &args,
                    &overrides,
                    &output_ctx,
                    beads_dir,
                    res,
                )
            } else {
                commands::stats::execute(&args, cli.json || args.robot, &overrides, &output_ctx)
            }
        }
        Commands::Config { command } => {
            commands::config::execute(&command, cli.json, &overrides, &output_ctx)
        }
        Commands::History(args) => commands::history::execute(args, &overrides, &output_ctx),
        Commands::Defer(args) => {
            commands::defer::execute_defer(&args, cli.json || args.robot, &overrides, &output_ctx)
        }
        Commands::Undefer(args) => {
            commands::defer::execute_undefer(&args, cli.json || args.robot, &overrides, &output_ctx)
        }
        Commands::Orphans(args) => {
            if let (Some(res), Some(beads_dir)) = (storage_result.as_ref(), ctx.beads_dir.as_ref())
            {
                commands::orphans::execute_with_storage_ctx(
                    &args,
                    cli.json || args.robot,
                    &overrides,
                    &output_ctx,
                    beads_dir,
                    res,
                )
            } else {
                commands::orphans::execute(&args, cli.json || args.robot, &overrides, &output_ctx)
            }
        }
        Commands::Changelog(args) => {
            if let (Some(res), Some(beads_dir)) = (storage_result.as_ref(), ctx.beads_dir.as_ref())
            {
                commands::changelog::execute_with_storage_ctx(
                    &args,
                    cli.json || args.robot,
                    &output_ctx,
                    beads_dir,
                    res,
                )
            } else {
                commands::changelog::execute(&args, cli.json || args.robot, &overrides, &output_ctx)
            }
        }
        Commands::Query { command } => commands::query::execute(&command, &overrides, &output_ctx),
        Commands::Graph(args) => storage_result.as_ref().map_or_else(
            || commands::graph::execute(&args, &overrides, &output_ctx),
            |res| {
                commands::graph::execute_with_storage_ctx(
                    &args,
                    &overrides,
                    &output_ctx,
                    ctx.beads_dir
                        .as_deref()
                        .expect("preopened graph storage should have a beads dir"),
                    res,
                )
            },
        ),
        Commands::Agents(args) => {
            let agents_args = commands::agents::AgentsArgs {
                add: args.add,
                remove: args.remove,
                update: args.update,
                check: args.check,
                dry_run: args.dry_run,
                force: args.force,
            };
            commands::agents::execute(&agents_args, &output_ctx)
        }
    };

    // Handle command result
    if let Err(e) = result {
        handle_error(&e, json_error_mode, color_error_mode);
    }

    // Phase 5: Auto-Flush (with advisory flock to serialize concurrent access)
    if is_mutating
        && !ctx.no_auto_flush()
        && let (Some(res), Some(paths)) = (storage_result.as_mut(), ctx.paths.as_ref())
    {
        let sync_lock = match beads_rust::sync::try_sync_lock(&paths.beads_dir) {
            Ok(Some(lock)) => Some(lock),
            Ok(None) => {
                let err = BeadsError::Config(format!(
                    "Automatic JSONL export skipped because sync lock at {} is held by another process",
                    paths.beads_dir.join(".sync.lock").display()
                ));
                commands::report_auto_flush_failure(
                    &output_ctx,
                    &paths.beads_dir,
                    &paths.jsonl_path,
                    &err,
                );
                None
            }
            Err(e) => {
                commands::report_auto_flush_failure(
                    &output_ctx,
                    &paths.beads_dir,
                    &paths.jsonl_path,
                    &e,
                );
                None
            }
        };

        if let Some(_sync_lock) = sync_lock
            && let Err(e) = auto_flush(
                &mut res.storage,
                &paths.beads_dir,
                &paths.jsonl_path,
                config::implicit_external_jsonl_allowed(
                    &paths.beads_dir,
                    &paths.db_path,
                    &paths.jsonl_path,
                ),
            )
        {
            commands::report_auto_flush_failure(
                &output_ctx,
                &paths.beads_dir,
                &paths.jsonl_path,
                &e,
            );
        }
    }
}

struct StartupContext {
    overrides: config::CliOverrides,
    startup: Option<config::StartupConfig>,
    beads_dir: Option<PathBuf>,
    paths: Option<config::ConfigPaths>,
    config: Option<config::ConfigLayer>,
}

impl StartupContext {
    fn init(overrides: &config::CliOverrides) -> Result<Self> {
        let beads_dir = config::discover_beads_dir_with_cli(overrides)?;
        let startup = config::load_startup_config_with_paths(&beads_dir, overrides.db.as_ref())?;

        // Merge startup config with CLI overrides to form the effective bootstrap config
        let mut final_config = startup.merged_config.clone();
        final_config.merge_from(&overrides.as_layer());
        let paths = startup.paths.clone();

        Ok(Self {
            overrides: overrides.clone(),
            startup: Some(startup),
            beads_dir: Some(beads_dir),
            paths: Some(paths),
            config: Some(final_config),
        })
    }

    fn empty(overrides: config::CliOverrides) -> Self {
        Self {
            overrides,
            startup: None,
            beads_dir: None,
            paths: None,
            config: None,
        }
    }

    fn is_initialized(&self) -> bool {
        self.beads_dir.is_some()
    }

    fn no_db(&self) -> bool {
        self.config
            .as_ref()
            .and_then(config::no_db_from_layer)
            .unwrap_or(false)
    }

    fn no_auto_import(&self) -> bool {
        self.config
            .as_ref()
            .and_then(config::no_auto_import_from_layer)
            .unwrap_or(false)
    }

    fn no_auto_flush(&self) -> bool {
        self.config
            .as_ref()
            .and_then(config::no_auto_flush_from_layer)
            .unwrap_or(false)
    }

    fn write_lock_timeout(&self) -> Option<u64> {
        self.config
            .as_ref()
            .and_then(config::lock_timeout_from_layer)
            .or(Some(beads_rust::sync::default_write_lock_timeout_ms()))
    }
}

fn open_storage_from_ctx(ctx: &mut StartupContext) -> Result<config::OpenStorageResult> {
    let startup = ctx.startup.take().ok_or(BeadsError::NotInitialized)?;
    config::open_storage_with_startup_config(startup, &ctx.overrides, false)
}

fn resolve_auto_import_expected_prefix(
    storage_result: &config::OpenStorageResult,
    cli: &config::CliOverrides,
) -> Result<String> {
    let layer = storage_result.load_config(cli)?;
    Ok(config::id_config_from_layer(&layer).prefix)
}

fn execute_create_command(
    args: &beads_rust::cli::CreateArgs,
    overrides: &config::CliOverrides,
    output_ctx: &OutputContext,
    storage_result: &mut Option<config::OpenStorageResult>,
) -> Result<()> {
    commands::create::execute_with_storage(args, overrides, output_ctx, storage_result.take())
}

const fn should_preopen_storage(
    storage_enabled: bool,
    needs_preopened_storage_context: bool,
) -> bool {
    storage_enabled && needs_preopened_storage_context
}

const fn should_acquire_startup_write_lock(
    command_needs_write_lock: bool,
    should_preopen_storage: bool,
) -> bool {
    command_needs_write_lock || should_preopen_storage
}

/// Determine if a command potentially mutates data and triggers auto-flush.
const fn is_mutating_command(cmd: &Commands) -> bool {
    match cmd {
        Commands::Create(_)
        | Commands::Update(_)
        | Commands::Delete(_)
        | Commands::Close(_)
        | Commands::Reopen(_)
        | Commands::Q(_)
        | Commands::Defer(_)
        | Commands::Undefer(_) => true,
        Commands::Dep { command } => matches!(
            command,
            beads_rust::cli::DepCommands::Add(_) | beads_rust::cli::DepCommands::Remove(_)
        ),
        Commands::Label { command } => matches!(
            command,
            beads_rust::cli::LabelCommands::Add(_)
                | beads_rust::cli::LabelCommands::Remove(_)
                | beads_rust::cli::LabelCommands::Rename(_)
        ),
        Commands::Comments(args) => matches!(
            args.command.as_ref(),
            Some(beads_rust::cli::CommentCommands::Add(_))
        ),
        Commands::Epic { command } => matches!(
            command,
            beads_rust::cli::EpicCommands::CloseEligible(args) if !args.dry_run
        ),
        _ => false,
    }
}

/// Determine if a command must hold `.write.lock` for its whole execution.
const fn needs_write_lock(cmd: &Commands) -> bool {
    if is_mutating_command(cmd) {
        return true;
    }
    match cmd {
        // Every sync mode must open storage inside `sync::execute`.
        // `--flush-only` looks like a "just rewrite JSONL" path but also calls
        // `finalize_export` inside a `with_write_transaction`, updating dirty
        // flags, export hashes, and metadata (jsonl_content_hash,
        // last_export_time, needs_flush). Without the `.write.lock`, a
        // concurrent `br sync --flush-only` racing with another process's
        // auto-flush (or a second `--flush-only`) can trip fsqlite's
        // concurrent-write deadlock that this lock was specifically added
        // to prevent (issue #243). `--status` only renders status after open,
        // but opening storage can still apply schema/runtime defaults or
        // recover the DB family, so it must also serialize before open.
        // Doctor inspects a live SQLite DB family via snapshot copy + rollback
        // write probe, so it must serialize with writers — merged into this arm
        // (identical body as Sync/Init) to satisfy clippy::match_same_arms.
        Commands::Sync(_) | Commands::Init { .. } | Commands::Doctor(_) => true,
        Commands::Config { command } => matches!(
            command,
            beads_rust::cli::ConfigCommands::Set { .. }
                | beads_rust::cli::ConfigCommands::Delete { .. }
        ),
        Commands::History(args) => matches!(
            args.command,
            Some(
                beads_rust::cli::HistoryCommands::Restore { .. }
                    | beads_rust::cli::HistoryCommands::Prune { .. }
            )
        ),
        _ => false,
    }
}

const fn should_auto_import(cmd: &Commands) -> bool {
    match cmd {
        Commands::List(_)
        | Commands::Show(_)
        | Commands::Search(_)
        | Commands::Ready(_)
        | Commands::Blocked(_)
        | Commands::Count(_)
        | Commands::Stale(_)
        | Commands::Lint(_)
        | Commands::Stats(_)
        | Commands::Status(_)
        | Commands::Changelog(_)
        | Commands::Graph(_)
        | Commands::Create(_)
        | Commands::Update(_)
        | Commands::Delete(_)
        | Commands::Close(_)
        | Commands::Reopen(_)
        | Commands::Q(_)
        | Commands::Defer(_)
        | Commands::Undefer(_)
        | Commands::Comments(_)
        | Commands::Dep { .. }
        | Commands::Label { .. }
        | Commands::Epic { .. }
        | Commands::Query { .. } => true,

        Commands::Init { .. }
        | Commands::Sync(_)
        | Commands::Doctor(_)
        | Commands::Info(_)
        | Commands::Schema(_)
        | Commands::Where
        | Commands::Version(_)
        | Commands::Completions(_)
        | Commands::Audit { .. }
        | Commands::Config { .. }
        | Commands::History(_)
        | Commands::Orphans(_)
        | Commands::Agents(_) => false,

        #[cfg(feature = "mcp")]
        Commands::Serve(_) => false,

        #[cfg(feature = "self_update")]
        Commands::Upgrade(_) => false,
    }
}

const fn supports_read_only_fast_open(cmd: &Commands) -> bool {
    match cmd {
        Commands::Stats(args) | Commands::Status(args) => args.no_activity,
        Commands::List(_)
        | Commands::Show(_)
        | Commands::Search(_)
        | Commands::Ready(_)
        | Commands::Blocked(_)
        | Commands::Count(_)
        | Commands::Stale(_)
        | Commands::Comments(beads_rust::cli::CommentsArgs {
            command: None | Some(beads_rust::cli::CommentCommands::List(_)),
            ..
        }) => true,
        _ => false,
    }
}

const fn supports_auto_import_read_only_probe(cmd: &Commands) -> bool {
    match cmd {
        Commands::List(_) => true,
        Commands::Stats(args) | Commands::Status(args) => args.no_activity,
        _ => false,
    }
}

fn command_requested_output_format(cmd: &Commands) -> Option<OutputFormat> {
    match cmd {
        Commands::List(args) => args.format,
        Commands::Search(args) => args.filters.format,
        Commands::Show(args) => args.format.map(Into::into),
        Commands::Ready(args) => args.format.map(Into::into),
        Commands::Blocked(args) => args.format.map(Into::into),
        Commands::Stats(args) | Commands::Status(args) => args.format.map(Into::into),
        Commands::Schema(args) => args.format.map(Into::into),
        Commands::Dep { command } => match command {
            beads_rust::cli::DepCommands::List(args) => args.format.map(Into::into),
            beads_rust::cli::DepCommands::Tree(_)
            | beads_rust::cli::DepCommands::Add(_)
            | beads_rust::cli::DepCommands::Remove(_)
            | beads_rust::cli::DepCommands::Cycles(_) => None,
        },
        Commands::Query { command } => match command {
            beads_rust::cli::QueryCommands::Run(args) => args.filters.format,
            beads_rust::cli::QueryCommands::Save(_)
            | beads_rust::cli::QueryCommands::List
            | beads_rust::cli::QueryCommands::Delete(_) => None,
        },
        _ => None,
    }
}

fn should_render_errors_as_json_with_env(
    cli: &Cli,
    env_output_format: Option<OutputFormat>,
) -> bool {
    cli.json
        || command_requests_robot_json(&cli.command)
        || matches!(
            command_requested_output_format(&cli.command).or(env_output_format),
            Some(OutputFormat::Json | OutputFormat::Toon)
        )
}

fn should_render_errors_as_json(cli: &Cli) -> bool {
    should_render_errors_as_json_with_env(cli, OutputFormat::from_env())
}

const fn should_color_human_errors(
    no_color_flag: bool,
    no_color_env_present: bool,
    stderr_is_terminal: bool,
) -> bool {
    !no_color_flag && !no_color_env_present && stderr_is_terminal
}

fn should_color_human_errors_for_cli(cli: &Cli) -> bool {
    should_color_human_errors(
        cli.no_color,
        std::env::var_os("NO_COLOR").is_some(),
        io::stderr().is_terminal(),
    )
}

/// Handle errors with structured output support.
fn handle_error(err: &BeadsError, json_mode: bool, color_mode: bool) -> ! {
    let structured = StructuredError::from_error(err);
    let exit_code = structured.code.exit_code();

    if json_mode {
        let json = structured.to_json();
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string())
        );
    } else {
        eprintln!("{}", structured.to_human(color_mode));
    }

    std::process::exit(exit_code);
}

fn build_cli_overrides(cli: &Cli) -> config::CliOverrides {
    config::CliOverrides {
        db: cli.db.clone(),
        actor: cli.actor.clone(),
        identity: None,
        // Only set bool overrides when the CLI flag was explicitly provided.
        // Eagerly setting Some(false) would override config-file values with the
        // CLI default, preventing users from setting these via config.
        json: cli.json.then_some(true),
        display_color: if cli.no_color { Some(false) } else { None },
        quiet: cli.quiet.then_some(true),
        allow_stale: if cli.allow_stale { Some(true) } else { None },
        no_db: if cli.no_db { Some(true) } else { None },
        no_daemon: if cli.no_daemon { Some(true) } else { None },
        no_auto_flush: if cli.no_auto_flush { Some(true) } else { None },
        no_auto_import: if cli.no_auto_import { Some(true) } else { None },
        lock_timeout: cli.lock_timeout,
        read_only_fast_open: !cli.no_db
            && supports_read_only_fast_open(&cli.command)
            && ((cli.no_auto_import && cli.no_auto_flush)
                || supports_auto_import_read_only_probe(&cli.command)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::fs;
    use tempfile::TempDir;

    fn make_create_args() -> beads_rust::cli::CreateArgs {
        beads_rust::cli::CreateArgs {
            title: Some("test-title".to_string()),
            title_flag: None,
            type_: None,
            priority: None,
            description: None,
            assignee: None,
            owner: None,
            labels: Vec::new(),
            parent: None,
            deps: Vec::new(),
            estimate: None,
            due: None,
            defer: None,
            external_ref: None,
            status: None,
            ephemeral: false,
            dry_run: false,
            silent: false,
            file: None,
        }
    }

    #[test]
    fn parse_global_flags_and_command() {
        let cli = Cli::parse_from(["br", "--json", "-vv", "list"]);
        assert!(cli.json);
        assert_eq!(cli.verbose, 2);
        assert!(!cli.quiet);
        assert!(matches!(cli.command, Commands::List(_)));
    }

    #[test]
    fn parse_create_title_positional() {
        let cli = Cli::parse_from(["br", "create", "FixBug"]);
        match cli.command {
            Commands::Create(args) => {
                assert_eq!(args.title.as_deref(), Some("FixBug"));
            }
            other => unreachable!("expected create command, got {other:?}"),
        }
    }

    #[test]
    fn human_error_color_respects_no_color_precedence() {
        assert!(
            should_color_human_errors(false, false, true),
            "interactive stderr should use color when no color controls are set"
        );
        assert!(
            !should_color_human_errors(true, false, true),
            "--no-color must suppress ANSI error output even on a TTY"
        );
        assert!(
            !should_color_human_errors(false, true, true),
            "NO_COLOR must suppress ANSI error output even on a TTY"
        );
        assert!(
            !should_color_human_errors(false, false, false),
            "non-terminal stderr should not receive ANSI error output"
        );
    }

    #[test]
    fn build_overrides_maps_flags() {
        let cli = Cli::parse_from([
            "br",
            "--json",
            "--no-color",
            "--allow-stale",
            "--no-db",
            "--no-auto-flush",
            "--lock-timeout",
            "2500",
            "list",
        ]);
        let overrides = build_cli_overrides(&cli);
        assert_eq!(overrides.json, Some(true));
        assert_eq!(overrides.display_color, Some(false));
        assert_eq!(overrides.allow_stale, Some(true));
        assert_eq!(overrides.no_db, Some(true));
        assert_eq!(overrides.no_auto_flush, Some(true));
        assert_eq!(overrides.lock_timeout, Some(2500));
    }

    #[test]
    fn build_overrides_omits_absent_startup_bool_flags() {
        let cli = Cli::parse_from(["br", "list"]);
        let overrides = build_cli_overrides(&cli);

        // Absent CLI bool flags must not produce Some(false) overrides — that
        // would silently clobber any config-file value (e.g. `sync.auto_flush:
        // false` would be ignored because the CLI's default `false` wins).
        assert_eq!(overrides.json, None);
        assert_eq!(overrides.quiet, None);
        assert_eq!(overrides.no_db, None);
        assert_eq!(overrides.no_daemon, None);
        assert_eq!(overrides.no_auto_flush, None);
        assert_eq!(overrides.no_auto_import, None);
        assert_eq!(overrides.allow_stale, None);
    }

    #[test]
    fn read_only_fast_open_supports_explicit_suppression_and_safe_list_probe() {
        let list = Cli::parse_from(["br", "list"]);
        assert!(build_cli_overrides(&list).read_only_fast_open);

        let stats = Cli::parse_from(["br", "stats"]);
        assert!(!build_cli_overrides(&stats).read_only_fast_open);

        let stats_no_activity = Cli::parse_from(["br", "stats", "--no-activity"]);
        assert!(build_cli_overrides(&stats_no_activity).read_only_fast_open);

        let status = Cli::parse_from(["br", "status"]);
        assert!(!build_cli_overrides(&status).read_only_fast_open);

        let status_no_activity = Cli::parse_from(["br", "status", "--no-activity"]);
        assert!(build_cli_overrides(&status_no_activity).read_only_fast_open);

        let ready = Cli::parse_from(["br", "--no-auto-import", "--no-auto-flush", "ready"]);
        assert!(build_cli_overrides(&ready).read_only_fast_open);

        let comments_list = Cli::parse_from([
            "br",
            "--no-auto-import",
            "--no-auto-flush",
            "comments",
            "list",
            "bd-abc",
        ]);
        assert!(build_cli_overrides(&comments_list).read_only_fast_open);

        let comments_shorthand = Cli::parse_from([
            "br",
            "--no-auto-import",
            "--no-auto-flush",
            "comments",
            "bd-abc",
        ]);
        assert!(build_cli_overrides(&comments_shorthand).read_only_fast_open);

        let missing_auto_flush = Cli::parse_from(["br", "--no-auto-import", "ready"]);
        assert!(!build_cli_overrides(&missing_auto_flush).read_only_fast_open);

        let mutating = Cli::parse_from([
            "br",
            "--no-auto-import",
            "--no-auto-flush",
            "create",
            "write path",
        ]);
        assert!(!build_cli_overrides(&mutating).read_only_fast_open);

        let comments_add = Cli::parse_from([
            "br",
            "--no-auto-import",
            "--no-auto-flush",
            "comments",
            "add",
            "bd-abc",
            "write path",
        ]);
        assert!(!build_cli_overrides(&comments_add).read_only_fast_open);
    }

    #[test]
    fn help_includes_core_commands() {
        let help = Cli::command().render_help().to_string();
        assert!(help.contains("create"));
        assert!(help.contains("list"));
        assert!(help.contains("sync"));
        assert!(help.contains("ready"));
    }

    #[test]
    fn version_includes_name_and_version() {
        let version = Cli::command().render_version();
        assert!(version.contains("br"));
        assert!(version.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn is_mutating_command_detects_mutations() {
        let create_cmd = Commands::Create(make_create_args());
        let list_cmd = Commands::List(beads_rust::cli::ListArgs::default());
        assert!(is_mutating_command(&create_cmd));
        assert!(!is_mutating_command(&list_cmd));
    }

    #[test]
    fn is_mutating_command_distinguishes_read_only_subcommands() {
        let dep_list = Cli::parse_from(["br", "dep", "list", "bd-123"]).command;
        let dep_add = Cli::parse_from(["br", "dep", "add", "bd-123", "bd-456"]).command;
        let label_list = Cli::parse_from(["br", "label", "list"]).command;
        let label_add = Cli::parse_from(["br", "label", "add", "bd-123", "--label", "ops"]).command;
        let comments_list = Cli::parse_from(["br", "comments", "bd-123"]).command;
        let comments_add = Cli::parse_from(["br", "comments", "add", "bd-123", "hello"]).command;

        assert!(!is_mutating_command(&dep_list));
        assert!(is_mutating_command(&dep_add));
        assert!(!is_mutating_command(&label_list));
        assert!(is_mutating_command(&label_add));
        assert!(!is_mutating_command(&comments_list));
        assert!(is_mutating_command(&comments_add));
    }

    #[test]
    fn sync_is_not_auto_imported_or_auto_flushed() {
        let sync_cmd = Cli::parse_from(["br", "sync"]).command;
        assert!(!is_mutating_command(&sync_cmd));
        assert!(!should_auto_import(&sync_cmd));
    }

    #[test]
    fn sync_modes_require_write_lock_before_storage_open() {
        // Regression: `br sync --flush-only` calls `finalize_export` inside a
        // `with_write_transaction` (clears dirty flags, updates
        // jsonl_content_hash + last_export_time + needs_flush metadata, writes
        // export hashes). That makes it a write-side operation as far as
        // fsqlite is concerned. Previously the `needs_write_lock` match arm
        // excluded `--flush-only`, leaving two concurrent `br sync
        // --flush-only` invocations — or one racing a mutating command's
        // auto-flush — to hit the fsqlite concurrent-write deadlock that the
        // `.write.lock` was specifically introduced (issue #243) to prevent.
        //
        // `br sync --status` is read-only after storage is open, but the open
        // path can apply runtime metadata defaults, recover from JSONL, or move
        // sidecars. It must therefore serialize before entering `sync::execute`.
        let flush_only = Cli::parse_from(["br", "sync", "--flush-only"]).command;
        let status = Cli::parse_from(["br", "sync", "--status"]).command;
        let merge = Cli::parse_from(["br", "sync", "--merge"]).command;
        let import_only = Cli::parse_from(["br", "sync", "--import-only"]).command;
        let default_sync = Cli::parse_from(["br", "sync"]).command;

        assert!(
            needs_write_lock(&flush_only),
            "`br sync --flush-only` writes DB metadata and must serialize via .write.lock"
        );
        assert!(
            needs_write_lock(&status),
            "`br sync --status` opens storage and must serialize before recovery/schema work"
        );
        assert!(needs_write_lock(&merge));
        assert!(needs_write_lock(&import_only));
        assert!(needs_write_lock(&default_sync));
    }

    #[test]
    fn doctor_requires_write_lock_before_live_inspection() {
        let inspect = Cli::parse_from(["br", "doctor"]).command;
        let repair = Cli::parse_from(["br", "doctor", "--repair"]).command;

        assert!(
            needs_write_lock(&inspect),
            "`br doctor` copies/probes the live DB family and must serialize via .write.lock"
        );
        assert!(needs_write_lock(&repair));
    }

    #[test]
    fn diagnostic_and_config_commands_skip_auto_import() {
        let cases: &[&[&str]] = &[
            &["br", "doctor"],
            &["br", "where"],
            &["br", "schema"],
            &["br", "config", "path"],
            &["br", "history", "list"],
        ];

        for argv in cases {
            let command = Cli::parse_from(*argv).command;
            assert!(
                !should_auto_import(&command),
                "command should not auto-import: {command:?}"
            );
        }
    }

    #[test]
    fn orphans_handles_freshness_without_global_auto_import() {
        let command = Cli::parse_from(["br", "orphans"]).command;
        assert!(!should_auto_import(&command));
    }

    #[test]
    fn auto_import_expected_prefix_uses_merged_config_layers() {
        let temp = TempDir::new().expect("tempdir");
        let beads_dir = temp.path().join(".beads");
        fs::create_dir_all(&beads_dir).expect("create beads dir");
        fs::write(
            beads_dir.join("config.yaml"),
            "issue_prefix: document-intelligence\n",
        )
        .expect("write config");

        let mut storage_result =
            config::open_storage_with_cli(&beads_dir, &config::CliOverrides::default())
                .expect("open storage");
        storage_result
            .storage
            .set_config("issue_prefix", "db-prefix")
            .expect("set db prefix");

        let prefix =
            resolve_auto_import_expected_prefix(&storage_result, &config::CliOverrides::default())
                .expect("resolve prefix");

        assert_eq!(prefix, "document-intelligence");
    }

    #[test]
    fn preopened_storage_reuses_startup_paths() {
        let temp = TempDir::new().expect("tempdir");
        let beads_dir = temp.path().join(".beads");
        fs::create_dir_all(&beads_dir).expect("create beads dir");

        let first_jsonl = beads_dir.join("first.jsonl");
        let second_jsonl = beads_dir.join("second.jsonl");
        let metadata_path = beads_dir.join("metadata.json");
        fs::write(
            &metadata_path,
            r#"{"database":"beads.db","jsonl_export":"first.jsonl"}"#,
        )
        .expect("write initial metadata");

        let overrides = config::CliOverrides {
            db: Some(beads_dir.join("beads.db")),
            no_db: Some(true),
            ..config::CliOverrides::default()
        };
        let mut ctx = StartupContext::init(&overrides).expect("startup context");

        fs::write(
            &metadata_path,
            r#"{"database":"beads.db","jsonl_export":"second.jsonl"}"#,
        )
        .expect("rewrite metadata");

        let storage_ctx = open_storage_from_ctx(&mut ctx).expect("preopened storage");

        assert_eq!(storage_ctx.paths.jsonl_path, first_jsonl);
        assert_ne!(storage_ctx.paths.jsonl_path, second_jsonl);
    }

    #[test]
    fn create_dispatch_reuses_preopened_storage_context() {
        let temp = TempDir::new().expect("tempdir");
        let beads_dir = temp.path().join(".beads");
        fs::create_dir_all(&beads_dir).expect("create beads dir");

        let first_db = beads_dir.join("first.db");
        let second_db = beads_dir.join("second.db");
        let metadata_path = beads_dir.join("metadata.json");
        fs::write(
            &metadata_path,
            format!(
                r#"{{"database":"{}","jsonl_export":"issues.jsonl"}}"#,
                first_db.display()
            ),
        )
        .expect("write initial metadata");

        let overrides = config::CliOverrides::default();
        let startup =
            config::load_startup_config_with_paths(&beads_dir, None).expect("startup context");

        fs::write(
            &metadata_path,
            format!(
                r#"{{"database":"{}","jsonl_export":"issues.jsonl"}}"#,
                second_db.display()
            ),
        )
        .expect("rewrite metadata");

        let cli = Cli::parse_from(["br", "--json", "create", "Use preopened storage"]);
        let output_ctx = OutputContext::from_args(&cli);
        let Commands::Create(args) = cli.command else {
            unreachable!("expected create command");
        };
        let mut storage_result = Some(
            config::open_storage_with_startup_config(startup, &overrides, false)
                .expect("preopened storage"),
        );

        execute_create_command(&args, &overrides, &output_ctx, &mut storage_result)
            .expect("create should use preopened storage");

        assert!(storage_result.is_none());

        let first_storage =
            beads_rust::storage::SqliteStorage::open(&first_db).expect("open first db");
        assert_eq!(first_storage.count_issues().expect("count first db"), 1);
        assert!(
            !second_db.exists(),
            "create dispatch reopened storage from rewritten metadata instead of using preopened context"
        );
    }

    #[test]
    fn should_render_errors_as_json_when_command_requests_json_format() {
        let cli = Cli::parse_from(["br", "list", "--format", "json"]);
        assert!(should_render_errors_as_json_with_env(&cli, None));
    }

    #[test]
    fn should_render_errors_as_json_for_query_run_json_format() {
        let cli = Cli::parse_from(["br", "query", "run", "saved", "--format", "json"]);
        assert!(should_render_errors_as_json_with_env(&cli, None));
    }

    #[test]
    fn should_render_errors_as_json_when_command_requests_toon_format() {
        let cli = Cli::parse_from(["br", "list", "--format", "toon"]);
        assert!(should_render_errors_as_json_with_env(&cli, None));
    }

    #[test]
    fn should_render_errors_as_json_when_env_requests_json_format() {
        let cli = Cli::parse_from(["br", "history", "list"]);
        assert!(should_render_errors_as_json_with_env(
            &cli,
            Some(OutputFormat::Json)
        ));
    }

    #[test]
    fn should_render_errors_as_json_when_env_requests_toon_format() {
        let cli = Cli::parse_from(["br", "history", "list"]);
        assert!(should_render_errors_as_json_with_env(
            &cli,
            Some(OutputFormat::Toon)
        ));
    }

    #[test]
    fn should_not_render_errors_as_json_without_json_request() {
        let cli = Cli::parse_from(["br", "history", "list"]);
        assert!(!should_render_errors_as_json_with_env(&cli, None));
    }

    #[test]
    fn preopen_storage_skips_commands_without_bootstrap_or_flush_work() {
        assert!(!should_preopen_storage(true, false));
    }

    #[test]
    fn preopen_storage_keeps_mutating_auto_flush_path() {
        assert!(should_preopen_storage(true, true));
    }

    #[test]
    fn preopen_storage_keeps_bootstrap_path_for_staleness_checks() {
        assert!(should_preopen_storage(true, true));
    }

    #[test]
    fn preopen_storage_requires_write_lock_before_open() {
        assert!(should_acquire_startup_write_lock(false, true));
        assert!(should_acquire_startup_write_lock(true, false));
        assert!(should_acquire_startup_write_lock(true, true));
        assert!(!should_acquire_startup_write_lock(false, false));
    }
}
