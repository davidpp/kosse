//! What a fixture process and its driver share: the line it prints on stdout once every socket is
//! bound (any driver, Rust harness or later the browser suite, waits for it instead of sleeping),
//! and what each `--serve` mode answers.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ready {
    pub pid: u32,
    /// Every listening socket, in the order the groups were requested.
    pub listeners: Vec<SocketAddr>,
}

/// What every accepted connection of a fixture gets (`--serve`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Serve {
    /// Speaks first with a non-HTTP banner, then closes.
    #[default]
    Banner,
    /// `200` HTML whose `<title>` is [`TITLE`].
    Title,
    /// `404` JSON, like an API without a root route.
    NotFound,
    /// `500`.
    Error,
    /// Reads the request and never answers.
    Hang,
    /// `200` with a 64 MiB body, streamed until the client leaves.
    Huge,
    /// `302` to `http://localhost:<own port>` + [`REDIRECT_PATH`].
    Redirect,
    /// HTTPS with a freshly generated self-signed certificate for `localhost`, answering like
    /// `title`.
    Tls,
}

impl Serve {
    #[must_use]
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Banner => "banner",
            Self::Title => "title",
            Self::NotFound => "not-found",
            Self::Error => "error",
            Self::Hang => "hang",
            Self::Huge => "huge",
            Self::Redirect => "redirect",
            Self::Tls => "tls",
        }
    }
}

/// The non-HTTP banner of [`Serve::Banner`].
pub const BANNER: &[u8] = b"kosse-fixture\n";
/// The raw `<title>` text of [`Serve::Title`] and [`Serve::Tls`] pages (an entity and markup).
pub const TITLE: &str = "Fixture &amp; &lt;Title&gt;";
/// Where [`Serve::Redirect`] points on its own port: a followed redirect would show up as a
/// recorded `GET /next…`, and the query secret must be redacted.
pub const REDIRECT_PATH: &str = "/next?token=fixture-redirect-secret";
/// Size of the [`Serve::Huge`] body.
pub const HUGE_BODY: usize = 64 * 1024 * 1024;
