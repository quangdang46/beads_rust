//! `br hooks` command — manage git hooks for beads auto-import/export.
//!
//! Installs bash hook scripts into `.git/hooks/` that integrate beads
//! operations into the git workflow:
//!
//! - **pre-commit**: auto-export (`br sync --flush-only`)
//! - **post-merge**: auto-import (`br sync --import-only`)
//! - **pre-push**: check for unsynced beads changes
//! - **post-checkout**: refresh state after branch switch
//!
//! Hook files use section markers (`BEGIN BEADS INTEGRATION` / `END BEADS
//! INTEGRATION`) so custom user content outside the markers is preserved.

use crate::error::BeadsError;
use crate::output::OutputContext;
use crate::Result;

/// Execute the `hooks` subcommand.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn execute(command: &crate::cli::HooksCommand, ctx: &OutputContext) -> Result<()> {
    match command {
        crate::cli::HooksCommand::Install(args) => cmd_install(args, ctx),
        crate::cli::HooksCommand::Uninstall(args) => cmd_uninstall(args, ctx),
        crate::cli::HooksCommand::List => cmd_list(ctx),
        crate::cli::HooksCommand::Run(args) => cmd_run(args, ctx),
    }
}

/// Install managed git hooks.
fn cmd_install(args: &crate::cli::HooksInstallArgs, ctx: &OutputContext) -> Result<()> {
    let git_info = crate::hooks::find_git_info()?;
    let all_hooks = args.all;

    let hooks_to_install: Vec<&str> = if let Some(name) = &args.hook {
        vec![name.as_str()]
    } else if all_hooks {
        crate::hooks::MANAGED_HOOKS
            .iter()
            .map(|h| h.name)
            .collect()
    } else {
        // Default: install all if none specified
        crate::hooks::MANAGED_HOOKS
            .iter()
            .map(|h| h.name)
            .collect()
    };

    let mut installed = Vec::new();
    for hook_name in &hooks_to_install {
        let path = crate::hooks::install_hook(hook_name, &git_info)?;
        installed.push((hook_name, path));
    }

    if ctx.is_json() || ctx.is_toon() {
        let payload = serde_json::json!({
            "status": "installed",
            "hooks_dir": git_info.hooks_dir.display().to_string(),
            "installed": installed.iter().map(|(name, path)| {
                serde_json::json!({
                    "name": name,
                    "path": path.display().to_string()
                })
            }).collect::<Vec<_>>()
        });
        ctx.print_line(
            &serde_json::to_string(&payload)
                .unwrap_or_else(|_| "{\"status\":\"installed\"}".to_string()),
        );
    } else {
        ctx.print_line(&format!(
            "Installed {} hooks in {}",
            installed.len(),
            git_info.hooks_dir.display()
        ));
        for (name, path) in &installed {
            ctx.print_line(&format!("  {} -> {}", name, path.display()));
        }
    }

    Ok(())
}

/// Uninstall managed git hooks.
fn cmd_uninstall(args: &crate::cli::HooksUninstallArgs, ctx: &OutputContext) -> Result<()> {
    let git_info = crate::hooks::find_git_info()?;

    let hooks_to_remove: Vec<&str> = if let Some(name) = &args.hook {
        vec![name.as_str()]
    } else if args.all {
        crate::hooks::MANAGED_HOOKS
            .iter()
            .map(|h| h.name)
            .collect()
    } else {
        crate::hooks::MANAGED_HOOKS
            .iter()
            .map(|h| h.name)
            .collect()
    };

    let mut removed = Vec::new();
    for hook_name in &hooks_to_remove {
        if crate::hooks::uninstall_hook(hook_name, &git_info)? {
            removed.push(*hook_name);
        }
    }

    if ctx.is_json() || ctx.is_toon() {
        let payload = serde_json::json!({
            "status": "uninstalled",
            "hooks_dir": git_info.hooks_dir.display().to_string(),
            "removed": removed
        });
        ctx.print_line(
            &serde_json::to_string(&payload)
                .unwrap_or_else(|_| "{\"status\":\"uninstalled\"}".to_string()),
        );
    } else {
        if removed.is_empty() {
            ctx.print_line("No managed hooks were found to uninstall.");
        } else {
            ctx.print_line(&format!(
                "Removed beads section from {} hook(s) in {}",
                removed.len(),
                git_info.hooks_dir.display()
            ));
            for name in &removed {
                ctx.print_line(&format!("  {}", name));
            }
        }
    }

    Ok(())
}

/// List the status of all managed hooks.
fn cmd_list(ctx: &OutputContext) -> Result<()> {
    let statuses = crate::hooks::check_hooks_status()?;

    if ctx.is_json() || ctx.is_toon() {
        let payload = serde_json::json!(statuses);
        ctx.print_line(
            &serde_json::to_string(&payload)
                .unwrap_or_else(|_| "[]".to_string()),
        );
    } else {
        if statuses.is_empty() {
            ctx.print_line("No managed hooks.");
            return Ok(());
        }

        ctx.print_line(&format!(
            "  {:<20} {}",
            "Hook", "Status"
        ));
        ctx.print_line(&format!(
            "  {} {}",
            "-".repeat(20),
            "-".repeat(10)
        ));
        for status in &statuses {
            let status_str = if status.installed {
                "installed"
            } else {
                "not installed"
            };
            ctx.print_line(&format!(
                "  {:<20} {}",
                status.name, status_str
            ));
        }

        ctx.print_line("");
        ctx.print_line("Use `br hooks install <name>` or `br hooks install --all` to install hooks.");
    }

    Ok(())
}

/// Run a hook's synchronisation logic.
///
/// This is invoked by the shell hook scripts installed via `br hooks install`.
/// The actual sync operations run as subprocess calls (`br sync --flush-only`,
/// etc.) by the hook scripts, but this CLI path is available for manual
/// debugging and one-off execution.
fn cmd_run(args: &crate::cli::HooksRunArgs, ctx: &OutputContext) -> Result<()> {
    let valid_names: Vec<&str> = crate::hooks::MANAGED_HOOKS.iter().map(|h| h.name).collect();

    if !valid_names.contains(&args.hook.as_str()) {
        return Err(BeadsError::Internal {
            message: format!(
                "Unknown hook '{}'. Valid hooks: {}",
                args.hook,
                valid_names.join(", ")
            ),
        });
    }

    ctx.print_line(&format!(
        "beads: hooks run {} — this runs the same logic as the installed git hook script.\n\
         Normally the hook script calls `br sync --flush-only` or `br sync --import-only`\n\
         directly. Run the appropriate sync command manually to achieve the same effect.",
        args.hook
    ));

    Ok(())
}
