//! Embedded web UI server for `br`.
//!
//! Serves a static Next.js SPA and a REST API that maps to br's storage layer.
//! Built only in CI via `scripts/build-web.sh`; the static files are embedded
//! via `rust-embed` at compile time.

mod api;
mod assets;

use crate::cli::WebArgs;
use crate::config;
use crate::error::{BeadsError, Result};
use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

/// Shared application state available to all route handlers.
///
/// Storage is NOT shared — each handler opens its own connection in a
/// blocking task (SqliteStorage is !Send due to fsqlite's Rc internals).
pub struct AppState {
    /// Discovered beads directory path.
    pub beads_dir: PathBuf,
    /// CLI overrides (db path, etc.).
    pub overrides: config::CliOverrides,
}

/// Start the web UI server.
///
/// Discovers the beads workspace, builds the router, and binds the HTTP
/// server. Opens a browser unless `--no-open` is set.
///
/// # Errors
///
/// Returns an error if storage can't be opened or the server fails to bind.
#[allow(clippy::module_name_repetitions)]
pub fn run_server(args: &WebArgs, overrides: &config::CliOverrides) -> Result<()> {
    // br web only looks for .beads/ in the current directory — never walks up.
    let beads_dir = if let Some(db_path) = overrides.db.as_ref() {
        let dir = if db_path.is_dir() {
            db_path.join(".beads")
        } else {
            db_path.parent().map(|p| p.join(".beads")).unwrap_or(db_path.join(".beads"))
        };
        if dir.is_dir() {
            dir
        } else {
            return Err(BeadsError::Config(format!("no .beads/ at db path")));
        }
    } else {
        let cwd = std::env::current_dir().map_err(|_| BeadsError::Config("cannot get current directory".into()))?;
        let candidate = cwd.join(".beads");
        if candidate.is_dir() {
            candidate
        } else {
            let banner = console_banner_no_workspace();
            return Err(BeadsError::Config(banner.to_string()));
        }
    };
    // Pre-flight: verify storage is accessible.
    let _storage_ctx = config::open_storage_with_cli(&beads_dir, overrides)
        .map_err(|e| BeadsError::Config(format!("Cannot open storage: {e}")))?;

    let state = Arc::new(AppState {
        beads_dir,
        overrides: overrides.clone(),
    });

    // Build the router with all API routes and static file serving.
    let app = Router::new()
        .route("/", axum::routing::get(|| async { axum::response::Redirect::temporary("/p/default") }))
        .route("/api/p/{project_id}/beads", axum::routing::get(api::list_beads).post(api::create_bead))
        .route(
            "/api/p/{project_id}/beads/{id}",
            axum::routing::get(api::get_bead).patch(api::update_bead).delete(api::delete_bead),
        )
        .route(
            "/api/p/{project_id}/beads/{id}/status",
            axum::routing::post(api::set_status),
        )
        .route(
            "/api/p/{project_id}/beads/{id}/comments",
            axum::routing::post(api::add_comment),
        )
        .route(
            "/api/p/{project_id}/beads/{id}/deps",
            axum::routing::post(api::add_dep).delete(api::remove_dep),
        )
        .route(
            "/api/p/{project_id}/beads/{id}/archive",
            axum::routing::post(api::archive_bead),
        )
        .route("/api/projects", axum::routing::get(api::list_projects))
        .route(
            "/api/p/{project_id}/doctor",
            axum::routing::get(api::doctor),
        )
        .route("/api/config", axum::routing::get(api::get_config).put(api::update_config))
        .fallback_service(axum::routing::get(assets::serve_static))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .map_err(|e| BeadsError::Config(format!("Invalid address: {e}")))?;

    eprintln!(
        "  br web → http://{}:{}/\n  (Ctrl+C to stop)",
        args.host, args.port
    );

    if !args.no_open {
        open_browser(&format!("http://{}:{}/", args.host, args.port));
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| BeadsError::Config(format!("Failed to start runtime: {e}")))?;

    rt.block_on(async {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| BeadsError::Config(format!("Failed to bind {addr}: {e}")))?;

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| BeadsError::Config(format!("Server error: {e}")))?;

        Ok::<(), BeadsError>(())
    })?;

    Ok(())
}

/// Open a browser to the given URL, best-effort per platform.
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(url).spawn(); }
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/c", "start", "", url]).spawn(); }
}

/// Wait for SIGINT/SIGTERM and initiate graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    eprintln!("\n  Shutting down…");
}

const fn console_banner_no_workspace() -> &'static str {
    concat!(
        "╔═══════════════════════════════════════════════╗\n",
        "║  No beads workspace found in this directory   ║\n",
        "║                                               ║\n",
        "║  Run `br init` to create one, then retry.     ║\n",
        "║                                               ║\n",
        "║  Or run from a directory that has a `.beads/` ║\n",
        "║  folder, or pass --db /path/to/beads.db       ║\n",
        "╚═══════════════════════════════════════════════╝"
    )
}
