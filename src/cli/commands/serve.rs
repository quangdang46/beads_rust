//! `br serve` subcommand — manage HTTP REST API server and MCP stdio server.
//!
//! This module provides commands to start, check status, and stop the HTTP REST API server.
//! The server runs as a background daemon, with its PID stored in `.beads/serve.pid`.

use crate::cli::ServeCommand;
use crate::config;
use crate::output::OutputContext;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// Returns the filename for the serve PID file.
fn serve_pid_filename() -> &'static str {
    "serve.pid"
}

/// Returns the absolute path to the serve PID file, or None if beads dir cannot be found.
fn serve_pid_path() -> Option<PathBuf> {
    let beads_dir = config::discover_beads_dir(None).ok()?;
    Some(beads_dir.join(serve_pid_filename()))
}

/// Execute the `br serve` command, dispatching to the appropriate subcommand.
pub fn run(command: &ServeCommand, ctx: &OutputContext) -> Result<()> {
    match command {
        ServeCommand::Start => cmd_start(ctx),
        ServeCommand::Status => cmd_status(ctx),
        ServeCommand::Stop => cmd_stop(ctx),
    }
}

/// Start the HTTP REST API server.
///
/// Currently prints a placeholder message indicating the HTTP API is not yet implemented.
/// The MCP stdio server can be started with `br serve --mcp` instead.
fn cmd_start(ctx: &OutputContext) -> Result<()> {
    let db_path =
        std::env::var("BR_DATABASE_PATH").unwrap_or_else(|_| ".beads/beads.db".to_string());

    if ctx.is_json() {
        println!(
            r#"{{"status":"not_implemented","message":"br serve HTTP API not yet implemented — use br serve --mcp for MCP stdio server","db_path":"{}"}}"#,
            db_path
        );
    } else {
        println!("br serve HTTP API not yet implemented — use br serve --mcp for MCP stdio server");
        println!("Database path: {db_path}");
    }

    Ok(())
}

/// Check if the HTTP REST API server is running and print its status.
fn cmd_status(ctx: &OutputContext) -> Result<()> {
    let pid_path = match serve_pid_path() {
        Some(path) => path,
        None => {
            if ctx.is_json() {
                println!(r#"{{"running":false,"reason":"beads_dir_not_found"}}"#);
            } else {
                println!("Beads directory not found.");
            }
            return Ok(());
        }
    };

    if !pid_path.exists() {
        if ctx.is_json() {
            println!(r#"{{"running":false,"reason":"not_started"}}"#);
        } else {
            println!("HTTP API server is not running (no PID file found).");
        }
        return Ok(());
    }

    let pid: u32 = match std::fs::read_to_string(&pid_path) {
        Ok(content) => content.trim().parse().context("Invalid PID in file")?,
        Err(e) => {
            if ctx.is_json() {
                println!(
                    r#"{{"running":false,"reason":"read_error","error":"{}"}}"#,
                    e
                );
            } else {
                println!("Error reading PID file: {e}");
            }
            return Ok(());
        }
    };

    // Check if process is running
    let is_running = check_process_running(pid);

    if is_running {
        if ctx.is_json() {
            println!(r#"{{"running":true,"pid":{}}}"#, pid);
        } else {
            println!("HTTP API server is running (PID: {pid})");
        }
    } else {
        // Clean up stale PID file
        if let Err(e) = std::fs::remove_file(&pid_path) {
            tracing::warn!(path = %pid_path.display(), error = %e, "Failed to remove stale PID file");
        }
        if ctx.is_json() {
            println!(r#"{{"running":false,"reason":"process_dead"}}"#);
        } else {
            println!("HTTP API server is not running (stale PID file removed).");
        }
    }

    Ok(())
}

/// Stop the running HTTP REST API server by reading the PID file and killing the process.
fn cmd_stop(ctx: &OutputContext) -> Result<()> {
    let pid_path = match serve_pid_path() {
        Some(path) => path,
        None => {
            if ctx.is_json() {
                println!(r#"{{"stopped":false,"reason":"beads_dir_not_found"}}"#);
            } else {
                println!("Beads directory not found. Nothing to stop.");
            }
            return Ok(());
        }
    };

    if !pid_path.exists() {
        if ctx.is_json() {
            println!(r#"{{"stopped":false,"reason":"not_started"}}"#);
        } else {
            println!("HTTP API server is not running (no PID file found).");
        }
        return Ok(());
    }

    let pid: u32 = match std::fs::read_to_string(&pid_path) {
        Ok(content) => content.trim().parse().context("Invalid PID in file")?,
        Err(e) => {
            if ctx.is_json() {
                println!(
                    r#"{{"stopped":false,"reason":"read_error","error":"{}"}}"#,
                    e
                );
            } else {
                println!("Error reading PID file: {e}");
            }
            return Ok(());
        }
    };

    // Check if process is running before attempting to kill
    if !check_process_running(pid) {
        // Clean up stale PID file
        if let Err(e) = std::fs::remove_file(&pid_path) {
            tracing::warn!(path = %pid_path.display(), error = %e, "Failed to remove stale PID file");
        }
        if ctx.is_json() {
            println!(r#"{{"stopped":false,"reason":"process_dead"}}"#);
        } else {
            println!("HTTP API server is not running (stale PID file removed).");
        }
        return Ok(());
    }

    // Attempt to kill the process
    #[cfg(unix)]
    {
        match Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .output()
        {
            Ok(output) if output.status.success() => {
                // Remove the PID file after successful kill
                if let Err(e) = std::fs::remove_file(&pid_path) {
                    tracing::warn!(path = %pid_path.display(), error = %e, "Failed to remove PID file after kill");
                }
                if ctx.is_json() {
                    println!(r#"{{"stopped":true,"pid":{}}}"#, pid);
                } else {
                    println!("HTTP API server stopped (PID: {pid}).");
                }
            }
            Ok(output) => {
                if ctx.is_json() {
                    println!(
                        r#"{{"stopped":false,"pid":{},"reason":"kill_failed","stderr":"{}"}}"#,
                        pid,
                        String::from_utf8_lossy(&output.stderr)
                    );
                } else {
                    println!("Failed to stop server: kill returned non-zero exit status.");
                    println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
                }
            }
            Err(e) => {
                if ctx.is_json() {
                    println!(
                        r#"{{"stopped":false,"pid":{},"reason":"kill_error","error":"{}"}}"#,
                        pid, e
                    );
                } else {
                    println!("Error executing kill command: {e}");
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix systems, just report that we can't stop
        if ctx.is_json() {
            println!(
                r#"{{"stopped":false,"pid":{},"reason":"not_supported_on_this_platform"}}"#,
                pid
            );
        } else {
            println!("Stopping processes is not supported on this platform (PID: {pid}).");
        }
    }

    Ok(())
}

/// Check if a process with the given PID is currently running.
fn check_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // On Unix, sending signal 0 checks if process exists without killing it
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        // On non-Unix, try to open /proc/<pid>
        let proc_path = std::path::Path::new("/proc").join(pid.to_string());
        proc_path.exists()
    }
}
