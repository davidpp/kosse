//! The loopback API's information page. The application UI is rendered by Rune.
use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};

const INFORMATION: &str = concat!(
    r#"<!doctype html><html lang="en"><meta charset="utf-8"><title>Kossé</title>"#,
    "<h1>Kossé</h1><p>Kossé is a native application. ",
    "Run <code>kosse open</code> to show its window.</p></html>",
);

pub async fn serve(uri: Uri) -> Response {
    if uri.path() != "/" {
        return StatusCode::NOT_FOUND.into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "text/html"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        INFORMATION,
    )
        .into_response()
}
