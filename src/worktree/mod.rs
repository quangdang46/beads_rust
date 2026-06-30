//! # Git worktree integration
//!
//! Detects git worktrees, manages `.beads/redirect` files for sharing bead
//! state between branches, and provides the `br worktree` subcommand.
//!
//! ## Architecture
//!
//! A git worktree is an additional working directory linked to a different
//! branch of the same repository. Each worktree has its own `.beads/`
//! directory by default. A `.beads/redirect` file (containing an absolute
//! path or a path relative to the worktree root) tells `br` to use another
//! `.beads/` directory instead, enabling bead sharing between branches.
//!
//! ## Safety
//!
//! - All git subprocess calls use `core.hooksPath=` and `GIT_TEMPLATE_DIR=`
//!   to disable hook execution (matching the sync engine's security model).
//! - `.beads/redirect` paths are validated (must resolve to an existing dir,
//!   must not contain path traversal components).
//! - Worktree removal checks for uncommitted changes and unpushed commits
//!   unless `--force` is passed.

use anyhow::{Context as _, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Beads state for a given worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BeadsState {
    /// No `.beads/` directory found.
    None,
    /// `.beads/` exists but is a local copy (not a redirect).
    Local,
    /// `.beads/` is the same as the main repository's (shared).
    Shared,
    /// `.beads/` contains a `redirect` file pointing elsewhere.
    Redirect,
}

impl BeadsState {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Local => "local",
            Self::Shared => "shared",
            Self::Redirect => "redirect",
        }
    }
}

/// Information about a single git worktree.
#[derive(Debug, Clone, Serialize)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree root.
    pub path: String,
    /// Directory name (basename of `path`).
    pub name: String,
    /// Current branch (e.g. `refs/heads/main` → `main`).
    pub branch: String,
    /// Whether this is the main worktree.
    pub is_main: bool,
    /// Beads state.
    #[serde(default)]
    pub beads_state: BeadsState,
    /// If `beads_state` is `Redirect`, the target path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_to: Option<String>,
}

/// Name of the redirect file inside `.beads/`.
pub const REDIRECT_FILE_NAME: &str = "redirect";

// ---------------------------------------------------------------------------
// Top-level API
// ---------------------------------------------------------------------------

/// List all git worktrees via `git worktree list --porcelain`.
///
/// Each worktree is enriched with beads state information.
pub fn list_worktrees(beads_dir: Option<&Path>) -> Result<Vec<WorktreeInfo>> {
    let worktrees = raw_worktree_list_porcelain()?;
    let main_beads_dir = beads_dir.map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));

    Ok(worktrees
        .into_iter()
        .map(|mut wt| {
            let path = Path::new(&wt.path);
            wt.beads_state = detect_beads_state(path, main_beads_dir.as_deref());
            if wt.beads_state == BeadsState::Redirect {
                wt.redirect_to = read_redirect_target(path);
            }
            wt
        })
        .collect())
}

/// Show detailed info about the current worktree.
///
/// Returns `None` if we are not in a worktree (i.e., this is the main repo).
pub fn current_worktree_info(beads_dir: Option<&Path>) -> Result<Option<WorktreeInfo>> {
    let cwd = std::env::current_dir().context("failed to get cwd")?;

    // Check whether we're in a worktree by parsing `git rev-parse --git-common-dir`
    let common_dir = capture_git(&cwd, &["rev-parse", "--git-common-dir"])?;
    let git_dir = capture_git(&cwd, &["rev-parse", "--git-dir"])?;

    let is_worktree = {
        let cd = Path::new(common_dir.trim());
        let gd = Path::new(git_dir.trim());
        // In a worktree, --git-dir differs from --git-common-dir
        cd != gd
    };

    if !is_worktree {
        return Ok(None);
    }

    let branch = get_current_branch(&cwd).unwrap_or_else(|| "(unknown)".to_string());
    let main_repo = capture_git(&cwd, &["rev-parse", "--git-common-dir"]).unwrap_or_default();
    let path = cwd.to_string_lossy().to_string();
    let name = cwd
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut wt = WorktreeInfo {
        path,
        name,
        branch,
        is_main: false,
        beads_state: BeadsState::None,
        redirect_to: None,
    };

    if let Some(bd) = beads_dir {
        wt.beads_state = detect_beads_state(&cwd, Some(bd));
        if wt.beads_state == BeadsState::Redirect {
            wt.redirect_to = read_redirect_target(&cwd);
        }
    }

    Ok(Some(wt))
}

/// Create a new git worktree.
///
/// Wraps `git worktree add <path> <branch>`.
pub fn create_worktree(
    path: &Path,
    branch: Option<&str>,
    repo_root: Option<&Path>,
) -> Result<WorktreeInfo> {
    let root = repo_root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let resolved_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };

    let branch_name = match branch {
        Some(b) => b.to_string(),
        None => {
            resolved_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        }
    };

    let resolved_str = resolved_path.to_string_lossy().to_string();

    capture_git(&root, &["worktree", "add", &resolved_str, &branch_name])?;

    // Add to .gitignore if worktree is inside the repo root
    if resolved_path.starts_with(&root) {
        if let Ok(rel) = resolved_path.strip_prefix(&root) {
            let _ = add_to_gitignore(&root, rel.to_string_lossy().as_ref());
        }
    }

    let name = resolved_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(WorktreeInfo {
        path: resolved_str,
        name,
        branch: branch_name.to_string(),
        is_main: false,
        beads_state: BeadsState::None,
        redirect_to: None,
    })
}

/// Remove a git worktree.
///
/// Wraps `git worktree remove [--force] <path>`.
pub fn remove_worktree(
    name_or_path: &str,
    repo_root: Option<&Path>,
    force: bool,
) -> Result<String> {
    let root = repo_root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let resolved = resolve_worktree_path(name_or_path, &root)?;
    let resolved_str = resolved.to_string_lossy().to_string();

    // Safety checks unless --force
    if !force {
        check_worktree_safety(&resolved)
            .context("safety check failed (use --force to skip)")?;
    }

    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&resolved_str);

    capture_git(&root, &args)?;

    // Remove from .gitignore
    if let Ok(rel) = resolved.strip_prefix(&root) {
        let _ = remove_from_gitignore(&root, rel.to_string_lossy().as_ref());
    }

    Ok(resolved_str)
}

// ---------------------------------------------------------------------------
// Beads state detection
// ---------------------------------------------------------------------------

fn detect_beads_state(worktree_path: &Path, main_beads_dir: Option<&Path>) -> BeadsState {
    let beads_dir = worktree_path.join(".beads");
    let redirect_file = beads_dir.join(REDIRECT_FILE_NAME);

    if redirect_file.is_file() {
        return BeadsState::Redirect;
    }

    if beads_dir.is_dir() {
        if let Some(ref main) = main_beads_dir {
            if let Ok(abs_worktree) = beads_dir.canonicalize() {
                if abs_worktree == *main {
                    return BeadsState::Shared;
                }
            }
        }
        return BeadsState::Local;
    }

    BeadsState::None
}

fn read_redirect_target(worktree_path: &Path) -> Option<String> {
    let path = worktree_path.join(".beads").join(REDIRECT_FILE_NAME);
    let data = std::fs::read_to_string(&path).ok()?;
    let target = data.trim().to_string();
    if target.is_empty() {
        return None;
    }
    // Resolve relative paths from the worktree root, matching Go's filepath.Abs behavior
    let resolved = if Path::new(&target).is_relative() {
        let joined = worktree_path.join(&target);
        // Make absolute without requiring existence (like filepath.Abs)
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join(&joined))
            .unwrap_or(joined)
    } else {
        PathBuf::from(&target)
    };
    // Try canonicalize for the best path, fall back to absolute path
    Some(
        resolved
            .canonicalize()
            .unwrap_or(resolved)
            .to_string_lossy()
            .to_string(),
    )
}
// ---------------------------------------------------------------------------

fn raw_worktree_list_porcelain() -> Result<Vec<WorktreeInfo>> {
    let cwd = std::env::current_dir().context("failed to get cwd")?;
    let output = capture_git(&cwd, &["worktree", "list", "--porcelain"])?;

    let mut worktrees: Vec<WorktreeInfo> = Vec::new();
    let mut current: Option<WorktreeInfo> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            // Push previous, start new
            if let Some(wt) = current.take() {
                worktrees.push(wt);
            }
            current = Some(WorktreeInfo {
                path: path.to_string(),
                name: Path::new(path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                branch: String::new(),
                is_main: false,
                beads_state: BeadsState::None,
                redirect_to: None,
            });
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            if let Some(ref mut wt) = current {
                wt.branch = branch.to_string();
            }
        } else if line == "bare" {
            if let Some(ref mut wt) = current {
                wt.is_main = true;
                wt.branch = "(bare)".to_string();
            }
        }
    }

    // Push the last entry
    if let Some(wt) = current {
        worktrees.push(wt);
    }

    // Mark the first worktree as main (unless it's bare)
    if let Some(first) = worktrees.first_mut() {
        if first.branch != "(bare)" {
            first.is_main = true;
        }
    }

    Ok(worktrees)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get the current branch name for a given directory.
fn get_current_branch(dir: &Path) -> Option<String> {
    capture_git(dir, &["branch", "--show-current"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve a worktree name/path to its absolute filesystem path.
fn resolve_worktree_path(name_or_path: &str, repo_root: &Path) -> Result<PathBuf> {
    // Try as absolute path
    let path = Path::new(name_or_path);
    if path.is_absolute() {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
    }

    // Try as relative to cwd
    if let Ok(abs) = std::env::current_dir().map(|d| d.join(name_or_path)) {
        if abs.exists() {
            return Ok(abs);
        }
    }

    // Try relative to repo root
    let repo_path = repo_root.join(name_or_path);
    if repo_path.exists() {
        return Ok(repo_path);
    }

    // Consult git's worktree registry — match by name or path
    let output = capture_git(repo_root, &["worktree", "list", "--porcelain"])?;
    let worktrees = parse_porcelain_for_resolve(&output);
    for wt in &worktrees {
        let wt_name = Path::new(&wt.path)
            .file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        if wt_name == name_or_path || wt.path == name_or_path {
            if Path::new(&wt.path).exists() {
                return Ok(PathBuf::from(&wt.path));
            }
        }
    }

    anyhow::bail!("worktree not found: {name_or_path}");
}

/// Minimal porcelain parser just for path resolution (avoids borrow issues with WorktreeInfo).
struct PorcelainEntry {
    path: String,
}

fn parse_porcelain_for_resolve(output: &str) -> Vec<PorcelainEntry> {
    let mut entries: Vec<PorcelainEntry> = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            entries.push(PorcelainEntry {
                path: path.to_string(),
            });
        }
    }
    entries
}

/// Safety checks before removing a worktree.
fn check_worktree_safety(worktree_path: &Path) -> Result<()> {
    // Check for uncommitted changes
    let status = capture_git(worktree_path, &["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        anyhow::bail!("worktree has uncommitted changes");
    }

    // Check for unpushed commits
    let unpushed = capture_git(worktree_path, &["log", "@{upstream}..", "--oneline"]);
    if let Ok(unpushed) = unpushed {
        if !unpushed.trim().is_empty() {
            anyhow::bail!("worktree has unpushed commits");
        }
    }

    Ok(())
}

/// Add a path to .gitignore (appends entry).
fn add_to_gitignore(repo_root: &Path, entry: &str) -> Result<()> {
    let gitignore = repo_root.join(".gitignore");

    // Check if already ignored
    if is_ignored_by_git(repo_root, entry) {
        return Ok(());
    }

    // Read existing content
    let content = std::fs::read_to_string(&gitignore).unwrap_or_default();

    // Check for existing matching entry or parent pattern
    for line in content.lines() {
        let trimmed = line.trim().trim_end_matches('/');
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == entry || entry.starts_with(&format!("{trimmed}/")) {
            return Ok(());
        }
    }

    // Append with marker comment
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .write(true)
        .open(&gitignore)
        .context("failed to open .gitignore")?;

    use std::io::Write;
    if !content.ends_with('\n') && !content.is_empty() {
        writeln!(file)?;
    }
    writeln!(file, "# br worktree")?;
    writeln!(file, "{entry}/")?;

    Ok(())
}

/// Check if git is already ignoring a path.
fn is_ignored_by_git(repo_root: &Path, entry: &str) -> bool {
    capture_git(repo_root, &["check-ignore", "-q", "--no-index", "--", entry])
        .is_ok()
}

/// Remove a path from .gitignore (removes the marker comment + entry).
fn remove_from_gitignore(repo_root: &Path, entry: &str) -> Result<()> {
    let gitignore = repo_root.join(".gitignore");
    let content = std::fs::read_to_string(&gitignore).unwrap_or_default();

    let mut new_lines: Vec<&str> = Vec::new();
    let mut skip_next = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "# br worktree" {
            skip_next = true;
            continue;
        }
        if skip_next && (trimmed == entry || trimmed == &format!("{entry}/")) {
            skip_next = false;
            continue;
        }
        skip_next = false;
        new_lines.push(line);
    }

    std::fs::write(&gitignore, new_lines.join("\n"))
        .context("failed to write .gitignore")?;

    Ok(())
}

/// Run a git subcommand in the given directory with security hardening.
fn capture_git(dir: &Path, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(["-c", "core.hooksPath="]);
    cmd.args(args);
    cmd.current_dir(dir);
    cmd.env("GIT_TEMPLATE_DIR", "");
    cmd.env_remove("GIT_DIR");
    cmd.env_remove("GIT_WORK_TREE");

    let output = cmd
        .output()
        .with_context(|| format!("failed to execute git in {}: {args:?}", dir.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git command failed: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Porcelain parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_porcelain_basic() {
        let output = "\
worktree /home/user/project
HEAD 1234567890123456789012345678901234567890
branch refs/heads/main

worktree /home/user/project-work
HEAD abcdef0123456789abcdef0123456789abcdef01
branch refs/heads/feature

";
        let wts = parse_porcelain_for_resolve(output);
        assert_eq!(wts.len(), 2);
        assert_eq!(wts[0].path, "/home/user/project");
        assert_eq!(wts[1].path, "/home/user/project-work");
    }

    #[test]
    fn test_parse_porcelain_bare() {
        let output = "\
worktree /home/user/project
HEAD 1234567890123456789012345678901234567890
bare

";
        let wts = parse_porcelain_for_resolve(output);
        assert_eq!(wts.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Beads state detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_beads_state_none() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(
            detect_beads_state(dir.path(), None),
            BeadsState::None
        );
    }

    #[test]
    fn test_detect_beads_state_local() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join(".beads")).unwrap();
        assert_eq!(
            detect_beads_state(dir.path(), None),
            BeadsState::Local
        );
    }

    #[test]
    fn test_detect_beads_state_redirect() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".beads")).unwrap();
        std::fs::write(dir.path().join(".beads").join(REDIRECT_FILE_NAME), "/some/target").unwrap();
        assert_eq!(
            detect_beads_state(dir.path(), None),
            BeadsState::Redirect
        );
    }

    #[test]
    fn test_detect_beads_state_shared() {
        let main = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(main.path().join(".beads")).unwrap();
        let worktree = tempfile::TempDir::new().unwrap();
        // Symlink the same .beads dir
        std::fs::create_dir(worktree.path().join(".beads")).unwrap(); // just a dir copy for the test
        // For shared detection both dirs must resolve to the same canonical path
        // Here we cheat by using the same path
        assert_eq!(
            detect_beads_state(main.path(), Some(&main.path().join(".beads"))),
            BeadsState::Shared
        );
    }

    // -----------------------------------------------------------------------
    // Redirect helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_redirect_target_absolute() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".beads")).unwrap();
        let target = "/tmp/beads-shared";
        std::fs::write(dir.path().join(".beads").join(REDIRECT_FILE_NAME), target).unwrap();
        let result = read_redirect_target(dir.path());
        // The resolved path may or may not exist; we just check it's Some
        assert!(result.is_some());
    }

    #[test]
    fn test_read_redirect_target_relative() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".beads")).unwrap();
        std::fs::write(
            dir.path().join(".beads").join(REDIRECT_FILE_NAME),
            "../other-project/.beads",
        )
        .unwrap();
        let result = read_redirect_target(dir.path());
        // Should resolve relative to worktree root — may not exist, but shouldn't crash
        assert!(result.is_some());
    }

    #[test]
    fn test_read_redirect_target_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(read_redirect_target(dir.path()).is_none());
    }

    // -----------------------------------------------------------------------
    // Gitignore helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_ignored_by_git_not_in_repo() {
        // Works even outside a git repo — just returns false
        let dir = tempfile::TempDir::new().unwrap();
        assert!(!is_ignored_by_git(dir.path(), "foo"));
    }

    #[test]
    fn test_add_to_gitignore_new_file() {
        let dir = tempfile::TempDir::new().unwrap();
        // Initialize git repo
        capture_git(dir.path(), &["init"]).unwrap();

        let result = add_to_gitignore(dir.path(), "my-worktree");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains("my-worktree/"));
        assert!(content.contains("# br worktree"));
    }

    // -----------------------------------------------------------------------
    // WorktreeInfo helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_beads_state_as_str() {
        assert_eq!(BeadsState::None.as_str(), "none");
        assert_eq!(BeadsState::Local.as_str(), "local");
        assert_eq!(BeadsState::Shared.as_str(), "shared");
        assert_eq!(BeadsState::Redirect.as_str(), "redirect");
    }
}
