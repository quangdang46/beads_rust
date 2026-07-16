//! `br reflect` — agent instructions to sync beads with the current codebase.
//!
//! Read-only instruction emitter (same product shape as `br prime`). Computes
//! an anchor from the last git commit that touched `.beads/issues.jsonl`, then
//! emits facts + a protocol that agents use to create/close/update issues.
//!
//! Does **not** mutate issues. Agent runs `br create` / `br close` / `br update`
//! after reading the instructions.

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;
use serde::Serialize;

use crate::config::{self, CliOverrides};
use crate::error::{BeadsError, Result};
use crate::model::Status;
use crate::output::OutputContext;
use crate::storage::{ListFilters, SqliteStorage};

const CONTRACT_VERSION: &str = "br.reflect.v1";
const COMMIT_LOG_CAP: usize = 40;
const DIFF_STAT_CAP_LINES: usize = 80;
const OPEN_ISSUES_CAP: usize = 100;

/// Arguments for `br reflect`.
#[derive(Args, Debug, Clone)]
pub struct ReflectArgs {
    /// Compact mode (MCP-friendly, token-light reminders)
    #[arg(long)]
    pub mcp: bool,

    /// Dump the default reflect template to stdout (ignore `.beads/REFLECT.md`)
    #[arg(long)]
    pub export: bool,

    /// Override the auto-detected git anchor (commit SHA / ref)
    #[arg(long, value_name = "REV")]
    pub since: Option<String>,
}

/// Machine envelope for `br reflect --json` (`br.reflect.v1`).
#[derive(Debug, Clone, Serialize)]
pub struct ReflectEnvelope {
    pub tool: &'static str,
    pub command: &'static str,
    pub contract_version: &'static str,
    pub anchor: ReflectCommitRef,
    pub head: ReflectCommitRef,
    pub range: ReflectRange,
    pub commits: Vec<String>,
    pub diff_stat: String,
    pub open_issues: Vec<ReflectOpenIssue>,
    pub orphans: Vec<ReflectOrphan>,
    pub hints: Vec<String>,
    pub stats_summary: ReflectStatsSummary,
    pub instructions_markdown: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReflectCommitRef {
    pub sha: String,
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReflectRange {
    pub spec: String,
    pub commit_count: usize,
    pub files_changed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReflectOpenIssue {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReflectOrphan {
    pub issue_id: String,
    pub title: String,
    pub latest_commit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReflectStatsSummary {
    pub open: usize,
    pub in_progress: usize,
    pub total_listed: usize,
}

/// Default reflect template content (returned by `--export`).
const DEFAULT_REFLECT_TEMPLATE: &str = r#"# br reflect — sync beads with codebase

## Goal

Make open/closed beads match what the code actually does since the beads JSONL last changed.

## Hard rules

- Do NOT invent product requirements. Use git + files as evidence.
- Prefer closing stale opens with a cite (commit SHA / path).
- Create beads for shipped work with no issue ID in history.
- Create open beads only for clear remaining gaps (proof required).
- One logical feature = one bead (or epic child).
- After mutations: `br sync --flush-only` (no auto git commit/push).

## Steps

1. Read Facts (anchor..HEAD).
2. For each OPEN issue: verify shipped in code/tests/docs.
   - Shipped → `br close <id> -r "Shipped: <sha> <proof>"`
   - Partial → comment + leave open or split child
   - Obsolete → close deferred/obsolete
3. Commits/files with no bead → group → create (+ close if already done)
4. WIP/plans gaps missing from open → `br create` (open)
5. `br list --status open` + `br ready` sanity
6. `br sync --flush-only`
"#;

/// Compact MCP-mode reflect content.
const DEFAULT_MCP_REFLECT: &str = "br reflect: sync beads ↔ git/code (instructions only).\n\
    1) Read Facts (anchor..HEAD) 2) close shipped opens with proof \
    3) create beads for untracked ship 4) br sync --flush-only (NO git)\n";

/// Execute the reflect command.
///
/// # Errors
///
/// Returns an error when the workspace is missing, git is unavailable, the
/// requested `--since` rev cannot be resolved, or storage cannot be opened.
pub fn execute(
    args: &ReflectArgs,
    json_mode: bool,
    overrides: &CliOverrides,
    ctx: &OutputContext,
) -> Result<()> {
    if args.export {
        let template = if args.mcp {
            DEFAULT_MCP_REFLECT
        } else {
            DEFAULT_REFLECT_TEMPLATE
        };
        println!("{template}");
        return Ok(());
    }

    let beads_dir = config::discover_beads_dir_with_cli(overrides)?;
    let storage_ctx = config::open_storage_with_cli(&beads_dir, overrides)?;
    let storage = &storage_ctx.storage;
    let jsonl_path = &storage_ctx.paths.jsonl_path;

    let repo_root = git_repo_root_for_path(jsonl_path)
        .or_else(|| git_repo_root_for_path(&beads_dir))
        .ok_or_else(|| {
            BeadsError::Config(
                "br reflect requires a git repository (could not resolve repo root)".to_string(),
            )
        })?;

    let facts = gather_facts(
        &repo_root,
        &beads_dir,
        jsonl_path,
        storage,
        args.since.as_deref(),
    )?;

    // Optional project override of the agent protocol body.
    let protocol = if args.mcp {
        None
    } else {
        let reflect_md = beads_dir.join("REFLECT.md");
        if reflect_md.is_file() {
            std::fs::read_to_string(&reflect_md).ok()
        } else {
            None
        }
    };

    let instructions = if args.mcp {
        format_mcp_output(&facts)
    } else {
        format_full_output(&facts, protocol.as_deref())
    };

    if json_mode || ctx.is_json() {
        let envelope = ReflectEnvelope {
            tool: "br",
            command: "reflect",
            contract_version: CONTRACT_VERSION,
            anchor: facts.anchor.clone(),
            head: facts.head.clone(),
            range: facts.range.clone(),
            commits: facts.commits.clone(),
            diff_stat: facts.diff_stat.clone(),
            open_issues: facts.open_issues.clone(),
            orphans: facts.orphans.clone(),
            hints: facts.hints.clone(),
            stats_summary: facts.stats_summary.clone(),
            instructions_markdown: instructions,
            mode: if args.mcp {
                "mcp".to_string()
            } else {
                "full".to_string()
            },
        };
        println!("{}", serde_json::to_string_pretty(&envelope)?);
    } else {
        println!("{instructions}");
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct ReflectFacts {
    anchor: ReflectCommitRef,
    head: ReflectCommitRef,
    range: ReflectRange,
    commits: Vec<String>,
    diff_stat: String,
    open_issues: Vec<ReflectOpenIssue>,
    orphans: Vec<ReflectOrphan>,
    hints: Vec<String>,
    stats_summary: ReflectStatsSummary,
}

fn gather_facts(
    repo_root: &Path,
    beads_dir: &Path,
    jsonl_path: &Path,
    storage: &SqliteStorage,
    since_override: Option<&str>,
) -> Result<ReflectFacts> {
    let (anchor_sha, anchor_path) = resolve_anchor(repo_root, beads_dir, jsonl_path, since_override)?;
    let anchor_date = git_commit_date(repo_root, &anchor_sha).unwrap_or_else(|| "unknown".into());
    let head_sha = git_rev_parse(repo_root, "HEAD").ok_or_else(|| {
        BeadsError::Config("failed to resolve HEAD in git repository".to_string())
    })?;
    let head_date = git_commit_date(repo_root, &head_sha).unwrap_or_else(|| "unknown".into());

    let range_spec = format!("{anchor_sha}..HEAD");
    let commit_count = git_rev_list_count(repo_root, &range_spec).unwrap_or(0);
    let commits = git_log_oneline(repo_root, &range_spec, COMMIT_LOG_CAP).unwrap_or_default();
    let diff_stat_raw = git_diff_stat(repo_root, &range_spec).unwrap_or_default();
    let files_changed = count_diff_stat_files(&diff_stat_raw);
    let diff_stat = truncate_lines(&diff_stat_raw, DIFF_STAT_CAP_LINES);

    let open_issues = load_open_issues(storage)?;
    let stats_summary = ReflectStatsSummary {
        open: open_issues
            .iter()
            .filter(|i| i.status == "open")
            .count(),
        in_progress: open_issues
            .iter()
            .filter(|i| i.status == "in_progress")
            .count(),
        total_listed: open_issues.len(),
    };

    let orphans = find_orphan_hits(repo_root, &range_spec, &open_issues);
    let hints = collect_plan_hints(repo_root);

    Ok(ReflectFacts {
        anchor: ReflectCommitRef {
            sha: short_sha(&anchor_sha),
            date: anchor_date,
            path: Some(anchor_path),
        },
        head: ReflectCommitRef {
            sha: short_sha(&head_sha),
            date: head_date,
            path: None,
        },
        range: ReflectRange {
            spec: format!("{}..{}", short_sha(&anchor_sha), short_sha(&head_sha)),
            commit_count,
            files_changed,
        },
        commits,
        diff_stat,
        open_issues,
        orphans,
        hints,
        stats_summary,
    })
}

fn resolve_anchor(
    repo_root: &Path,
    beads_dir: &Path,
    jsonl_path: &Path,
    since_override: Option<&str>,
) -> Result<(String, String)> {
    if let Some(rev) = since_override {
        let sha = git_rev_parse(repo_root, rev).ok_or_else(|| {
            BeadsError::validation(
                "since",
                format!("cannot resolve git revision '{rev}'"),
            )
        })?;
        return Ok((sha, format!("--since {rev}")));
    }

    // Prefer last commit that touched issues.jsonl (relative path from repo root).
    let rel_jsonl = path_relative_to(jsonl_path, repo_root)
        .unwrap_or_else(|| PathBuf::from(".beads/issues.jsonl"));
    if let Some(sha) = git_log_last_touching(repo_root, &rel_jsonl) {
        return Ok((
            sha,
            rel_jsonl.to_string_lossy().into_owned(),
        ));
    }

    // Fallback: last commit under .beads/
    let rel_beads = path_relative_to(beads_dir, repo_root)
        .unwrap_or_else(|| PathBuf::from(".beads"));
    if let Some(sha) = git_log_last_touching(repo_root, &rel_beads) {
        return Ok((sha, format!("{}/**", rel_beads.to_string_lossy())));
    }

    // Fallback: root commit (oldest) so the full history is the range.
    if let Some(sha) = git_rev_list_oldest(repo_root) {
        return Ok((sha, "repository root (no .beads history found)".into()));
    }

    Err(BeadsError::Config(
        "could not determine a git anchor for br reflect (empty repository?)".to_string(),
    ))
}

fn load_open_issues(storage: &SqliteStorage) -> Result<Vec<ReflectOpenIssue>> {
    let filters = ListFilters {
        statuses: Some(vec![Status::Open, Status::InProgress]),
        sort: Some("priority".to_string()),
        ..Default::default()
    };
    let issues = storage.list_issues(&filters)?;
    let mut out: Vec<ReflectOpenIssue> = issues
        .into_iter()
        .map(|issue| ReflectOpenIssue {
            id: issue.id,
            title: issue.title,
            status: issue.status.as_str().to_string(),
            priority: issue.priority.0,
        })
        .collect();
    // Stable secondary sort by id for determinism.
    out.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.id.cmp(&b.id))
    });
    if out.len() > OPEN_ISSUES_CAP {
        out.truncate(OPEN_ISSUES_CAP);
    }
    Ok(out)
}

/// Best-effort orphans: open issues whose IDs appear in commits since anchor.
fn find_orphan_hits(
    repo_root: &Path,
    range_spec: &str,
    open_issues: &[ReflectOpenIssue],
) -> Vec<ReflectOrphan> {
    if open_issues.is_empty() {
        return Vec::new();
    }
    let log = git_log_oneline(repo_root, range_spec, 500).unwrap_or_default();
    if log.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for issue in open_issues {
        for line in &log {
            // Match whole-token-ish: id appears after whitespace or '(' etc.
            if line.contains(&issue.id) {
                let sha = line.split_whitespace().next().unwrap_or("").to_string();
                hits.push(ReflectOrphan {
                    issue_id: issue.id.clone(),
                    title: issue.title.clone(),
                    latest_commit: sha,
                });
                break;
            }
        }
    }
    hits
}

fn collect_plan_hints(repo_root: &Path) -> Vec<String> {
    let mut hints = Vec::new();
    for candidate in ["WIP.md", "docs/plans", "PLAN.md", "TODO.md"] {
        let path = repo_root.join(candidate);
        if path.exists() {
            hints.push(candidate.to_string());
        }
    }
    hints
}

fn format_full_output(facts: &ReflectFacts, protocol_override: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("# br reflect — sync beads with codebase\n\n");

    out.push_str(&format!(
        "Anchor: {} ({}) — {}\n",
        facts.anchor.sha,
        facts.anchor.date,
        facts.anchor.path.as_deref().unwrap_or(".beads")
    ));
    out.push_str(&format!(
        "HEAD:   {} ({})\n",
        facts.head.sha, facts.head.date
    ));
    out.push_str(&format!(
        "Range:  {}  ({} commits, {} files)\n\n",
        facts.range.spec, facts.range.commit_count, facts.range.files_changed
    ));

    out.push_str(&format!(
        "## Stats\n\n- open: {}\n- in_progress: {}\n- listed: {}\n\n",
        facts.stats_summary.open,
        facts.stats_summary.in_progress,
        facts.stats_summary.total_listed
    ));

    out.push_str(&format!(
        "## Open issues ({})\n\n",
        facts.open_issues.len()
    ));
    if facts.open_issues.is_empty() {
        out.push_str("_none_\n\n");
    } else {
        for issue in &facts.open_issues {
            out.push_str(&format!(
                "- {}  P{}  [{}] {}\n",
                issue.id, issue.priority, issue.status, issue.title
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "## Commits since anchor ({})\n\n",
        facts.commits.len()
    ));
    if facts.commits.is_empty() {
        out.push_str("_none (anchor is HEAD or empty range)_\n\n");
    } else {
        for line in &facts.commits {
            out.push_str(&format!("- {line}\n"));
        }
        if facts.range.commit_count > facts.commits.len() {
            out.push_str(&format!(
                "- … {} more commits not shown (cap {})\n",
                facts.range.commit_count - facts.commits.len(),
                COMMIT_LOG_CAP
            ));
        }
        out.push('\n');
    }

    out.push_str("## Diff stat\n\n```\n");
    if facts.diff_stat.is_empty() {
        out.push_str("(empty)\n");
    } else {
        out.push_str(&facts.diff_stat);
        if !facts.diff_stat.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str("```\n\n");

    out.push_str(&format!(
        "## Orphans (open IDs referenced in range) ({})\n\n",
        facts.orphans.len()
    ));
    if facts.orphans.is_empty() {
        out.push_str("_none in this range — also run `br orphans` for full history_\n\n");
    } else {
        for o in &facts.orphans {
            out.push_str(&format!(
                "- {}  {}  (latest: {})\n",
                o.issue_id, o.title, o.latest_commit
            ));
        }
        out.push('\n');
    }

    if !facts.hints.is_empty() {
        out.push_str("## Plan / WIP hints\n\n");
        for h in &facts.hints {
            out.push_str(&format!("- `{h}` present — review for gaps\n"));
        }
        out.push('\n');
    }

    out.push_str("## Agent protocol\n\n");
    if let Some(protocol) = protocol_override {
        out.push_str(protocol.trim());
        out.push('\n');
    } else {
        // Strip the H1 from the default template since we already emitted one.
        let body = DEFAULT_REFLECT_TEMPLATE
            .trim_start_matches("# br reflect — sync beads with codebase\n\n");
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }

    out
}

fn format_mcp_output(facts: &ReflectFacts) -> String {
    let mut out = String::from(DEFAULT_MCP_REFLECT);
    out.push_str(&format!(
        "range: {} ({} commits, {} files)\n",
        facts.range.spec, facts.range.commit_count, facts.range.files_changed
    ));
    out.push_str(&format!(
        "open: {} | in_progress: {} | orphans_in_range: {}\n",
        facts.stats_summary.open,
        facts.stats_summary.in_progress,
        facts.orphans.len()
    ));
    if !facts.open_issues.is_empty() {
        out.push_str("issues: ");
        let ids: Vec<&str> = facts
            .open_issues
            .iter()
            .take(12)
            .map(|i| i.id.as_str())
            .collect();
        out.push_str(&ids.join(", "));
        if facts.open_issues.len() > 12 {
            out.push_str(", …");
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Git helpers (local; br does not automate git commit/push)
// ---------------------------------------------------------------------------

fn git_repo_root_for_path(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

fn git_rev_parse(repo_root: &Path, rev: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() { None } else { Some(sha) }
}

fn git_commit_date(repo_root: &Path, sha: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["show", "-s", "--format=%cs", sha])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let date = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if date.is_empty() { None } else { Some(date) }
}

fn git_log_last_touching(repo_root: &Path, path: &Path) -> Option<String> {
    let path_str = path.to_string_lossy();
    let output = Command::new("git")
        .args(["log", "-1", "--format=%H", "--", path_str.as_ref()])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() { None } else { Some(sha) }
}

fn git_rev_list_oldest(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-list", "--max-parents=0", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Take the first root (there may be multiple in exotic histories).
    let sha = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if sha.is_empty() { None } else { Some(sha) }
}

fn git_rev_list_count(repo_root: &Path, range_spec: &str) -> Option<usize> {
    let output = Command::new("git")
        .args(["rev-list", "--count", range_spec])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()
}

fn git_log_oneline(repo_root: &Path, range_spec: &str, cap: usize) -> Option<Vec<String>> {
    let output = Command::new("git")
        .args([
            "log",
            "--oneline",
            &format!("--max-count={cap}"),
            range_spec,
        ])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    Some(lines)
}

fn git_diff_stat(repo_root: &Path, range_spec: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["diff", "--stat", range_spec])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn count_diff_stat_files(diff_stat: &str) -> usize {
    // `git diff --stat` ends with a summary line " N files changed, ...".
    // Count non-empty non-summary lines as a good approximation.
    let mut count = 0usize;
    for line in diff_stat.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.contains("files changed") || t.contains("file changed") {
            // Parse "N files changed"
            if let Some(n) = t.split_whitespace().next().and_then(|s| s.parse().ok()) {
                return n;
            }
            continue;
        }
        count += 1;
    }
    count
}

fn truncate_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }
    let mut out = lines[..max_lines].join("\n");
    out.push_str(&format!(
        "\n… {} more lines truncated\n",
        lines.len() - max_lines
    ));
    out
}

fn short_sha(sha: &str) -> String {
    if sha.len() > 7 {
        sha[..7].to_string()
    } else {
        sha.to_string()
    }
}

fn path_relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
    path.strip_prefix(root).ok().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_git_repo(dir: &Path) {
        run_git(dir, &["init", "-b", "main"]);
        run_git(dir, &["config", "user.email", "test@example.com"]);
        run_git(dir, &["config", "user.name", "Test"]);
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("spawn git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn write_and_commit(dir: &Path, rel: &str, contents: &str, msg: &str) -> String {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        run_git(dir, &["add", rel]);
        run_git(dir, &["commit", "-m", msg]);
        git_rev_parse(dir, "HEAD").expect("HEAD after commit")
    }

    #[test]
    fn short_sha_truncates() {
        assert_eq!(short_sha("abcdef0123456789"), "abcdef0");
        assert_eq!(short_sha("abc"), "abc");
    }

    #[test]
    fn count_diff_stat_files_parses_summary() {
        let sample = " src/a.rs | 2 +-\n src/b.rs | 1 +\n 2 files changed, 2 insertions(+), 1 deletion(-)\n";
        assert_eq!(count_diff_stat_files(sample), 2);
    }

    #[test]
    fn truncate_lines_caps() {
        let text = (0..10).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let out = truncate_lines(&text, 3);
        assert!(out.contains("line0"));
        assert!(out.contains("line2"));
        assert!(out.contains("more lines truncated"));
        assert!(!out.contains("line9"));
    }

    #[test]
    fn format_mcp_output_includes_counts() {
        let facts = ReflectFacts {
            anchor: ReflectCommitRef {
                sha: "aaaaaaa".into(),
                date: "2026-07-01".into(),
                path: Some(".beads/issues.jsonl".into()),
            },
            head: ReflectCommitRef {
                sha: "bbbbbbb".into(),
                date: "2026-07-16".into(),
                path: None,
            },
            range: ReflectRange {
                spec: "aaaaaaa..bbbbbbb".into(),
                commit_count: 3,
                files_changed: 5,
            },
            commits: vec!["bbbbbbb feat: x".into()],
            diff_stat: "a.rs | 1 +\n".into(),
            open_issues: vec![ReflectOpenIssue {
                id: "br-1".into(),
                title: "t".into(),
                status: "open".into(),
                priority: 1,
            }],
            orphans: vec![],
            hints: vec![],
            stats_summary: ReflectStatsSummary {
                open: 1,
                in_progress: 0,
                total_listed: 1,
            },
        };
        let out = format_mcp_output(&facts);
        assert!(out.contains("3 commits"));
        assert!(out.contains("br-1"));
        assert!(out.contains("open: 1"));
    }

    #[test]
    fn format_full_output_is_deterministic() {
        let facts = ReflectFacts {
            anchor: ReflectCommitRef {
                sha: "aaaaaaa".into(),
                date: "2026-07-01".into(),
                path: Some(".beads/issues.jsonl".into()),
            },
            head: ReflectCommitRef {
                sha: "bbbbbbb".into(),
                date: "2026-07-16".into(),
                path: None,
            },
            range: ReflectRange {
                spec: "aaaaaaa..bbbbbbb".into(),
                commit_count: 1,
                files_changed: 1,
            },
            commits: vec!["bbbbbbb feat: ship".into()],
            diff_stat: " file | 1 +\n".into(),
            open_issues: vec![ReflectOpenIssue {
                id: "br-abc".into(),
                title: "Do the thing".into(),
                status: "open".into(),
                priority: 0,
            }],
            orphans: vec![],
            hints: vec!["WIP.md".into()],
            stats_summary: ReflectStatsSummary {
                open: 1,
                in_progress: 0,
                total_listed: 1,
            },
        };
        let a = format_full_output(&facts, None);
        let b = format_full_output(&facts, None);
        assert_eq!(a, b);
        assert!(a.contains("Anchor: aaaaaaa"));
        assert!(a.contains("br-abc"));
        assert!(a.contains("Agent protocol"));
        assert!(a.contains("WIP.md"));
        assert!(a.contains("br sync --flush-only"));
    }

    #[test]
    fn resolve_anchor_prefers_issues_jsonl_commit() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        init_git_repo(root);

        let first = write_and_commit(
            root,
            ".beads/issues.jsonl",
            "{\"id\":\"x\"}\n",
            "chore: beads baseline",
        );
        let _second = write_and_commit(root, "src/main.rs", "fn main() {}\n", "feat: code");

        let beads = root.join(".beads");
        let jsonl = beads.join("issues.jsonl");
        let (sha, path) = resolve_anchor(root, &beads, &jsonl, None).expect("anchor");
        assert_eq!(sha, first);
        assert!(path.contains("issues.jsonl"));
    }

    #[test]
    fn resolve_anchor_honors_since_override() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        init_git_repo(root);
        let first = write_and_commit(root, "a.txt", "a\n", "a");
        let _second = write_and_commit(root, "b.txt", "b\n", "b");

        let beads = root.join(".beads");
        fs::create_dir_all(&beads).unwrap();
        let jsonl = beads.join("issues.jsonl");
        fs::write(&jsonl, "").unwrap();

        let (sha, path) = resolve_anchor(root, &beads, &jsonl, Some("HEAD~1")).unwrap();
        assert_eq!(sha, first);
        assert!(path.contains("--since"));
    }

    #[test]
    fn resolve_anchor_rejects_bad_since() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        init_git_repo(root);
        let _ = write_and_commit(root, "a.txt", "a\n", "a");
        let beads = root.join(".beads");
        fs::create_dir_all(&beads).unwrap();
        let jsonl = beads.join("issues.jsonl");
        fs::write(&jsonl, "").unwrap();

        let err = resolve_anchor(root, &beads, &jsonl, Some("no-such-rev-xyz"))
            .expect_err("bad rev");
        let msg = err.to_string();
        assert!(
            msg.contains("since") || msg.contains("no-such") || msg.contains("resolve"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn git_log_and_diff_helpers_work() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        init_git_repo(root);
        let a = write_and_commit(root, "a.txt", "a\n", "commit-a");
        let _b = write_and_commit(root, "b.txt", "b\n", "commit-b");
        let range = format!("{a}..HEAD");
        let count = git_rev_list_count(root, &range).unwrap();
        assert_eq!(count, 1);
        let log = git_log_oneline(root, &range, 10).unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("commit-b"));
        let stat = git_diff_stat(root, &range).unwrap();
        assert!(stat.contains("b.txt") || stat.contains("file"));
    }

    #[test]
    fn export_template_constants_are_nonempty() {
        assert!(DEFAULT_REFLECT_TEMPLATE.contains("br sync --flush-only"));
        assert!(DEFAULT_MCP_REFLECT.contains("br reflect"));
    }
}
