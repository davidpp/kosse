//! The production web bundle, compiled into the binary so it runs without Bun or Node.

use std::path::Path;

use axum::{
    http::{HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::{EmbeddedFile, RustEmbed};

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct WebBundle;

const INDEX: &str = "index.html";
/// Vite content-hashes everything under `assets/`, so those files never change in place.
const IMMUTABLE_PREFIX: &str = "assets/";

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { INDEX } else { path };

    if let Some(file) = WebBundle::get(path) {
        return file_response(path, file);
    }
    // Client-side routes are extensionless; a missing `.js`/`.css` must 404, not return HTML.
    if Path::new(path).extension().is_some() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match WebBundle::get(INDEX) {
        Some(index) => file_response(INDEX, index),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "This build has no embedded web UI. Run `bun run build`.",
        )
            .into_response(),
    }
}

fn file_response(path: &str, file: EmbeddedFile) -> Response {
    let content_type = HeaderValue::from_str(file.metadata.mimetype())
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    let cache_control = if path.starts_with(IMMUTABLE_PREFIX) {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    } else {
        HeaderValue::from_static("no-cache")
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        file.data,
    )
        .into_response()
}
