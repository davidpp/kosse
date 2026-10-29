//! The request policy through the real binary over raw HTTP: forged `Host`, cross-origin reads,
//! preflights, cross-site form posts, missing and stale tokens, bad payloads, the dev authority,
//! and the security headers on every kind of route.

mod support;

use kosse_core::model::Snapshot;
use support::{HttpResponse, RawRequest, Server};

const DEV_AUTHORITY: &str = "127.0.0.1:47833";

fn assert_refused(response: &HttpResponse, status: u16, code: &str) {
    // Bodies are truncated in messages: a wrongly accepted request returns real host data.
    let excerpt: String = response.text().chars().take(160).collect();
    assert_eq!(response.status, status, "{excerpt}");
    assert_eq!(response.error_code(), code, "{excerpt}");
    assert_eq!(response.header("access-control-allow-origin"), None);
}

#[test]
fn every_kind_of_route_carries_the_security_headers() {
    let server = Server::start();
    let index = server.get("/");
    let refused = server.send(&RawRequest::new("GET", "/").host("evil.example"));
    for (what, response) in [
        ("index", index),
        ("client route", server.get("/services/example")),
        ("asset", server.get("/assets/removed.js")),
        ("license notice", server.get("/.vite/license.md")),
        ("api", server.get("/api/health")),
        ("api 404", server.get("/api/nope")),
        ("refusal", refused),
    ] {
        let csp = response
            .header("content-security-policy")
            .unwrap_or_else(|| panic!("{what}: no CSP"));
        for directive in [
            "default-src 'self'",
            "script-src 'self'",
            "object-src 'none'",
            "base-uri 'none'",
            "frame-ancestors 'none'",
            "form-action 'self'",
        ] {
            assert!(csp.contains(directive), "{what}: {csp}");
        }
        assert!(
            !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
            "{what}: {csp}"
        );
        assert!(
            !csp.contains("http:") && !csp.contains("https:"),
            "{what}: no remote sources"
        );
        assert_eq!(
            response.header("x-content-type-options"),
            Some("nosniff"),
            "{what}"
        );
        assert_eq!(
            response.header("referrer-policy"),
            Some("no-referrer"),
            "{what}"
        );
        assert_eq!(response.header("x-frame-options"), Some("DENY"), "{what}");
        assert_eq!(
            response.header("access-control-allow-origin"),
            None,
            "{what}"
        );
    }
}

#[test]
fn the_api_information_page_has_no_scripts() {
    let server = Server::start();
    let html = server.get("/").text();
    assert!(html.contains("kosse open"));
    assert!(!html.contains("<script"));
}

#[test]
fn foreign_hosts_are_refused_on_every_route() {
    let server = Server::start();
    let port = server.addr.port();
    for path in [
        "/",
        "/services/example",
        "/.vite/license.md",
        "/api/snapshot",
        "/api/session",
    ] {
        for host in [
            "evil.example".to_owned(),
            format!("evil.example:{port}"),
            format!("127.0.0.1.evil.example:{port}"),
            format!("127.0.0.2:{port}"),
            format!("[::1]:{port}"),
            format!("127.0.0.1:{}", port.wrapping_add(1)),
            "127.0.0.1".to_owned(),
            DEV_AUTHORITY.to_owned(),
        ] {
            let response = server.send(&RawRequest::new("GET", path).host(&host));
            assert_refused(&response, 421, "hostNotAllowed");
            assert!(
                !response.text().contains("observation"),
                "{path} leaked to {host}"
            );
        }
    }
    // Missing, duplicated, and an absolute request target naming another host.
    let missing = server.send(&RawRequest::new("GET", "/api/snapshot").http10());
    assert_refused(&missing, 421, "hostNotAllowed");
    let duplicated = server.send(
        &RawRequest::new("GET", "/api/snapshot")
            .host(&server.authority())
            .host("evil.example"),
    );
    assert_refused(&duplicated, 421, "hostNotAllowed");
    let absolute = server.send(
        &RawRequest::new("GET", "http://evil.example/api/snapshot").host(&server.authority()),
    );
    assert_refused(&absolute, 421, "hostNotAllowed");
    // Both loopback names of this instance are accepted, in any letter case.
    for host in [
        server.authority(),
        format!("localhost:{port}"),
        format!("LocalHost:{port}"),
    ] {
        assert_eq!(
            server
                .send(&RawRequest::new("GET", "/api/health").host(&host))
                .status,
            200,
            "{host}"
        );
    }
}

#[test]
fn cross_origin_reads_of_the_api_are_refused_but_navigation_works() {
    let server = Server::start();
    let host = server.authority();
    for path in ["/api/session", "/api/snapshot", "/api/events"] {
        for (name, value) in [
            ("Origin", "https://evil.example"),
            ("Origin", "null"),
            (
                "Origin",
                &format!("http://localhost:{}", server.addr.port()),
            ),
            ("Sec-Fetch-Site", "cross-site"),
            ("Sec-Fetch-Site", "same-site"),
        ] {
            let response =
                server.send(&RawRequest::new("GET", path).host(&host).header(name, value));
            assert_refused(&response, 403, "originNotAllowed");
            assert!(
                !response.text().contains("token\""),
                "{path}: {name}: {value}"
            );
        }
    }
    // A link to the dashboard from another site is a plain navigation, and works.
    let navigation = server.send(
        &RawRequest::new("GET", "/")
            .host(&host)
            .header("Sec-Fetch-Site", "cross-site")
            .header("Sec-Fetch-Mode", "navigate"),
    );
    assert_eq!(navigation.status, 200);
    // The dashboard's own fetches (same-origin, or no fetch metadata at all).
    let same_origin = server.send(
        &RawRequest::new("GET", "/api/session")
            .host(&host)
            .header("Origin", &server.origin())
            .header("Sec-Fetch-Site", "same-origin"),
    );
    assert_eq!(same_origin.status, 200);
    assert_eq!(same_origin.header("cache-control"), Some("no-store"));
}

#[test]
fn preflights_get_no_cors_permission() {
    let server = Server::start();
    let response = server.send(
        &RawRequest::new("OPTIONS", "/api/refresh")
            .host(&server.authority())
            .header("Origin", "https://evil.example")
            .header("Access-Control-Request-Method", "POST")
            .header(
                "Access-Control-Request-Headers",
                "content-type,x-kosse-token",
            ),
    );
    assert_refused(&response, 403, "originNotAllowed");
    assert_eq!(response.header("access-control-allow-methods"), None);
    assert_eq!(response.header("access-control-allow-headers"), None);
}

#[test]
fn unsafe_requests_need_exact_origin_json_and_the_current_token() {
    let server = Server::start();
    let token = server.token();
    let base = || {
        RawRequest::new("POST", "/api/refresh")
            .host(&server.authority())
            .body("{}")
    };
    let json = |request: RawRequest| request.header("Content-Type", "application/json");
    let with_token = |request: RawRequest| request.header("X-Kosse-Token", &token);
    let origin = server.origin();

    // Origin: missing, opaque, foreign, the other loopback name, a different port.
    assert_refused(
        &server.send(&with_token(json(base()))),
        403,
        "originNotAllowed",
    );
    for bad in [
        "null".to_owned(),
        "https://evil.example".to_owned(),
        format!("http://localhost:{}", server.addr.port()),
        format!("https://{}", server.authority()),
        format!("http://127.0.0.1:{}", server.addr.port().wrapping_add(1)),
    ] {
        let response = server.send(&with_token(json(base().header("Origin", &bad))));
        assert_refused(&response, 403, "originNotAllowed");
    }
    // Cross-site forms (all CORS-safelisted content types) and other types.
    for content_type in [
        "application/x-www-form-urlencoded",
        "multipart/form-data; boundary=x",
        "text/plain",
        "application/json-patch+json",
    ] {
        let response = server.send(&with_token(
            base()
                .header("Origin", &origin)
                .header("Content-Type", content_type),
        ));
        assert_refused(&response, 415, "unsupportedMediaType");
    }
    let no_type = server.send(&with_token(base().header("Origin", &origin)));
    assert_refused(&no_type, 415, "unsupportedMediaType");
    // Tokens: missing, wrong, and the token of another (earlier) instance.
    assert_refused(
        &server.send(&json(base().header("Origin", &origin))),
        403,
        "tokenRequired",
    );
    let wrong = json(base().header("Origin", &origin)).header("X-Kosse-Token", "0");
    assert_refused(&server.send(&wrong), 403, "tokenRequired");
    let stale = Server::start().token();
    let stale = json(base().header("Origin", &origin)).header("X-Kosse-Token", &stale);
    assert_refused(&server.send(&stale), 403, "tokenRequired");
    // The token is never accepted from the URL.
    let in_query = server.send(&json(
        RawRequest::new("POST", &format!("/api/refresh?token={token}"))
            .host(&server.authority())
            .header("Origin", &origin)
            .body("{}"),
    ));
    assert_refused(&in_query, 403, "tokenRequired");
}

#[test]
fn payloads_are_bounded_and_validated() {
    let server = Server::start();
    let token = server.token();
    for body in ["", "not json", "[]", r#"{"unexpected":1}"#] {
        let response = server.post_json("/api/refresh", &token, body);
        assert_refused(&response, 400, "invalidPayload");
    }
    let huge = format!(r#"{{"padding":"{}"}}"#, "x".repeat(20 * 1024));
    assert_refused(
        &server.post_json("/api/refresh", &token, &huge),
        413,
        "payloadTooLarge",
    );
    // Without Content-Length the limit applies while reading, with the same answer.
    let chunked = server.send(
        &RawRequest::new("POST", "/api/refresh")
            .host(&server.authority())
            .header("Origin", &server.origin())
            .header("Content-Type", "application/json")
            .header("X-Kosse-Token", &token)
            .chunked(&huge),
    );
    assert_refused(&chunked, 413, "payloadTooLarge");
}

#[test]
fn intended_same_origin_refresh_scans_and_is_rate_limited() {
    let server = Server::start();
    let before = server.snapshot();
    let token = server.token();

    let refreshed = server.post_json("/api/refresh", &token, "{}");
    assert_eq!(refreshed.status, 200, "{}", refreshed.text());
    let refreshed: Snapshot = serde_json::from_slice(&refreshed.body).expect("snapshot");
    assert_eq!(refreshed.instance, before.instance);
    assert!(refreshed.revision > before.revision);

    let limited = server.post_json("/api/refresh", &token, "{}");
    assert_refused(&limited, 429, "rateLimited");
    assert_eq!(limited.header("retry-after"), Some("1"));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(server.post_json("/api/refresh", &token, "{}").status, 200);
}

#[test]
fn unsafe_requests_to_the_bundle_pass_the_same_policy() {
    let server = Server::start();
    let response = server.send(
        &RawRequest::new("POST", "/")
            .host(&server.authority())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Origin", "https://evil.example")
            .body("a=1"),
    );
    assert_refused(&response, 403, "originNotAllowed");
}

#[test]
fn the_dev_authority_is_admitted_only_in_dev_mode() {
    let production = Server::start();
    let refused = production.send(&RawRequest::new("GET", "/api/health").host(DEV_AUTHORITY));
    assert_refused(&refused, 421, "hostNotAllowed");

    // Through the Vite proxy the browser's Host and Origin are the dev server's.
    let dev = Server::start_with(&["--dev"]);
    let token = dev.token();
    let health = dev.send(&RawRequest::new("GET", "/api/health").host(DEV_AUTHORITY));
    assert_eq!(health.status, 200);
    let refresh = dev.send(
        &RawRequest::new("POST", "/api/refresh")
            .host(DEV_AUTHORITY)
            .header("Origin", &format!("http://{DEV_AUTHORITY}"))
            .header("Content-Type", "application/json")
            .header("X-Kosse-Token", &token)
            .body("{}"),
    );
    assert_eq!(refresh.status, 200, "{}", refresh.text());
    // Dev mode admits exactly that authority, and still refuses cross-origin writes.
    let mismatch = dev.send(
        &RawRequest::new("POST", "/api/refresh")
            .host(DEV_AUTHORITY)
            .header("Origin", &dev.origin())
            .header("Content-Type", "application/json")
            .header("X-Kosse-Token", &token)
            .body("{}"),
    );
    assert_refused(&mismatch, 403, "originNotAllowed");
    let localhost = dev.send(&RawRequest::new("GET", "/api/health").host("localhost:47833"));
    assert_refused(&localhost, 421, "hostNotAllowed");
}
