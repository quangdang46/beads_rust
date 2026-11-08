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

use crate::Result;
use crate::error::BeadsError;
use crate::output::OutputContext;

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

    if let Some(name) = &args.hook {
        validate_managed_hook_name(name)?;
    }

    let hooks_to_install: Vec<&str> = if let Some(name) = &args.hook {
        vec![name.as_str()]
    } else if all_hooks {
        crate::hooks::MANAGED_HOOKS.iter().map(|h| h.name).collect()
    } else {
        // Default: install all if none specified
        crate::hooks::MANAGED_HOOKS.iter().map(|h| h.name).collect()
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

    if let Some(name) = &args.hook {
        validate_managed_hook_name(name)?;
    }

    let hooks_to_remove: Vec<&str> = if let Some(name) = &args.hook {
        vec![name.as_str()]
    } else if args.all {
        crate::hooks::MANAGED_HOOKS.iter().map(|h| h.name).collect()
    } else {
        crate::hooks::MANAGED_HOOKS.iter().map(|h| h.name).collect()
    };

    let mut removed: Vec<String> = Vec::new();
    for hook_name in &hooks_to_remove {
        if crate::hooks::uninstall_hook(hook_name, &git_info)? {
            removed.push((*hook_name).to_string());
        }
    }

    // `uninstall --all` must also strip beads markers from hook files whose
    // names are not managed hook names (e.g. a user-renamed or custom hook
    // that still carries the BEADS INTEGRATION section).
    if args.all {
        removed.extend(uninstall_noncanonical_hook_sections(&git_info)?);
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
        ctx.print_line(&serde_json::to_string(&payload).unwrap_or_else(|_| "[]".to_string()));
    } else {
        if statuses.is_empty() {
            ctx.print_line("No managed hooks.");
            return Ok(());
        }

        ctx.print_line(&format!("  {:<20} {}", "Hook", "Status"));
        ctx.print_line(&format!("  {} {}", "-".repeat(20), "-".repeat(10)));
        for status in &statuses {
            let status_str = if status.installed {
                "installed"
            } else {
                "not installed"
            };
            ctx.print_line(&format!("  {:<20} {}", status.name, status_str));
        }

        ctx.print_line("");
        ctx.print_line(
            "Use `br hooks install <name>` or `br hooks install --all` to install hooks.",
        );
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
    validate_managed_hook_name(&args.hook)?;

    // post-merge / post-checkout run `br sync --import-only`, which writes to
    // the database. `hooks::run_hook` opens storage read-only (fast path), so
    // the import fails on a read-only handle — run those two with a normal
    // writable open instead.
    match args.hook.as_str() {
        "post-merge" | "post-checkout" => run_import_hook(),
        // prepare-commit-msg is invoked by git with the commit-msg file path;
        // there is no standalone sync operation to run for it.
        "prepare-commit-msg" => {
            ctx.print_line(
                "prepare-commit-msg is editor-invoked by git; nothing to run standalone.",
            );
            Ok(())
        }
        _ => crate::hooks::run_hook(&args.hook),
    }
}

/// Validate that `name` is one of the managed git hook names.
fn validate_managed_hook_name(name: &str) -> Result<()> {
    let valid: Vec<&str> = crate::hooks::MANAGED_HOOKS.iter().map(|h| h.name).collect();
    if valid.contains(&name) {
        Ok(())
    } else {
        Err(BeadsError::validation(
            "hook",
            format!("unknown hook '{name}'. Valid hooks: {}", valid.join(", ")),
        ))
    }
}

/// Marker lines emitted by `br hooks install` (see `src/hooks/mod.rs`).
const BEADS_BEGIN_MARKER: &str = "# --- BEGIN BEADS INTEGRATION";
const BEADS_END_MARKER: &str = "# --- END BEADS INTEGRATION";

/// Strip the beads section from every file in the hooks directory whose name
/// is NOT a managed hook name but which still contains beads markers.
///
/// Returns the file names that were modified (or removed).
fn uninstall_noncanonical_hook_sections(git_info: &crate::hooks::GitInfo) -> Result<Vec<String>> {
    let managed: std::collections::HashSet<&str> =
        crate::hooks::MANAGED_HOOKS.iter().map(|h| h.name).collect();

    let mut cleaned = Vec::new();
    let entries = match std::fs::read_dir(&git_info.hooks_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(cleaned),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if managed.contains(file_name.as_str()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !content.contains(BEADS_BEGIN_MARKER) && !content.contains(BEADS_END_MARKER) {
            continue;
        }
        let (new_content, found) = strip_beads_section(&content);
        if !found {
            continue;
        }
        if is_only_shebang_or_empty(&new_content) {
            std::fs::remove_file(&path)?;
        } else {
            std::fs::write(&path, &new_content)?;
        }
        cleaned.push(file_name);
    }

    Ok(cleaned)
}

/// Remove the beads-managed section (BEGIN..END marker lines, inclusive)
/// from hook file content. Orphaned single markers are removed as well.
/// Returns `(new_content, found)`.
fn strip_beads_section(content: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let begin = lines.iter().position(|l| l.contains(BEADS_BEGIN_MARKER));
    let end = lines.iter().position(|l| l.contains(BEADS_END_MARKER));

    let mut keep = vec![true; lines.len()];
    let mut found = false;

    match (begin, end) {
        (Some(b), Some(e)) if b <= e => {
            for i in b..=e {
                keep[i] = false;
            }
            found = true;
        }
        (Some(b), Some(e)) => {
            // Reversed markers — drop both marker lines only.
            keep[b] = false;
            keep[e] = false;
            found = true;
        }
        (Some(b), None) => {
            keep[b] = false;
            found = true;
        }
        (None, Some(e)) => {
            keep[e] = false;
            found = true;
        }
        (None, None) => {}
    }

    if !found {
        return (content.to_string(), false);
    }

    let mut out = String::with_capacity(content.len());
    for (i, line) in lines.iter().enumerate() {
        if keep[i] {
            out.push_str(line);
            out.push('\n');
        }
    }
    // Preserve a trailing-newline-less file shape loosely; trailing blank
    // lines left by removal are harmless in shell hooks.
    (out, true)
}

/// True when nothing meaningful remains (empty or shebang/comments only).
fn is_only_shebang_or_empty(content: &str) -> bool {
    content.lines().all(|line| {
        let t = line.trim();
        t.is_empty() || t.starts_with("#!") || t.starts_with('#')
    })
}

/// Run the post-merge / post-checkout import with a writable storage handle.
///
/// Mirrors `crate::hooks::run_hook`'s import branch but opens storage
/// normally (`read_only_fast_open: false`) so `import_from_jsonl` can write.
fn run_import_hook() -> Result<()> {
    use crate::config;
    use crate::sync::{self, ImportConfig};

    let beads_dir = config::discover_beads_dir(None)?;
    let mut storage_ctx = config::open_storage_with_cli(
        &beads_dir,
        &config::CliOverrides {
            read_only_fast_open: false,
            ..Default::default()
        },
    )?;

    let jsonl_path = beads_dir.join("beads.jsonl");
    if jsonl_path.is_file() {
        let import_config = ImportConfig {
            skip_prefix_validation: false,
            rename_on_import: false,
            clear_duplicate_external_refs: false,
            orphan_mode: sync::OrphanMode::Strict,
            force_upsert: false,
            beads_dir: Some(beads_dir),
            allow_external_jsonl: false,
            show_progress: false,
            strict: false,
        };
        sync::import_from_jsonl(&mut storage_ctx.storage, &jsonl_path, &import_config, None)?;
    }

    Ok(())
}
