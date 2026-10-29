//! The request policy every route passes through, the embedded web bundle included.
//!
//! Defends against websites in the user's browser (cross-site requests and DNS rebinding), not
//! against other local processes, which can read the snapshot through the OS anyway:
//!
//! - `Host` (and an absolute request target's authority) must be exactly this instance's loopback
//!   authority: `127.0.0.1:<port>` or `localhost:<port>`, plus `127.0.0.1:47833` only in
//!   development mode (the Vite dev server, which proxies `/api` here).
//! - API requests that a browser marks as cross-origin (`Origin`, `Sec-Fetch-Site`) are refused,
//!   so no other site can read or embed API responses. Top-level navigation to the dashboard from
//!   elsewhere still works.
//! - Unsafe methods additionally need the exact same-origin `Origin`, `Content-Type:
//!   application/json`, a body of at most [`MAX_BODY_BYTES`], and the per-instance action token
//!   in [`TOKEN_HEADER`]. No CORS headers are ever sent.
//! - Every response carries a strict Content Security Policy and related headers.

use std::{fmt::Write as _, fs::File, io, io::Read as _, sync::Arc};

use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use kosse_core::contract::{ApiError, ApiErrorCode};

/// The header that carries the action token on unsafe requests.
pub const TOKEN_HEADER: &str = "x-kosse-token";
/// Largest request body accepted (and the router's body limit).
pub const MAX_BODY_BYTES: usize = 16 * 1024;
/// The Vite dev server's authority (`web/dev-ports.ts`), admitted only in development mode.
pub const DEV_AUTHORITY: &str = "127.0.0.1:47833";

/// No inline or remote script, style, font or frame; the API and assets are same-origin only.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
    img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; \
    base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// Which authorities this instance answers for, and the action token.
pub struct Policy {
    authorities: Vec<String>,
    token: ActionToken,
}

impl Policy {
    /// `dev` adds [`DEV_AUTHORITY`]; it is only reachable from debug builds' `--dev`.
    pub fn new(port: u16, dev: bool, token: ActionToken) -> Self {
        let mut authorities = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
        if dev {
            authorities.push(DEV_AUTHORITY.to_owned());
        }
        Self { authorities, token }
    }

    pub fn token(&self) -> &str {
        &self.token.0
    }

    fn allowed_authority(&self, authority: &str) -> Option<&str> {
        self.authorities
            .iter()
            .find(|allowed| allowed.eq_ignore_ascii_case(authority))
            .map(String::as_str)
    }
}

/// 32 random bytes from the kernel, hex-encoded. Lives only in memory.
pub struct ActionToken(String);

impl ActionToken {
    pub fn generate() -> io::Result<Self> {
        let mut bytes = [0u8; 32];
        File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        let mut hex = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(hex, "{byte:02x}");
        }
        Ok(Self(hex))
    }
}

/// Applies the policy, then adds the security headers to whatever the route (or the refusal)
/// returned.
pub async fn enforce(State(policy): State<Arc<Policy>>, request: Request, next: Next) -> Response {
    let is_api = {
        let path = request.uri().path();
        path == "/api" || path.starts_with("/api/")
    };
    let mut response = match check(&policy, &request, is_api) {
        Ok(()) => next.run(request).await,
        Err(refusal) => refusal.into_response(),
    };
    let headers = response.headers_mut();
    for (name, value) in [
        (
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CONTENT_SECURITY_POLICY),
        ),
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        (
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ),
        (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
        (
            header::HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static("same-origin"),
        ),
        (
            header::HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        ),
    ] {
        headers.insert(name, value);
    }
    if is_api && !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

/// A refused request: status plus the [`ApiError`] body.
pub struct Refusal {
    status: StatusCode,
    code: ApiErrorCode,
    message: &'static str,
}

impl Refusal {
    pub const fn new(status: StatusCode, code: ApiErrorCode, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
        }
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiError {
                error: self.code,
                message: self.message.to_owned(),
            }),
        )
            .into_response()
    }
}

fn check(policy: &Policy, request: &Request, is_api: bool) -> Result<(), Refusal> {
    let headers = request.headers();
    let host = single(headers, header::HOST)
        .and_then(|host| policy.allowed_authority(host))
        .ok_or(Refusal::new(
            StatusCode::MISDIRECTED_REQUEST,
            ApiErrorCode::HostNotAllowed,
            "this Kossé instance only answers for its own loopback address",
        ))?;
    if let Some(authority) = request.uri().authority()
        && !authority.as_str().eq_ignore_ascii_case(host)
    {
        return Err(Refusal::new(
            StatusCode::MISDIRECTED_REQUEST,
            ApiErrorCode::HostNotAllowed,
            "the request target names another host",
        ));
    }

    let same_origin = format!("http://{host}");
    let origin = single(headers, header::ORIGIN);
    let fetch_site = single(headers, "sec-fetch-site");
    let unsafe_method = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let cross_origin = origin.is_some_and(|origin| origin != same_origin)
        || fetch_site.is_some_and(|site| site != "same-origin" && site != "none");
    if (is_api || unsafe_method) && cross_origin {
        return Err(Refusal::new(
            StatusCode::FORBIDDEN,
            ApiErrorCode::OriginNotAllowed,
            "cross-origin requests are not allowed",
        ));
    }
    if !unsafe_method {
        return Ok(());
    }

    if origin != Some(same_origin.as_str()) || fetch_site == Some("none") {
        return Err(Refusal::new(
            StatusCode::FORBIDDEN,
            ApiErrorCode::OriginNotAllowed,
            "unsafe requests need this instance's exact Origin",
        ));
    }
    let is_json = single(headers, header::CONTENT_TYPE).is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
    });
    if !is_json {
        return Err(Refusal::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ApiErrorCode::UnsupportedMediaType,
            "send Content-Type: application/json",
        ));
    }
    let too_large = single(headers, header::CONTENT_LENGTH)
        .and_then(|length| length.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_BODY_BYTES);
    if too_large {
        return Err(Refusal::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            ApiErrorCode::PayloadTooLarge,
            "request body too large",
        ));
    }
    let token_matches = single(headers, TOKEN_HEADER)
        .is_some_and(|token| constant_time_eq(token.as_bytes(), policy.token().as_bytes()));
    if !token_matches {
        return Err(Refusal::new(
            StatusCode::FORBIDDEN,
            ApiErrorCode::TokenRequired,
            "missing or stale action token; fetch /api/session again",
        ));
    }
    Ok(())
}

/// The header's value if it appears exactly once and is visible ASCII.
fn single(headers: &HeaderMap, name: impl header::AsHeaderName) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    if values.next().is_some() {
        return None;
    }
    first.to_str().ok()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_compared_exactly() {
        let first = ActionToken::generate().expect("token");
        let second = ActionToken::generate().expect("token");
        assert_eq!(first.0.len(), 64);
        assert_ne!(first.0, second.0);
        assert!(constant_time_eq(first.0.as_bytes(), first.0.as_bytes()));
        assert!(!constant_time_eq(first.0.as_bytes(), second.0.as_bytes()));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
