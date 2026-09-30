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
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::{IpAddr, SocketAddr};
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
            db_path
                .parent()
                .map(|p| p.join(".beads"))
                .unwrap_or(db_path.join(".beads"))
        };
        if dir.is_dir() {
            dir
        } else {
            return Err(BeadsError::Config(format!("no .beads/ at db path")));
        }
    } else {
        let cwd = std::env::current_dir()
            .map_err(|_| BeadsError::Config("cannot get current directory".into()))?;
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

    // A non-loopback bind exposes a mutating API to the network, and this
    // server has no authentication. Refuse to start rather than hand out a
    // read-write issue tracker to the LAN; `--allow-remote` is the explicit,
    // logged opt-in.
    if !args.allow_remote && !is_loopback_host(&args.host) {
        return Err(BeadsError::Config(format!(
            "refusing to bind {}: `br web` exposes an unauthenticated read-write API. \
             Loopback is the default and is the only safe option today. Pass \
             `--allow-remote` if you have put your own authentication in front \
             of it, and put the port behind a firewall.",
            args.host
        )));
    }

    // Build the router with all API routes and static file serving.
    let app = Router::new()
        .route(
            "/",
            axum::routing::get(|| async { axum::response::Redirect::temporary("/p/default") }),
        )
        // Beads CRUD
        .route(
            "/api/p/{project_id}/beads",
            axum::routing::get(api::list_beads).post(api::create_bead),
        )
        .route(
            "/api/p/{project_id}/beads/{id}",
            axum::routing::get(api::get_bead)
                .patch(api::update_bead)
                .delete(api::delete_bead),
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
        .route(
            "/api/p/{project_id}/beads/{id}/gate",
            axum::routing::post(api::stub_created),
        )
        .route(
            "/api/p/{project_id}/beads/{id}/assist",
            axum::routing::post(api::stub_assist),
        )
        .route(
            "/api/p/{project_id}/beads/{id}/human",
            axum::routing::post(api::stub_created),
        )
        // Views
        .route(
            "/api/p/{project_id}/insights",
            axum::routing::get(api::stub_insights),
        )
        .route(
            "/api/p/{project_id}/activity",
            axum::routing::get(api::stub_empty_activity),
        )
        .route(
            "/api/p/{project_id}/gamification",
            axum::routing::get(api::stub_gamification),
        )
        // Attachments
        .route(
            "/api/p/{project_id}/attachments",
            axum::routing::post(api::stub_json),
        )
        .route(
            "/api/p/{project_id}/attachments/{*path}",
            axum::routing::post(api::stub_json).put(api::stub_json),
        )
        // Board order
        .route(
            "/api/p/{project_id}/order",
            axum::routing::get(api::stub_empty_orders).put(api::stub_empty_orders),
        )
        // Publish / showcase
        .route(
            "/api/p/{project_id}/publish",
            axum::routing::post(api::stub_json),
        )
        // Projects
        .route("/api/projects", axum::routing::get(api::list_projects))
        .route(
            "/api/projects/{id}",
            axum::routing::patch(api::stub_json).delete(api::stub_json),
        )
        // Config & diagnostics
        .route(
            "/api/p/{project_id}/doctor",
            axum::routing::get(api::doctor),
        )
        .route(
            "/api/config",
            axum::routing::get(api::get_config).put(api::update_config),
        )
        .route("/api/fs", axum::routing::get(api::stub_fs))
        // Self-update
        .route(
            "/api/update/check",
            axum::routing::get(api::stub_update_check),
        )
        .route("/api/update/run", axum::routing::post(api::stub_json))
        .fallback_service(axum::routing::get(assets::serve_static))
        // Same-origin only. The UI is served by this same router, so it never
        // needed CORS; `permissive()` was handing every origin on the internet
        // a readable channel to a database with no auth in front of it.
        .layer(tower_http::cors::CorsLayer::new())
        // CORS only governs whether a browser will hand the *response* back to
        // the calling script. It does not stop a simple cross-origin POST from
        // being sent and acted on, so the mutating half needs its own gate.
        .layer(axum::middleware::from_fn_with_state(
            args.host.clone(),
            reject_cross_origin_mutation,
        ))
        .with_state(state);

    // Bind with auto port-pick: default 3000, try +1 +2 … if busy.
    let requested = args.port.unwrap_or(3000);
    let listener = bind_first_free(&args.host, requested, args.strict_port)?;
    listener
        .set_nonblocking(true)
        .map_err(|e| BeadsError::Config(format!("nonblock: {e}")))?;
    let actual = listener
        .local_addr()
        .map_err(|e| BeadsError::Config(format!("addr: {e}")))?;

    eprintln!(
        "  br web → http://{}:{}/\n  (Ctrl+C to stop)",
        actual.ip(),
        actual.port()
    );

    if !args.no_open {
        open_browser(&format!("http://{}:{}/", actual.ip(), actual.port()));
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| BeadsError::Config(format!("Failed to start runtime: {e}")))?;

    rt.block_on(async {
        let tokio_listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|e| BeadsError::Config(format!("listener: {e}")))?;

        axum::serve(tokio_listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| BeadsError::Config(format!("Server error: {e}")))?;

        Ok::<(), BeadsError>(())
    })?;

    Ok(())
}

/// Bind to host:port. If busy and !strict, try port+1, port+2 … up to +50.
fn bind_first_free(host: &str, port: u16, strict: bool) -> Result<std::net::TcpListener> {
    let limit = if strict { 1 } else { 50 };
    for offset in 0..limit {
        let p = port + offset;
        let addr: SocketAddr = format!("{host}:{p}")
            .parse()
            .map_err(|e| BeadsError::Config(format!("invalid addr: {e}")))?;
        match std::net::TcpListener::bind(addr) {
            Ok(l) => {
                if offset > 0 {
                    eprintln!("  Port {port} busy → using {p}");
                }
                return Ok(l);
            }
            Err(_) if offset < limit - 1 => continue,
            Err(e) => {
                if strict || limit == 1 {
                    return Err(BeadsError::Config(format!(
                        "Failed to bind {host}:{port}: {e}"
                    )));
                }
                return Err(BeadsError::Config(format!(
                    "Could not find a free port near {port} (tried {host}:{port}-{}): {e}",
                    port + limit - 1
                )));
            }
        }
    }
    Err(BeadsError::Config("unreachable".into()))
}

/// Open a browser to the given URL, best-effort per platform.
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
    }
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

/// Reject a mutating request that did not come from this server's own UI.
///
/// Two independent checks, because they catch different attacks:
///
/// 1. `Origin` must match `Host`. `br web` serves its own UI, so every
///    legitimate call is same-origin. A browser on any site the user visits
///    can reach `http://127.0.0.1:3000`, and a `POST` there is delivered
///    whether or not CORS would have let the page read the reply — CORS
///    governs the response, not the delivery.
/// 2. `Host` must be the address we bound to. Without this, DNS rebinding
///    walks straight past check 1: an attacker page at `http://evil.com`
///    whose DNS flips to 127.0.0.1 sends `Host: evil.com` and
///    `Origin: http://evil.com`, which match each other perfectly.
///
/// Requests with no `Origin` are left alone: they come from curl, the MCP
/// client, or another tool on the machine, not from a page in a browser.
async fn reject_cross_origin_mutation(
    State(bind_host): State<String>,
    request: Request,
    next: Next,
) -> Response {
    let is_mutation = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );

    if is_mutation {
        let origin = request
            .headers()
            .get(axum::http::header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let host = request
            .headers()
            .get(axum::http::header::HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        if let Some(host) = host.as_deref()
            && !host_matches_bind(host, &bind_host)
        {
            return rejected("cross-origin mutation refused: Host is not this server");
        }
        if let (Some(origin), Some(host)) = (origin, host)
            && !origin_matches_host(&origin, &host)
        {
            return rejected("cross-origin mutation refused: Origin does not match Host");
        }
    }

    next.run(request).await
}

/// 403 with a JSON body, so a caller sees why rather than an empty response.
fn rejected(reason: &str) -> Response {
    let body = serde_json::json!({ "error": reason });
    (
        StatusCode::FORBIDDEN,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

/// Whether the request's `Host` is this server.
///
/// Compare hostnames rather than `host:port` strings — the port is already
/// pinned by the connection, and the bind may be written as `localhost` while
/// the browser sends the resolved `127.0.0.1`.
///
/// Three ways to qualify, all of them "this is the same machine":
/// the literal bind name; a loopback name against a loopback bind, in either
/// spelling; or a loopback address when the bind is a wildcard, because
/// `0.0.0.0` and `::` listen on every interface but are still only reached
/// locally in the default configuration.
fn host_matches_bind(host: &str, bind_host: &str) -> bool {
    /// Strip the brackets IPv6 literals carry: `[::1]:3000` -> `::1`.
    fn bare(value: &str) -> &str {
        value.trim_matches(|c| c == '[' || c == ']')
    }

    let host_name = bare(host.rsplit_once(':').map_or(host, |(name, _)| name));
    let bind_name = bare(bind_host);

    if host_name.eq_ignore_ascii_case(bind_name) {
        return true;
    }

    let host_is_local = host_name.eq_ignore_ascii_case("localhost")
        || host_name.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if !host_is_local {
        return false;
    }

    // The bind qualifies if it is loopback, or if it is a wildcard — `0.0.0.0`
    // and `::` listen on every interface, but the client still arrives on a
    // concrete address and loopback is the only one this server is reachable
    // on in the default configuration.
    bind_name.eq_ignore_ascii_case("localhost")
        || matches!(bind_name, "0.0.0.0" | "::" | "*")
        || bind_name.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// An `Origin` is same-origin when its host:port equals the request's `Host`.
///
/// `Host` may carry a `:port`; `Origin` includes a scheme, so strip it and
/// compare what is left. An `Origin` bearing a path, query or fragment is not
/// a browser `Origin` at all — browsers send only scheme and authority — so it
/// is rejected rather than normalised.
fn origin_matches_host(origin: &str, host: &str) -> bool {
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if scheme.is_empty() {
        return false;
    }
    if rest.contains(['/', '?', '#']) {
        return false;
    }
    rest.eq_ignore_ascii_case(host.trim())
}

/// Whether a bind address keeps the server on this machine.
fn is_loopback_host(host: &str) -> bool {
    // `localhost` and the unspecified-but-loopback spellings. A name other
    // than localhost is not assumed safe: it may resolve off-box.
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{host_matches_bind, is_loopback_host, origin_matches_host};

    #[test]
    fn same_origin_is_allowed() {
        assert!(origin_matches_host(
            "http://127.0.0.1:3000",
            "127.0.0.1:3000"
        ));
        assert!(origin_matches_host(
            "http://localhost:3000",
            "localhost:3000"
        ));
        // Scheme case is not significant, host case is not either.
        assert!(origin_matches_host(
            "HTTP://LocalHost:3000",
            "localhost:3000"
        ));
        // A browser sends no path in Origin; a hand-rolled client that adds
        // one must not slip past the check by appending it.
        assert!(!origin_matches_host(
            "http://127.0.0.1:3000/../elsewhere",
            "127.0.0.1:3000"
        ));
    }

    #[test]
    fn cross_origin_is_rejected() {
        // The case that motivated the guard: a page on an unrelated site
        // posting to the local server.
        assert!(!origin_matches_host(
            "https://evil.example",
            "127.0.0.1:3000"
        ));
        // Same host, different port is a different origin.
        assert!(!origin_matches_host(
            "http://127.0.0.1:3001",
            "127.0.0.1:3000"
        ));
        // A missing or malformed scheme is not a pass.
        assert!(!origin_matches_host("evil.example", "evil.example"));
        assert!(!origin_matches_host("://127.0.0.1:3000", "127.0.0.1:3000"));
        assert!(!origin_matches_host("", "127.0.0.1:3000"));
    }

    #[test]
    fn host_must_be_the_address_we_bound_to() {
        // The plain case: the browser's Host is what we bound.
        assert!(host_matches_bind("127.0.0.1:3000", "127.0.0.1"));
        assert!(host_matches_bind("localhost:3000", "127.0.0.1"));
        // DNS rebinding: the attacker's name resolves here, so Origin and Host
        // agree with each other and only the bind check catches it.
        assert!(!host_matches_bind("evil.example:3000", "127.0.0.1"));
        assert!(!host_matches_bind("evil.example", "127.0.0.1"));
        // A wildcard bind is still reached on a concrete address.
        assert!(host_matches_bind("127.0.0.1:3000", "0.0.0.0"));
        assert!(host_matches_bind("[::1]:3000", "::"));
        assert!(!host_matches_bind("10.0.0.5:3000", "0.0.0.0"));
        // A host with no port at all.
        assert!(host_matches_bind("127.0.0.1", "127.0.0.1"));
    }

    #[test]
    fn only_loopback_binds_are_considered_safe() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.0.0.5"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("::"));
        // A name that is not localhost may resolve off-box, so it is not
        // assumed safe.
        assert!(!is_loopback_host("br.internal"));
    }
}
