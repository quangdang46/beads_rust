//! Static file serving for the embedded web UI.
//!
//! Uses `rust-embed` to include the prebuilt Next.js static export at compile
//! time. All non-file, non-API requests fall back to `index.html` for SPA
//! client-side routing.

use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

/// Embedded static files from the Next.js build output.
#[derive(RustEmbed)]
#[folder = "src/web/static/"]
struct WebAssets;

/// Serve a static file, or fall back to `index.html` for SPA routing.
pub async fn serve_static(req: Request) -> impl IntoResponse {
    let uri = req.uri().clone();
    let path = uri.path().trim_start_matches('/');

    // If the path is empty, serve index.html.
    if path.is_empty() {
        return serve_file("index.html").await;
    }

    // Try to serve the exact file.
    if let Some(resp) = try_serve_file(path).await {
        return resp;
    }

    // If the path looks like a file (has an extension), return 404.
    if path.contains('.') {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain")],
            "Not found",
        )
            .into_response();
    }

    // Otherwise, it's a client-side route — serve index.html for SPA routing.
    serve_file("index.html").await
}

async fn serve_file(path: &str) -> Response {
    match WebAssets::get(path) {
        Some(content) => {
            let mime = mime_type(path);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::CACHE_CONTROL, cache_header(path))
                .body(Body::from(content.data.to_vec()))
                .unwrap_or_else(|_| error_response())
        }
        None => error_response(),
    }
}

async fn try_serve_file(path: &str) -> Option<Response> {
    let asset = WebAssets::get(path)?;
    let mime = mime_type(path);
    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, cache_header(path))
            .body(Body::from(asset.data.to_vec()))
            .unwrap_or_else(|_| error_response()),
    )
}

fn error_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/plain")],
        "Not found",
    )
        .into_response()
}

/// Determine the MIME type based on the file extension.
fn mime_type(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".gif") {
        "image/gif"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".woff") {
        "font/woff"
    } else if path.ends_with(".ttf") {
        "font/ttf"
    } else if path.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else if path.ends_with(".xml") {
        "application/xml"
    } else {
        "application/octet-stream"
    }
}

/// Determine cache-control header.
/// Immutable build artifacts get long cache, HTML gets no-cache.
fn cache_header(path: &str) -> &'static str {
    if path.starts_with("_next/static/") || path.starts_with("_next/") {
        "public, max-age=31536000, immutable"
    } else if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=3600"
    }
}
