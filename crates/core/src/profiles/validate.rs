//! The rules a profile must satisfy beyond its JSON shape. Repository files are untrusted input,
//! so a profile that breaks any rule is refused as a whole (the last good one stays in use) rather
//! than partly applied.
//!
//! - Links are `http`/`https` with an ASCII host and no credentials, backslashes or
//!   percent-encoded authority; the host is classified as loopback (`localhost`, `*.localhost`,
//!   `127.0.0.0/8`, `::1`) or declared. Nothing here resolves or contacts a host.
//! - Probe paths are origin-relative (`/…`, never `//…`), visible ASCII, without `\` or `#`.
//! - Match directories are relative, `/`-separated, without `..`.

use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr},
};

use crate::profiles::{
    document::{
        LinkKind, MAX_LABEL_CHARS, MAX_LINKS, MAX_PATH_BYTES, MAX_SERVICES, MAX_URL_BYTES,
        PROFILE_VERSION, ProjectProfile, ServiceMatch, ServiceProfile,
    },
    view::LinkReach,
};

/// Why a profile was refused, and where in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError {
    /// A JSON-path-like location such as `services.web.links[0].url`; empty for the whole file.
    pub at: String,
    pub problem: String,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.at.is_empty() {
            f.write_str(&self.problem)
        } else {
            write!(f, "{}: {}", self.at, self.problem)
        }
    }
}

impl std::error::Error for ProfileError {}

fn refuse(at: impl Into<String>, problem: impl Into<String>) -> ProfileError {
    ProfileError {
        at: at.into(),
        problem: problem.into(),
    }
}

/// Checks every rule; the first violation is reported.
pub fn profile(profile: &ProjectProfile) -> Result<(), ProfileError> {
    problems(profile).into_iter().next().map_or(Ok(()), Err)
}

/// Every rule `profile` breaks, in document order; empty when it is valid. Any one refuses the
/// whole profile.
#[must_use]
pub fn problems(profile: &ProjectProfile) -> Vec<ProfileError> {
    let mut found = Vec::new();
    let mut check = |at: &str, result: Result<(), String>| {
        if let Err(problem) = result {
            found.push(refuse(at, problem));
        }
    };
    if profile.version != PROFILE_VERSION {
        check(
            "version",
            Err(format!(
                "must be {PROFILE_VERSION}, not {}",
                profile.version
            )),
        );
    }
    if profile
        .schema
        .as_ref()
        .is_some_and(|schema| schema.len() > MAX_URL_BYTES)
    {
        check("$schema", Err("too long".into()));
    }
    if let Some(name) = &profile.name {
        check("name", label(name));
    }
    if profile.services.len() > MAX_SERVICES {
        check("services", Err(format!("at most {MAX_SERVICES} services")));
    }
    for (id, service) in &profile.services {
        let at = format!("services.{id}");
        check(&at, service_id(id));
        service_profile(service, &at, &mut check);
    }
    found
}

fn service_profile(
    service: &ServiceProfile,
    at: &str,
    check: &mut impl FnMut(&str, Result<(), String>),
) {
    if let Some(name) = &service.name {
        check(&format!("{at}.name"), label(name));
    }
    if let Some(matcher) = &service.matcher {
        service_match(matcher, &format!("{at}.match"), check);
    }
    if let Some(path) = &service.probe_path {
        check(&format!("{at}.probePath"), probe_path(path));
    }
    if let Some(links) = &service.links {
        if links.len() > MAX_LINKS {
            check(
                &format!("{at}.links"),
                Err(format!("at most {MAX_LINKS} links")),
            );
        }
        for (index, link) in links.iter().enumerate() {
            let at = format!("{at}.links[{index}]");
            check(&format!("{at}.label"), label(&link.label));
            check(&format!("{at}.url"), link_url(&link.url).map(|_| ()));
        }
    }
}

fn service_match(
    matcher: &ServiceMatch,
    at: &str,
    check: &mut impl FnMut(&str, Result<(), String>),
) {
    if matcher.cwd.is_none() && matcher.port.is_none() {
        check(at, Err("needs `cwd`, `port` or both".into()));
    }
    if matcher.port == Some(0) {
        check(&format!("{at}.port"), Err("must be 1–65535".into()));
    }
    if let Some(cwd) = &matcher.cwd {
        check(&format!("{at}.cwd"), relative_dir(cwd).map(|_| ()));
    }
}

fn service_id(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if first_ok && rest_ok && id.len() <= 64 {
        Ok(())
    } else {
        Err("a service ID is 1–64 ASCII letters, digits, `.`, `_` or `-`, starting with a letter or digit".into())
    }
}

/// A display name: 1–100 characters, not blank, no control or invisible format characters.
pub fn label(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("must not be empty".into());
    }
    if text.chars().count() > MAX_LABEL_CHARS {
        return Err(format!("at most {MAX_LABEL_CHARS} characters"));
    }
    if text
        .chars()
        .any(|c| c.is_control() || is_invisible_format(c))
    {
        return Err("must not contain control or invisible formatting characters".into());
    }
    Ok(())
}

/// Bidi controls, zero-width characters, line separators and the BOM: they can disguise text.
pub(crate) fn is_invisible_format(character: char) -> bool {
    matches!(
        character,
        '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// An origin-relative path for the HTTP probe.
///
/// RFC 3986 `path-absolute [ "?" query ]` without a fragment: unreserved and sub-delimiter
/// characters, `:`, `@`, `/` (and `?` in the query) or `%XX`. That is a strict subset of what
/// `http::uri::PathAndQuery` accepts, so a valid path always forms a request; anything else (`<`,
/// `>`, `` ` ``, `"`, `{`, `\`, spaces, non-ASCII) must be percent-encoded.
pub fn probe_path(path: &str) -> Result<(), String> {
    if path.len() > MAX_PATH_BYTES {
        return Err(format!("at most {MAX_PATH_BYTES} bytes"));
    }
    if !path.starts_with('/') || path.starts_with("//") {
        return Err("must be an origin-relative path starting with a single `/`".into());
    }
    let (path_part, query) = match path.split_once('?') {
        Some((path_part, query)) => (path_part, Some(query)),
        None => (path, None),
    };
    let valid = |text: &str, extra: &[u8]| {
        let bytes = text.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte == b'%' {
                let hex = bytes.get(index + 1..index + 3);
                if !hex.is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit)) {
                    return false;
                }
                index += 3;
                continue;
            }
            let allowed = byte.is_ascii_alphanumeric()
                || b"-._~!$&'()*+,;=:@/".contains(&byte)
                || extra.contains(&byte);
            if !allowed {
                return false;
            }
            index += 1;
        }
        true
    };
    if !valid(path_part, b"") || !query.is_none_or(|query| valid(query, b"?")) {
        return Err(
            "only letters, digits, `-._~!$&'()*+,;=:@/`, `?` for the query and `%XX` escapes are allowed"
                .into(),
        );
    }
    Ok(())
}

/// A match directory, relative to the project root. Returns its normal components (`.` removed).
pub fn relative_dir(cwd: &str) -> Result<Vec<&str>, String> {
    if cwd.len() > MAX_PATH_BYTES {
        return Err(format!("at most {MAX_PATH_BYTES} bytes"));
    }
    if cwd.is_empty() || cwd.starts_with('/') || cwd.contains(['\\', '\0']) {
        return Err(
            "must be a `/`-separated path relative to the project root (`.` for the root)".into(),
        );
    }
    let mut parts = Vec::new();
    for part in cwd.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err("must not contain `..`: matches stay inside the project".into()),
            part => parts.push(part),
        }
    }
    Ok(parts)
}

/// A link URL accepted for display, with its host made visible and classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkUrl {
    /// The URL with its scheme and host lowercased.
    pub url: String,
    pub host: String,
    pub reach: LinkReach,
}

impl LinkUrl {
    /// The kind to show when the profile does not say.
    #[must_use]
    pub fn default_kind(&self) -> LinkKind {
        match self.reach {
            LinkReach::Loopback => LinkKind::Local,
            LinkReach::Declared => LinkKind::External,
        }
    }
}

/// Parses a link URL; see the module documentation for the rules.
pub fn link_url(url: &str) -> Result<LinkUrl, String> {
    if url.len() > MAX_URL_BYTES {
        return Err(format!("at most {MAX_URL_BYTES} bytes"));
    }
    if !url.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(
            "must be visible ASCII without spaces (use punycode for international domains)".into(),
        );
    }
    if url.contains('\\') {
        return Err("must not contain `\\`".into());
    }
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| "must be an absolute http:// or https:// URL".to_owned())?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err("only http:// and https:// links are allowed".into());
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.contains('@') {
        return Err("must not contain credentials".into());
    }
    if authority.contains('%') {
        return Err("the host must not be percent-encoded".into());
    }
    let (host, port) = split_host_port(authority)?;
    if let Some(port) = port {
        match port.parse::<u16>() {
            Ok(1..) => {}
            _ => return Err("the port must be 1–65535".into()),
        }
    }
    let host = host.to_ascii_lowercase();
    let reach = classify_host(&host)?;
    let mut normalized = format!("{scheme}://{host}");
    if let Some(port) = port {
        normalized.push(':');
        normalized.push_str(port);
    }
    normalized.push_str(tail);
    Ok(LinkUrl {
        url: normalized,
        host,
        reach,
    })
}

fn split_host_port(authority: &str) -> Result<(&str, Option<&str>), String> {
    if authority.is_empty() {
        return Err("must name a host".into());
    }
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (literal, after) = bracketed
            .split_once(']')
            .ok_or_else(|| "unterminated IPv6 address".to_owned())?;
        let host = &authority[..literal.len() + 2];
        return match after {
            "" => Ok((host, None)),
            port if port.starts_with(':') => Ok((host, Some(&port[1..]))),
            _ => Err("unexpected text after the IPv6 address".into()),
        };
    }
    Ok(match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    })
}

fn classify_host(host: &str) -> Result<LinkReach, String> {
    if let Some(literal) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let ip: Ipv6Addr = literal
            .parse()
            .map_err(|_| "not a valid IPv6 address".to_owned())?;
        return Ok(if ip.is_loopback() {
            LinkReach::Loopback
        } else {
            LinkReach::Declared
        });
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(if ip.is_loopback() {
            LinkReach::Loopback
        } else {
            LinkReach::Declared
        });
    }
    let name = host.strip_suffix('.').unwrap_or(host);
    let valid_label = |label: &str| {
        (1..=63).contains(&label.len())
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    };
    if name.is_empty() || name.len() > 253 || !name.split('.').all(valid_label) {
        return Err("not a valid host name".into());
    }
    // RFC 6761: `localhost` and every name under it are loopback.
    Ok(if name == "localhost" || name.ends_with(".localhost") {
        LinkReach::Loopback
    } else {
        LinkReach::Declared
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_link_urls_are_refused() {
        for url in [
            "javascript:alert(1)",
            "JaVaScRiPt://localhost/%0aalert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "vscode://open?file=/etc/passwd",
            "//localhost:3000/",
            "http://user:secret@localhost:3000/",
            "http://localhost@evil.example/",
            "http://evil.example\\@localhost/",
            "http://ex ample.com/",
            "http://exa%6dple.com/",
            "http://xn--80ak6aa92e.com\u{0430}/",
            "http://bücher.example/",
            "http:///path",
            "http://:80/",
            "http://localhost:0/",
            "http://localhost:99999/",
            "http://[::1/",
            "http://[zz::1]/",
            "http://-bad-.example/",
            "http://localhost:3000/\u{0}",
        ] {
            assert!(link_url(url).is_err(), "{url:?} must be refused");
        }
    }

    #[test]
    fn hosts_are_lowercased_and_classified_without_resolution() {
        let loopback = |url: &str| link_url(url).expect(url).reach == LinkReach::Loopback;
        assert!(loopback("http://localhost:3001/docs"));
        assert!(loopback("HTTPS://Acme.LOCALHOST"));
        assert!(loopback("http://127.0.0.2:8080/"));
        assert!(loopback("http://[::1]:5173/"));
        assert!(
            !loopback("https://acme.local/"),
            "LocalCan .local is declared"
        );
        assert!(!loopback("https://staging.example.com/a?b#c"));
        assert!(!loopback("http://localhost.example.com/"));
        assert!(!loopback("http://192.168.1.10:3000/"));

        let parsed = link_url("HTTPS://Acme.LOCALHOST:8443/Path?Q=1").expect("valid");
        assert_eq!(parsed.url, "https://acme.localhost:8443/Path?Q=1");
        assert_eq!(parsed.host, "acme.localhost");
        assert_eq!(parsed.default_kind(), LinkKind::Local);
    }

    #[test]
    fn probe_paths_are_origin_relative() {
        for good in [
            "/",
            "/health",
            "/api/health?ready=1",
            "/%7Euser",
            "/?probe",
            "/a/b:c@d;e=f?x=/y?z",
        ] {
            assert_eq!(probe_path(good), Ok(()), "{good}");
        }
        for bad in [
            "",
            "health",
            "//evil.example/",
            "http://localhost/",
            "/a\\b",
            "/a#b",
            "/a b",
            "/é",
            "/\r\nX-Injected: 1",
            // Accepted by a naive visible-ASCII check, refused by the HTTP request builder.
            "/<script>",
            "/a>b",
            "/`id`",
            "/a\"b",
            "/{x}",
            "/a|b",
            "/a^b",
            "/a[0]",
            "/%zz",
            "/%4",
        ] {
            assert!(probe_path(bad).is_err(), "{bad:?}");
        }
        assert!(probe_path(&format!("/{}", "a".repeat(600))).is_err());
    }

    #[test]
    fn match_directories_stay_relative() {
        assert_eq!(relative_dir("."), Ok(vec![]));
        assert_eq!(relative_dir("apps/web/"), Ok(vec!["apps", "web"]));
        assert_eq!(relative_dir("./apps//web"), Ok(vec!["apps", "web"]));
        for bad in ["", "/abs", "../sibling", "apps/../../x", "a\\b", "a\0b"] {
            assert!(relative_dir(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn labels_refuse_disguising_characters() {
        assert_eq!(label("Frontend é"), Ok(()));
        for bad in ["", "   ", "a\u{202E}b", "a\nb", "\u{200B}"] {
            assert!(label(bad).is_err(), "{bad:?}");
        }
        assert!(label(&"x".repeat(101)).is_err());
    }

    #[test]
    fn a_profile_is_refused_as_a_whole_with_a_location() {
        let text = r#"{"version":1,"services":{"web":{"links":[{"label":"ok","url":"http://localhost:1/"},{"label":"x","url":"javascript:alert(1)"}]}}}"#;
        let parsed: ProjectProfile = serde_json::from_str(text).expect("shape is fine");
        let error = profile(&parsed).expect_err("semantic rule");
        assert_eq!(error.at, "services.web.links[1].url");

        let empty_match = r#"{"version":1,"services":{"web":{"match":{}}}}"#;
        let parsed: ProjectProfile = serde_json::from_str(empty_match).expect("shape");
        assert_eq!(
            profile(&parsed).expect_err("empty").at,
            "services.web.match"
        );

        let bad_id = r#"{"version":1,"services":{"-web":{}}}"#;
        let parsed: ProjectProfile = serde_json::from_str(bad_id).expect("shape");
        assert_eq!(profile(&parsed).expect_err("id").at, "services.-web");
    }
}
