//! Git hooks system for beads auto-import/export.
//!
//! Manages git hooks (pre-commit, post-merge, pre-push, post-checkout) that
//! integrate beads operations into the git workflow:
//!
//! - **pre-commit**: auto-export (`br sync --flush-only`)
//! - **post-merge**: auto-import (`br sync --import-only`)
//! - **pre-push**: check for unsynced beads changes
//! - **post-checkout**: refresh state after branch switch
//!
//! Hook files use section markers (`BEGIN BEADS INTEGRATION` / `END BEADS
//! INTEGRATION`) so user modifications outside the markers are preserved
//! across install/uninstall.

use crate::error::{BeadsError, Result};
use serde::Serialize;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Marker prefix for the start of the beads-managed section.
const SECTION_BEGIN_PREFIX: &str = "# --- BEGIN BEADS INTEGRATION";

/// Marker prefix for the end of the beads-managed section.
const SECTION_END_PREFIX: &str = "# --- END BEADS INTEGRATION";

/// Default timeout for hook execution in seconds.
const HOOK_TIMEOUT_SECS: u64 = 120;

/// Managed git hook names and their purposes.
pub const MANAGED_HOOKS: &[HookDef] = &[
    HookDef {
        name: "pre-commit",
        description: "Auto-export to JSONL before committing",
    },
    HookDef {
        name: "post-merge",
        description: "Auto-import from JSONL after pulling",
    },
    HookDef {
        name: "pre-push",
        description: "Verify beads state before pushing",
    },
    HookDef {
        name: "post-checkout",
        description: "Refresh beads state after branch switch",
    },
];

/// Definition of a managed git hook.
#[derive(Debug, Clone, Serialize)]
pub struct HookDef {
    /// File name in `.git/hooks/`.
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
}

/// Status of a managed hook in the repository.
#[derive(Debug, Clone, Serialize)]
pub struct HookStatus {
    /// Hook file name.
    pub name: &'static str,
    /// Whether the hook is installed.
    pub installed: bool,
    /// Human-readable description.
    pub description: &'static str,
}

/// Result of finding the git hooks directory.
#[derive(Debug)]
pub struct GitInfo {
    /// Absolute path to `.git/hooks/`.
    pub hooks_dir: PathBuf,
    /// Absolute path to the repository root.
    pub repo_root: PathBuf,
    /// Whether this is a git worktree.
    pub is_worktree: bool,
}

/// Find the git hooks directory and repository info.
///
/// Runs `git rev-parse --git-dir --git-common-dir --show-toplevel` to
/// discover the repository layout.
///
/// # Errors
///
/// Returns `Internal` error if not in a git repository.
pub fn find_git_info() -> Result<GitInfo> {
    let output = Command::new("git")
        .args([
            "rev-parse",
            "--git-dir",
            "--git-common-dir",
            "--show-toplevel",
        ])
        .output()
        .map_err(|e| BeadsError::Internal {
            message: format!("Failed to run git: {e}"),
        })?;

    if !output.status.success() {
        return Err(BeadsError::Internal {
            message: "Not a git repository (or git is not available)".to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stdout.trim().lines().collect();
    if parts.len() < 3 {
        return Err(BeadsError::Internal {
            message: format!(
                "Unexpected git output: expected 3 lines, got {}",
                parts.len()
            ),
        });
    }

    let git_dir = PathBuf::from(parts[0]);
    let common_dir_str = parts[1];
    let repo_root_str = parts[2];

    let git_dir_abs = if git_dir.is_absolute() {
        git_dir
    } else {
        let root = PathBuf::from(repo_root_str);
        root.join(&git_dir)
    };

    let common_dir = PathBuf::from(common_dir_str);
    let common_dir_abs = if common_dir.is_absolute() {
        common_dir
    } else {
        let root = PathBuf::from(repo_root_str);
        root.join(&common_dir)
    };

    let hooks_dir = common_dir_abs.join("hooks");
    let repo_root = canonicalize_path(Path::new(repo_root_str));
    let is_worktree = git_dir_abs
        .canonicalize()
        .map(|g| g != common_dir_abs)
        .unwrap_or(false);

    Ok(GitInfo {
        hooks_dir,
        repo_root,
        is_worktree,
    })
}

/// Canonicalize a path by resolving symlinks.
fn canonicalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Generate the shell script content for a managed hook.
fn generate_hook_script(hook_name: &str) -> String {
    let br_cmd = match hook_name {
        "pre-commit" => {
            "echo \"beads: pre-commit hook running br sync --flush-only...\"\n\
             br sync --flush-only 2>&1 || true"
        }
        "post-merge" => {
            "echo \"beads: post-merge hook running br sync --import-only...\"\n\
             br sync --import-only 2>&1 || true"
        }
        "pre-push" => {
            "# Check if beads database has unsynced changes\n\
             if br sync --status 2>/dev/null | grep -q \"needs.export\"; then\n\
             \x20 echo \"beads: warning - beads changes not yet exported!\"\n\
             \x20 echo \"beads: run 'br sync --flush-only' before pushing\"\n\
             \x20 br sync --flush-only 2>&1 || true\n\
             fi"
        }
        "post-checkout" => {
            "# Check if the new branch has a different beads state\n\
             br sync --import-only 2>&1 || true"
        }
        _ => "",
    };

    let begin_line = format!("{} v{} ---", SECTION_BEGIN_PREFIX, "1.0");
    let end_line = format!("{} v{} ---", SECTION_END_PREFIX, "1.0");

    format!(
        r#"{begin_line}
# This section is managed by beads. Do not remove these markers.
if command -v br >/dev/null 2>&1; then
  export BR_GIT_HOOK=1
  _br_timeout=${{BR_HOOK_TIMEOUT:-{timeout}}}
  if command -v timeout >/dev/null 2>&1; then
    timeout "$_br_timeout" sh -c '{br_cmd}' < /dev/null
    _br_exit=$?
  else
    {br_cmd}
    _br_exit=$?
  fi
  if [ $_br_exit -eq 124 ] || [ $_br_exit -eq 142 ]; then
    echo >&2 "beads: hook '{hook_name}' timed out after ${{_br_timeout}}s — continuing without beads"
  elif [ $_br_exit -eq 3 ]; then
    echo >&2 "beads: database not initialized — skipping hook '{hook_name}'"
  elif [ $_br_exit -ne 0 ]; then
    echo >&2 "beads: hook '{hook_name}' failed with exit $_br_exit — see above"
  fi
fi
{end_line}
"#,
        begin_line = begin_line,
        end_line = end_line,
        timeout = HOOK_TIMEOUT_SECS,
        hook_name = hook_name,
        br_cmd = br_cmd,
    )
}

/// Check the status of all managed hooks.
///
/// # Errors
///
/// Returns an error if git info cannot be obtained.
pub fn check_hooks_status() -> Result<Vec<HookStatus>> {
    let git_info = find_git_info()?;

    let statuses: Vec<HookStatus> = MANAGED_HOOKS
        .iter()
        .map(|def| {
            let hook_path = git_info.hooks_dir.join(def.name);
            let installed = hook_path.exists() && is_hook_managed(&hook_path);
            HookStatus {
                name: def.name,
                installed,
                description: def.description,
            }
        })
        .collect();

    Ok(statuses)
}

/// Check if a hook file contains the beads section marker.
fn is_hook_managed(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    match std::fs::read_to_string(path) {
        Ok(content) => content.contains(SECTION_BEGIN_PREFIX),
        Err(_) => false,
    }
}

/// Install a managed hook.
///
/// Reads the existing hook file (if any), injects or replaces the beads
/// section, and writes the result. Creates the file if it doesn't exist.
/// Makes the file executable.
///
/// # Errors
///
/// Returns an error if file operations fail.
pub fn install_hook(hook_name: &str, git_info: &GitInfo) -> Result<PathBuf> {
    let hook_path = git_info.hooks_dir.join(hook_name);
    let section = generate_hook_script(hook_name);

    let existing = if hook_path.exists() {
        std::fs::read_to_string(&hook_path)?
    } else {
        String::new()
    };

    let new_content = inject_hook_section(&existing, &section);

    // Ensure hooks directory exists
    if let Some(parent) = hook_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&hook_path, &new_content)?;

    // Make executable
    let mut perms = std::fs::metadata(&hook_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&hook_path, perms)?;

    Ok(hook_path)
}

/// Uninstall a managed hook (remove only the beads section).
///
/// If the file is empty or only contains shebang/comments after removal,
/// the file is deleted entirely.
///
/// # Errors
///
/// Returns an error if file operations fail.
pub fn uninstall_hook(hook_name: &str, git_info: &GitInfo) -> Result<bool> {
    let hook_path = git_info.hooks_dir.join(hook_name);

    if !hook_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&hook_path)?;
    let (new_content, found) = remove_hook_section(&content);

    if !found {
        return Ok(false);
    }

    // If nothing meaningful remains (or only shebang), delete the file
    if is_only_shebang_or_empty(&new_content) {
        std::fs::remove_file(&hook_path)?;
    } else {
        std::fs::write(&hook_path, &new_content)?;
    }

    Ok(true)
}

/// Inject the beads section into existing hook content.
///
/// If section markers exist, only the content between them is replaced.
/// If no markers exist, the section is appended.
fn inject_hook_section(existing: &str, section: &str) -> String {
    let begin_idx = existing.find(SECTION_BEGIN_PREFIX);
    let end_idx = existing.find(SECTION_END_PREFIX);

    match (begin_idx, end_idx) {
        (Some(b), Some(e)) if b < e => {
            // Valid pair — replace between markers
            let line_start = existing[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);

            let end_of_end = e + SECTION_END_PREFIX.len();
            let end_of_line = existing[end_of_end..]
                .find('\n')
                .map(|i| end_of_end + i + 1)
                .unwrap_or(existing.len());

            format!(
                "{}{}{}",
                &existing[..line_start],
                section,
                &existing[end_of_line..]
            )
        }
        (Some(_b), Some(_e)) => {
            // Reversed or broken — remove both markers and content, then append
            let cleaned = remove_broken_markers(existing);
            let trimmed = cleaned.trim_end();
            if trimmed.is_empty() {
                format!("{}\n", section)
            } else {
                format!("{}\n\n{}", trimmed, section)
            }
        }
        (Some(b), None) => {
            // Orphaned BEGIN — remove it and append
            let cleaned = remove_orphan_begin(existing, b);
            let trimmed = cleaned.trim_end();
            if trimmed.is_empty() {
                format!("{}\n", section)
            } else {
                format!("{}\n\n{}", trimmed, section)
            }
        }
        (None, Some(e)) => {
            // Orphaned END — remove it and append
            let cleaned = remove_marker_line(existing, e, SECTION_END_PREFIX);
            let trimmed = cleaned.trim_end();
            if trimmed.is_empty() {
                format!("{}\n", section)
            } else {
                format!("{}\n\n{}", trimmed, section)
            }
        }
        (None, None) => {
            // No markers — append
            let trimmed = existing.trim_end();
            if trimmed.is_empty() {
                format!("{}\n", section)
            } else {
                format!("{}\n\n{}", trimmed, section)
            }
        }
    }
}

/// Remove both orphaned BEGIN and END markers from content.
fn remove_broken_markers(content: &str) -> String {
    let mut result = content.to_string();
    loop {
        let begin_idx = result.find(SECTION_BEGIN_PREFIX);
        let end_idx = result.find(SECTION_END_PREFIX);
        match (begin_idx, end_idx) {
            (Some(b), Some(e)) if b < e => {
                // Valid pair — replace between markers
                let line_start = result[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let end_of_end = e + SECTION_END_PREFIX.len();
                let end_of_line = result[end_of_end..]
                    .find('\n')
                    .map(|i| end_of_end + i + 1)
                    .unwrap_or(result.len());
                result = format!(
                    "{}{}",
                    &result[..line_start],
                    &result[end_of_line..]
                );
            }
            (Some(b), _) => {
                result = remove_orphan_begin(&result, b);
            }
            (_, Some(e)) => {
                result = remove_marker_line(&result, e, SECTION_END_PREFIX);
            }
            (None, None) => break,
        }
    }
    result
}

/// Remove the beads section from hook content.
///
/// Returns (content, was_found).
fn remove_hook_section(content: &str) -> (String, bool) {
    let begin_idx = content.find(SECTION_BEGIN_PREFIX);
    let end_idx = content.find(SECTION_END_PREFIX);

    let result = match (begin_idx, end_idx) {
        (Some(b), Some(e)) if b < e => {
            // Valid pair — remove the whole section
            let line_start = content[..b].rfind('\n').map(|i| i + 1).unwrap_or(0);

            let end_of_end = e + SECTION_END_PREFIX.len();
            let end_of_line = content[end_of_end..]
                .find('\n')
                .map(|i| end_of_end + i + 1)
                .unwrap_or(content.len());

            let mut result = content[..line_start].to_string();
            result.push_str(&content[end_of_line..]);
            result
        }
        (Some(b), _) => remove_orphan_begin(content, b),
        (_, Some(e)) => remove_marker_line(content, e, SECTION_END_PREFIX),
        (None, None) => return (content.to_string(), false),
    };

    // Clean up trailing blank lines
    let trimmed = result.trim_end().to_string();
    (trimmed, true)
}

/// Remove an orphaned BEGIN block (no matching END marker).
fn remove_orphan_begin(content: &str, begin_idx: usize) -> String {
    let line_start = content[..begin_idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let after_begin = &content[begin_idx..];

    // Find end of the orphaned block: next blank line or EOF
    let block_end = after_begin
        .find("\n\n")
        .map(|i| begin_idx + i + 1)
        .unwrap_or(content.len());

    let mut result = content[..line_start].to_string();
    result.push_str(&content[block_end..]);
    result
}

/// Remove a single marker line from content.
fn remove_marker_line(content: &str, marker_idx: usize, _prefix: &str) -> String {
    let line_start = content[..marker_idx]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);

    let end_of_line = content[marker_idx..]
        .find('\n')
        .map(|i| marker_idx + i + 1)
        .unwrap_or(content.len());

    let mut result = content[..line_start].to_string();
    result.push_str(&content[end_of_line..]);
    result
}

/// Check if content is only shebang, comments, or blank lines.
fn is_only_shebang_or_empty(content: &str) -> bool {
    content.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty() || trimmed.starts_with("#!") || trimmed.starts_with('#')
    })
}

/// Run a hook's synchronisation logic in-process.
///
/// # Errors
///
/// Returns an error if the hook name is unknown.
/// Execute a managed git hook by name.
///
/// This runs the same logic as the installed shell hook script would:
///
/// - `pre-commit`: export issues to JSONL (`br sync --flush-only`)
/// - `post-merge`: import issues from JSONL (`br sync --import-only`)
/// - `pre-push`: export issues to JSONL (same as pre-commit)
/// - `post-checkout`: import issues from JSONL
///
/// # Errors
///
/// Returns an error if the hook name is unknown or the underlying
/// sync operation fails.
pub fn run_hook(hook_name: &str) -> Result<()> {
    use crate::config;
    use crate::sync::{self, ExportConfig, ExportErrorPolicy, ImportConfig};

    let beads_dir = config::discover_beads_dir(None)?;
    let mut storage_ctx = config::open_storage_with_cli(
        &beads_dir,
        &config::CliOverrides {
            db: None,
            actor: None,
            identity: None,
            json: None,
            display_color: None,
            quiet: None,
            allow_stale: None,
            no_db: None,
            no_daemon: None,
            no_auto_flush: None,
            no_auto_import: None,
            lock_timeout: None,
            held_write_lock_beads_dir: None,
            read_only_fast_open: true,
        },
    )?;

    match hook_name {
        "pre-commit" | "pre-push" => {
            let jsonl_path = beads_dir.join("beads.jsonl");
            let export_config = ExportConfig {
                force: false,
                is_default_path: true,
                error_policy: ExportErrorPolicy::Strict,
                retention_days: None,
                beads_dir: Some(beads_dir),
                allow_external_jsonl: false,
                show_progress: false,
                history: crate::sync::history::HistoryConfig::default(),
                max_parallel_workers: 0,
            };
            sync::export_to_jsonl_with_policy(
                &storage_ctx.storage,
                &jsonl_path,
                &export_config,
            )?;
        }
        "post-merge" | "post-checkout" => {
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
                };
                sync::import_from_jsonl(
                    &mut storage_ctx.storage,
                    &jsonl_path,
                    &import_config,
                    None,
                )?;
            }
        }
        _ => {
            return Err(BeadsError::Internal {
                message: format!("Unknown hook name: {hook_name}"),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_hook_script_contains_markers() {
        let script = generate_hook_script("pre-commit");
        assert!(script.contains(SECTION_BEGIN_PREFIX));
        assert!(script.contains(SECTION_END_PREFIX));
        assert!(script.contains("br sync --flush-only"));
    }

    #[test]
    fn test_generate_post_merge_script() {
        let script = generate_hook_script("post-merge");
        assert!(script.contains("br sync --import-only"));
    }

    #[test]
    fn test_inject_hook_section_empty() {
        let section = generate_hook_script("pre-commit");
        let result = inject_hook_section("", &section);
        assert!(result.contains(SECTION_BEGIN_PREFIX));
        assert!(result.contains(SECTION_END_PREFIX));
    }

    #[test]
    fn test_inject_hook_section_preserves_user_content() {
        let section = generate_hook_script("pre-commit");
        let existing = "#!/bin/sh\n# User content here\nexit 0\n";
        let result = inject_hook_section(existing, &section);
        assert!(result.contains("User content here"));
        assert!(result.contains(SECTION_BEGIN_PREFIX));
    }

    #[test]
    fn test_inject_hook_section_replaces_existing() {
        let old_section = generate_hook_script("pre-commit");
        let existing = format!("#!/bin/sh\n{}\n# user code", old_section);
        let new_section = generate_hook_script("pre-commit");
        let result = inject_hook_section(&existing, &new_section);
        // Should contain the markers once
        let begin_count = result.matches(SECTION_BEGIN_PREFIX).count();
        assert_eq!(
            begin_count, 1,
            "Should have exactly one BEGIN marker"
        );
    }

    #[test]
    fn test_remove_hook_section() {
        let section = generate_hook_script("pre-commit");
        let content = format!(
            "#!/bin/sh\n# my hook\n{}\n# more user code\nexit 0\n",
            section
        );
        let (result, found) = remove_hook_section(&content);
        assert!(found);
        assert!(result.contains("my hook"));
        assert!(result.contains("more user code"));
        assert!(!result.contains(SECTION_BEGIN_PREFIX));
        assert!(!result.contains(SECTION_END_PREFIX));
    }

    #[test]
    fn test_is_only_shebang_or_empty() {
        assert!(is_only_shebang_or_empty("#!/bin/sh\n# comment\n"));
        assert!(is_only_shebang_or_empty(""));
        assert!(!is_only_shebang_or_empty("#!/bin/sh\necho hi"));
    }

    #[test]
    fn test_remove_hook_section_no_markers() {
        let content = "#!/bin/sh\necho hi\n";
        let (result, found) = remove_hook_section(content);
        assert!(!found);
        assert_eq!(result, content);
    }
}
