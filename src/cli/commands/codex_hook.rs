//! Native `br codex-hook` command.
//!
//! Implements Codex CLI lifecycle hooks comparable to the Go `bd codex-hook`:
//! - SessionStart: inject `br prime` output as additional context
//! - PreCompact: run `br prime --memories-only` to validate state before compaction
//! - PostCompact: create a refresh marker file
//! - UserPromptSubmit: if refresh marker exists, run `br prime` and clear the marker
//!
//! The Go upstream is `/tmp/beads_upstream/cmd/bd/codex_hook.go`.

use crate::config::CliOverrides;
use crate::error::{BeadsError, Result};
use crate::output::OutputContext;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as FmtWrite;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Hex-encode a SHA256 digest for refresh marker filenames.
fn hex_encode_digest(digest: &[u8]) -> String {
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        write!(&mut s, "{b:02x}").expect("writing to String never fails");
    }
    s
}

/// Marker directory for codex hook refresh markers.
/// Uses XDG_CACHE_HOME or a temp directory fallback.
fn marker_base_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("BR_CODEX_HOOK_CACHE") {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("beads").join("codex-hooks");
    }
    if let Ok(dir) = std::env::var("HOME") {
        let candidate = PathBuf::from(dir)
            .join(".cache")
            .join("beads")
            .join("codex-hooks");
        if candidate.exists() || std::fs::create_dir_all(&candidate).is_ok() {
            return candidate;
        }
    }
    PathBuf::from("/tmp").join("beads-codex-hooks")
}

/// Compute the refresh marker path for a given session + workspace.
fn refresh_marker_path(session_id: &str, cwd: &str) -> PathBuf {
    let base = marker_base_dir();
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update(b"\x00");
    hasher.update(cwd.as_bytes());
    let hash = hex_encode_digest(&hasher.finalize());
    base.join(format!("{hash}.refresh"))
}

/// Input received from Codex CLI on stdin for hook events.
#[derive(Debug, Deserialize, Default)]
struct CodexHookInput {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    transcript_path: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    hook_event_name: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    trigger: String,
}

/// Response written to stdout for Codex CLI hooks.
#[derive(Debug, Serialize)]
struct CodexHookResponse {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    continue_: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_specific_output: Option<CodexHookSpecificOutput>,
}

#[derive(Debug, Serialize)]
struct CodexHookSpecificOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_event_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_context: Option<String>,
}

/// Run `br prime` and capture its stdout.
fn run_br_prime(memories_only: bool) -> Result<String> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("prime");
    if memories_only {
        cmd.arg("--memories-only");
    }
    let output = cmd
        .output()
        .map_err(|e| BeadsError::Config(format!("Failed to execute br prime: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BeadsError::Config(format!("br prime failed: {stderr}")));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

/// Handle the SessionStart hook: inject `br prime` output as additional context.
fn handle_session_start(input: &CodexHookInput, stdout: io::StdoutLock<'_>) -> Result<()> {
    let out = run_br_prime(false)?;
    let trimmed = out.trim();
    if trimmed.is_empty() {
        // No additional context; just signal continue
        let response = CodexHookResponse {
            continue_: true,
            system_message: None,
            hook_specific_output: None,
        };
        let mut writer = io::BufWriter::new(stdout);
        serde_json::to_writer(&mut writer, &response)?;
        writeln!(writer)?;
        return Ok(());
    }

    let response = CodexHookResponse {
        continue_: true,
        system_message: None,
        hook_specific_output: Some(CodexHookSpecificOutput {
            hook_event_name: Some("SessionStart".to_string()),
            additional_context: Some(trimmed.to_string()),
        }),
    };
    let mut writer = io::BufWriter::new(stdout);
    serde_json::to_writer(&mut writer, &response)?;
    writeln!(writer)?;
    Ok(())
}

/// Handle the PreCompact hook: validate state before compaction.
fn handle_pre_compact(stdout: io::StdoutLock<'_>) -> Result<()> {
    match run_br_prime(true) {
        Ok(_) => {
            let response = CodexHookResponse {
                continue_: true,
                system_message: None,
                hook_specific_output: None,
            };
            let mut writer = io::BufWriter::new(stdout);
            serde_json::to_writer(&mut writer, &response)?;
            writeln!(writer)?;
            Ok(())
        }
        Err(e) => {
            let response = CodexHookResponse {
                continue_: true,
                system_message: Some(format!("Beads context check failed before compaction: {e}")),
                hook_specific_output: None,
            };
            let mut writer = io::BufWriter::new(stdout);
            serde_json::to_writer(&mut writer, &response)?;
            writeln!(writer)?;
            Ok(())
        }
    }
}

/// Handle the PostCompact hook: create a refresh marker.
fn handle_post_compact(input: &CodexHookInput) -> Result<()> {
    let path = refresh_marker_path(&input.session_id, &input.cwd);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, b"1\n")?;
    Ok(())
}

/// Handle the UserPromptSubmit hook: check for refresh marker, run prime if set.
fn handle_user_prompt_submit(input: &CodexHookInput, stdout: io::StdoutLock<'_>) -> Result<()> {
    let path = refresh_marker_path(&input.session_id, &input.cwd);
    if !path.exists() {
        // No marker; just continue
        let response = CodexHookResponse {
            continue_: true,
            system_message: None,
            hook_specific_output: None,
        };
        let mut writer = io::BufWriter::new(stdout);
        serde_json::to_writer(&mut writer, &response)?;
        writeln!(writer)?;
        return Ok(());
    }

    // Remove the marker first (best-effort)
    let _ = std::fs::remove_file(&path);

    match run_br_prime(false) {
        Ok(out) => {
            let trimmed = out.trim();
            if trimmed.is_empty() {
                let response = CodexHookResponse {
                    continue_: true,
                    system_message: None,
                    hook_specific_output: None,
                };
                let mut writer = io::BufWriter::new(stdout);
                serde_json::to_writer(&mut writer, &response)?;
                writeln!(writer)?;
                return Ok(());
            }
            let response = CodexHookResponse {
                continue_: true,
                system_message: None,
                hook_specific_output: Some(CodexHookSpecificOutput {
                    hook_event_name: Some("UserPromptSubmit".to_string()),
                    additional_context: Some(trimmed.to_string()),
                }),
            };
            let mut writer = io::BufWriter::new(stdout);
            serde_json::to_writer(&mut writer, &response)?;
            writeln!(writer)?;
            Ok(())
        }
        Err(e) => {
            let response = CodexHookResponse {
                continue_: true,
                system_message: Some(format!(
                    "Beads context refresh after compaction failed: {e}"
                )),
                hook_specific_output: None,
            };
            let mut writer = io::BufWriter::new(stdout);
            serde_json::to_writer(&mut writer, &response)?;
            writeln!(writer)?;
            Ok(())
        }
    }
}

/// Arguments for the codex-hook command.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct CodexHookArgs {
    /// The hook event name (SessionStart, PreCompact, PostCompact, UserPromptSubmit)
    pub event: String,
}

/// Execute the codex-hook command.
///
/// Reads a JSON input from stdin (CodexHookInput) and writes a JSON
/// response (CodexHookResponse) to stdout.
///
/// # Errors
///
/// Returns an error if the hook event is unknown or processing fails.
pub fn execute(args: &CodexHookArgs, _cli: &CliOverrides, _ctx: &OutputContext) -> Result<()> {
    let event = &args.event;

    // Read stdin for the hook input
    let mut stdin_input = String::new();
    io::stdin().read_to_string(&mut stdin_input)?;

    let input: CodexHookInput = if stdin_input.trim().is_empty() {
        CodexHookInput::default()
    } else {
        serde_json::from_str(&stdin_input)?
    };

    // If hook_event_name is set in the input, use it over the command arg
    let effective_event = if !input.hook_event_name.is_empty() {
        &input.hook_event_name
    } else {
        event
    };

    let stdout = io::stdout();
    let stdout_lock = stdout.lock();

    match effective_event.as_str() {
        "SessionStart" => handle_session_start(&input, stdout_lock),
        "PreCompact" => handle_pre_compact(stdout_lock),
        "PostCompact" => handle_post_compact(&input),
        "UserPromptSubmit" => handle_user_prompt_submit(&input, stdout_lock),
        other => Err(BeadsError::Config(format!(
            "unsupported codex hook event: {other}"
        ))),
    }
}
