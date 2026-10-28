//! Central secret redaction for process-derived text. Every command line Kossé displays,
//! serves or logs passes through [`command_line`]; probe redirect targets pass through [`text`].
//!
//! Redaction is heuristic and errs towards hiding: a value is replaced when its key looks
//! sensitive (`--token x`, `--api-key=x`, `DB_PASSWORD=x`, `?access_token=x`, `Authorization:
//! Bearer x`, `{"password":"x"}`, `token: x`), when it is URL user information
//! (`postgres://user:secret@host`, `https://<token>@host`), the password of `-u`/`--user
//! user:password`, or when it has the shape
//! of a well-known credential (GitHub, Slack, AWS, Stripe, npm, GitLab, Google and `sk-` API
//! keys, JWTs, PEM private keys). An opaque secret with none of these markers (a bare positional
//! argument) cannot be recognised and is not hidden.

use std::{ffi::OsStr, ops::Range};

use crate::model::CommandLine;

/// What replaces a hidden value.
pub const REDACTED: &str = "[redacted]";
/// Arguments kept from one command line; the rest are dropped and `truncated` is set.
const MAX_ARGS: usize = 128;
/// Characters kept per argument, after redaction.
const MAX_ARG_CHARS: usize = 512;
/// Characters kept across all arguments, after redaction.
const MAX_TOTAL_CHARS: usize = 8 * 1024;

/// Redacts one process's argument vector. Arguments that are not valid UTF-8 are decoded lossily
/// first. Limits apply after redaction, so truncation can never expose part of a hidden value.
#[must_use]
pub fn command_line<S: AsRef<OsStr>>(args: &[S]) -> CommandLine {
    let mut out = Vec::with_capacity(args.len().min(MAX_ARGS));
    let mut redacted = false;
    let mut truncated = false;
    let mut total = 0;
    let mut hide_next = false;
    let mut hide_next_password = false;
    for (index, arg) in args.iter().enumerate() {
        if index == MAX_ARGS || total >= MAX_TOTAL_CHARS {
            truncated = true;
            break;
        }
        let arg = arg.as_ref().to_string_lossy();
        let mut clean = if hide_next {
            redacted = true;
            REDACTED.to_owned()
        } else {
            let (clean, changed) = redact_text(&arg);
            redacted |= changed;
            match (hide_next_password, clean.split_once(':')) {
                (true, Some((user, _))) => {
                    redacted = true;
                    format!("{user}:{REDACTED}")
                }
                _ => clean,
            }
        };
        hide_next = takes_secret_value(&arg);
        hide_next_password = is_user_flag(&arg);
        let budget = MAX_ARG_CHARS.min(MAX_TOTAL_CHARS - total);
        if let Some((cut, _)) = clean.char_indices().nth(budget) {
            clean.truncate(cut);
            clean.push('…');
            truncated = true;
        }
        total += clean.chars().count();
        out.push(clean);
    }
    CommandLine {
        args: out,
        redacted,
        truncated,
    }
}

/// Redacts free text such as a URL; returns the result and whether anything was hidden.
#[must_use]
pub fn text(input: &str) -> (String, bool) {
    redact_text(input)
}

/// A flag whose value follows as the next argument: `--token abc`, `-password abc`.
fn takes_secret_value(arg: &str) -> bool {
    arg.starts_with('-') && !arg.contains('=') && is_sensitive_key(arg)
}

/// curl-style `-u user:password` / `--user user:password` / `--proxy-user …`.
fn is_user_flag(arg: &str) -> bool {
    matches!(arg, "-u" | "-U" | "--user" | "--proxy-user")
}

fn redact_text(input: &str) -> (String, bool) {
    if input.contains("-----BEGIN") && input.contains("PRIVATE KEY") {
        return (REDACTED.to_owned(), true);
    }
    let mut hidden: Vec<Range<usize>> = Vec::new();
    key_values(input, &mut hidden);
    colon_pairs(input, &mut hidden);
    header_value(input, &mut hidden);
    url_passwords(input, &mut hidden);
    auth_schemes(input, &mut hidden);
    credential_shapes(input, &mut hidden);
    if hidden.is_empty() {
        return (input.to_owned(), false);
    }
    hidden.sort_by_key(|range| range.start);
    // Overlapping or touching ranges become one hidden span.
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(hidden.len());
    for range in hidden {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    for range in merged {
        out.push_str(&input[cursor..range.start]);
        out.push_str(REDACTED);
        cursor = range.end;
    }
    out.push_str(&input[cursor..]);
    (out, true)
}

/// Separators between `key=value` pairs: argument start, query strings, lists, whitespace.
fn is_pair_separator(character: char) -> bool {
    matches!(character, '&' | '?' | ';' | ',' | ' ' | '\t' | '\n')
}

/// `key=value` pairs anywhere in the text (`--token=x`, `API_KEY=x`, `?token=x&y=1`,
/// `-Ddb.password=x`) whose key is sensitive.
fn key_values(input: &str, hidden: &mut Vec<Range<usize>>) {
    let mut segment_start = 0;
    for (index, character) in input.char_indices().chain([(input.len(), ' ')]) {
        if !is_pair_separator(character) {
            continue;
        }
        let segment = &input[segment_start..index];
        if let Some(equals) = segment.find('=') {
            let key = &segment[..equals];
            let value_start = segment_start + equals + 1;
            if value_start < index && is_sensitive_key(key) {
                hidden.push(value_start..index);
            } else if is_user_flag(key)
                && let Some(colon) = input[value_start..index].find(':')
            {
                // `--user=name:password`
                hidden.push(value_start + colon + 1..index);
            }
        }
        segment_start = index + character.len_utf8();
    }
}

/// `key: value` pairs inside an argument, quoted or not: JSON (`{"password":"x"}`), YAML-like
/// (`token: x`). A quoted value is hidden up to its closing quote; a bare one up to a delimiter.
fn colon_pairs(input: &str, hidden: &mut Vec<Range<usize>>) {
    let bytes = input.as_bytes();
    let is_key_byte = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.');
    for (colon, _) in input.match_indices(':') {
        if input[colon + 1..].starts_with("//") {
            continue; // a URL scheme
        }
        let mut key_end = colon;
        while key_end > 0 && bytes[key_end - 1] == b' ' {
            key_end -= 1;
        }
        let quote = key_end
            .checked_sub(1)
            .map(|at| bytes[at])
            .filter(|b| matches!(b, b'"' | b'\''));
        let key_end = if quote.is_some() {
            key_end - 1
        } else {
            key_end
        };
        let mut key_start = key_end;
        while key_start > 0 && is_key_byte(bytes[key_start - 1]) {
            key_start -= 1;
        }
        if key_start == key_end || !is_sensitive_key(&input[key_start..key_end]) {
            continue;
        }
        let after = &input[colon + 1..];
        let value_start = colon + 1 + after.len() - after.trim_start().len();
        let rest = &input[value_start..];
        let range = match rest.chars().next() {
            Some(open @ ('"' | '\'')) => {
                let end = rest[1..]
                    .find(open)
                    .map_or(input.len(), |end| value_start + 1 + end);
                value_start + 1..end
            }
            Some(_) => {
                let end = rest
                    .find(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']' | ';' | '&'))
                    .map_or(input.len(), |end| value_start + end);
                value_start..end
            }
            None => continue,
        };
        if !range.is_empty() {
            hidden.push(range);
        }
    }
}

/// `Authorization: Bearer x`, `X-Api-Key: x`, `Cookie: a=b`: a header whose name is sensitive.
fn header_value(input: &str, hidden: &mut Vec<Range<usize>>) {
    let Some(colon) = input.find(':') else {
        return;
    };
    let name = &input[..colon];
    let is_header_name =
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let value_start = colon + 1 + input[colon + 1..].len() - input[colon + 1..].trim_start().len();
    if is_header_name && value_start < input.len() && is_sensitive_key(name) {
        hidden.push(value_start..input.len());
    }
}

/// All user information in `scheme://userinfo@host`: a password (`user:password`) or a token used
/// as the user name (`https://<token>@github.com`).
fn url_passwords(input: &str, hidden: &mut Vec<Range<usize>>) {
    let mut from = 0;
    while let Some(found) = input[from..].find("://") {
        let authority_start = from + found + 3;
        let authority_end = input[authority_start..]
            .find(|c: char| matches!(c, '/' | '?' | '#') || c.is_whitespace())
            .map_or(input.len(), |end| authority_start + end);
        let authority = &input[authority_start..authority_end];
        if let Some(at) = authority.rfind('@')
            && at > 0
        {
            hidden.push(authority_start..authority_start + at);
        }
        from = authority_end.max(authority_start);
    }
}

/// The credential after `Bearer` or `Basic`, wherever it appears.
fn auth_schemes(input: &str, hidden: &mut Vec<Range<usize>>) {
    let lower = input.to_ascii_lowercase();
    for scheme in ["bearer ", "basic "] {
        let mut from = 0;
        while let Some(found) = lower[from..].find(scheme) {
            let start = from + found + scheme.len();
            let start = start + input[start..].len() - input[start..].trim_start().len();
            let end = input[start..]
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ','))
                .map_or(input.len(), |end| start + end);
            if end > start {
                hidden.push(start..end);
            }
            from = end.max(start);
        }
    }
}

/// Prefixes of well-known credential formats, each with the shortest length worth hiding. The
/// prefix must start a word (see [`credential_shapes`]).
const CREDENTIAL_PREFIXES: [(&str, usize); 17] = [
    ("ghp_", 20),
    ("gho_", 20),
    ("ghu_", 20),
    ("ghs_", 20),
    ("ghr_", 20),
    ("github_pat_", 30),
    ("glpat-", 20),
    ("xoxa-", 15),
    ("xoxb-", 15),
    ("xoxp-", 15),
    ("xoxr-", 15),
    ("xoxs-", 15),
    ("xapp-", 15),
    ("sk_live_", 16),
    ("rk_live_", 16),
    ("sk_test_", 16),
    ("npm_", 30),
];

/// Words shaped like well-known credentials. A word is a run of `[A-Za-z0-9_.-]`.
fn credential_shapes(input: &str, hidden: &mut Vec<Range<usize>>) {
    let is_word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
    let mut start = None;
    for (index, character) in input.char_indices().chain([(input.len(), ' ')]) {
        match (start, is_word(character)) {
            (None, true) => start = Some(index),
            (Some(word_start), false) => {
                // A sentence-ending dot is not part of the credential.
                let word = input[word_start..index].trim_end_matches('.');
                if looks_like_credential(word) {
                    hidden.push(word_start..word_start + word.len());
                }
                start = None;
            }
            _ => {}
        }
    }
}

fn looks_like_credential(word: &str) -> bool {
    let alnum = |text: &str| text.chars().all(|c| c.is_ascii_alphanumeric());
    CREDENTIAL_PREFIXES
        .iter()
        .any(|(prefix, min)| word.starts_with(prefix) && word.len() >= *min)
        // AWS access key IDs: AKIA/ASIA plus 16 upper-case letters or digits.
        || (word.len() == 20
            && (word.starts_with("AKIA") || word.starts_with("ASIA"))
            && word[4..].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
        // Google API keys.
        || (word.starts_with("AIza") && word.len() >= 35)
        // OpenAI/Anthropic-style keys (`sk-…`, `sk-proj-…`, `sk-ant-…`).
        || (word.starts_with("sk-") && word.len() >= 20)
        // JWTs: three base64url segments, the header starting with `{"`.
        || (word.starts_with("eyJ") && word.split('.').count() == 3 && word.len() >= 20
            && word.split('.').take(2).all(|part| !part.is_empty() && alnum(&part.replace(['-', '_'], ""))))
}

/// Words that mark a key as holding a secret. Keys are split into lower-case words on
/// punctuation and camel-case boundaries, so `apiKey`, `API_KEY`, `--api-key` and `api.key` all
/// match `key`.
const SENSITIVE_WORDS: [&str; 24] = [
    "apikey",
    "auth",
    "authorization",
    "bearer",
    "cookie",
    "credential",
    "credentials",
    "dsn",
    "jwt",
    "key",
    "pass",
    "passphrase",
    "passwd",
    "password",
    "private",
    "privatekey",
    "pwd",
    "secret",
    "secrets",
    "session",
    "sessionid",
    "signature",
    "token",
    "tokens",
];
/// Compound words with no boundary (`ACCESSTOKEN`, `PGPASSWORD`, `clientsecret`).
/// `pass` also matches `bypass` and `compass`: over-hiding is the accepted cost.
const SENSITIVE_SUFFIXES: [&str; 6] = ["token", "secret", "password", "passwd", "apikey", "pass"];

fn is_sensitive_key(key: &str) -> bool {
    words(key.trim_start_matches('-')).iter().any(|word| {
        SENSITIVE_WORDS.contains(&word.as_str())
            || SENSITIVE_SUFFIXES
                .iter()
                .any(|suffix| word.ends_with(suffix))
    })
}

fn words(key: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for character in key.chars() {
        if !character.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if character.is_uppercase() && previous_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        previous_lower = character.is_lowercase() || character.is_ascii_digit();
        current.extend(character.to_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Credential-shaped test values are assembled at run time so that the source holds no
    /// literal that secret scanners would flag.
    fn fake(prefix: &str, body: &str) -> String {
        format!("{prefix}{body}")
    }

    fn redact(args: &[&str]) -> Vec<String> {
        command_line(args).args
    }

    #[test]
    fn sensitive_flags_hide_inline_and_following_values() {
        assert_eq!(
            redact(&["node", "--token=abc123", "--port", "3000"]),
            ["node", "--token=[redacted]", "--port", "3000"]
        );
        assert_eq!(
            redact(&["app", "--password", "hunter2", "--verbose"]),
            ["app", "--password", "[redacted]", "--verbose"]
        );
        assert_eq!(
            redact(&["app", "--client-secret", "s3cr3t", "-apiKey", "k"]),
            [
                "app",
                "--client-secret",
                "[redacted]",
                "-apiKey",
                "[redacted]"
            ]
        );
        assert_eq!(redact(&["--author", "me"]), ["--author", "me"]);
    }

    #[test]
    fn environment_style_assignments_and_properties_are_hidden() {
        assert_eq!(
            redact(&[
                "env",
                "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI",
                "PGPASSWORD=pw",
                "NODE_ENV=development",
                "-Dspring.datasource.password=pw",
            ]),
            [
                "env",
                "AWS_SECRET_ACCESS_KEY=[redacted]",
                "PGPASSWORD=[redacted]",
                "NODE_ENV=development",
                "-Dspring.datasource.password=[redacted]",
            ]
        );
    }

    #[test]
    fn url_credentials_and_query_secrets_are_hidden() {
        assert_eq!(
            redact(&["postgres://app:pa55word@localhost:5432/db?sslmode=disable"]),
            ["postgres://[redacted]@localhost:5432/db?sslmode=disable"]
        );
        assert_eq!(
            redact(&["http://127.0.0.1:9/cb?access_token=abc&state=ok"]),
            ["http://127.0.0.1:9/cb?access_token=[redacted]&state=ok"]
        );
        assert_eq!(
            redact(&["redis://localhost:6379"]),
            ["redis://localhost:6379"]
        );
        assert_eq!(
            redact(&["ssh://git@host/repo"]),
            ["ssh://[redacted]@host/repo"]
        );
        assert_eq!(
            redact(&["https://opaque-token-value@github.com/org/repo"]),
            ["https://[redacted]@github.com/org/repo"]
        );
    }

    #[test]
    fn authorization_headers_and_schemes_are_hidden() {
        assert_eq!(
            redact(&[
                "curl",
                "-H",
                "Authorization: Bearer abc.def",
                "-H",
                "Accept: */*"
            ]),
            [
                "curl",
                "-H",
                "Authorization: [redacted]",
                "-H",
                "Accept: */*"
            ]
        );
        assert_eq!(
            redact(&["--header=X-Trace: 1, bearer qwerty"]),
            ["--header=X-Trace: 1, bearer [redacted]"]
        );
        assert_eq!(redact(&["Cookie: sid=1"]), ["Cookie: [redacted]"]);
    }

    #[test]
    fn credential_shapes_are_hidden_wherever_they_appear() {
        let github = fake("ghp_", "0123456789abcdefghijABCDEFGHIJ012345");
        let slack = fake("xoxb-", "1234-5678-abcdefgh");
        let aws = fake("AKIA", "ABCDEFGHIJ012345");
        let jwt = fake("eyJ", "hbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl");
        let openai = fake("sk-", "proj-0123456789abcdef");
        let args = [
            format!("https://{github}@github.com/org/repo"),
            format!("{slack},{aws}"),
            jwt.clone(),
            format!("--note=uses {openai}."),
        ];
        let out = command_line(&args);
        assert!(out.redacted);
        let joined = out.args.join(" ");
        for secret in [&github, &slack, &aws, &jwt, &openai] {
            assert!(!joined.contains(secret.as_str()), "{secret} in {joined}");
        }
        assert_eq!(out.args[0], "https://[redacted]@github.com/org/repo");
        assert_eq!(out.args[1], "[redacted],[redacted]");
        assert_eq!(out.args[3], "--note=uses [redacted].");
    }

    #[test]
    fn json_and_colon_pairs_inside_an_argument_are_hidden() {
        assert_eq!(
            redact(&[
                "--config",
                r#"{"password":"hunter2","apiKey": "k1","port":3000,"token":42}"#
            ]),
            [
                "--config",
                r#"{"password":"[redacted]","apiKey": "[redacted]","port":3000,"token":[redacted]}"#
            ]
        );
        assert_eq!(
            redact(&["--set=token: abc, mode: dev"]),
            ["--set=token: [redacted], mode: dev"]
        );
        assert_eq!(redact(&["{'secret': 'x y'}"]), ["{'secret': '[redacted]'}"]);
        assert_eq!(redact(&["host:8080", "a:b"]), ["host:8080", "a:b"]);
    }

    #[test]
    fn user_flags_hide_the_password_half() {
        assert_eq!(
            redact(&["curl", "-u", "me:pw", "--user=you:pw2", "-u", "nopassword"]),
            [
                "curl",
                "-u",
                "me:[redacted]",
                "--user=you:[redacted]",
                "-u",
                "nopassword"
            ]
        );
    }

    #[test]
    fn compound_pass_keys_are_hidden() {
        assert_eq!(
            redact(&["DBPASS=x", "SMTP_PASS=y"]),
            ["DBPASS=[redacted]", "SMTP_PASS=[redacted]"]
        );
    }

    #[test]
    fn private_keys_hide_the_whole_argument() {
        assert_eq!(
            redact(&["-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----"]),
            ["[redacted]"]
        );
    }

    #[test]
    fn ordinary_commands_pass_unchanged() {
        let args = [
            "/usr/local/bin/node",
            "node_modules/.bin/vite",
            "--host",
            "127.0.0.1",
            "--port=5173",
            "<script>alert(1)</script>",
            "skeleton-key-pattern",
        ];
        let out = command_line(&args);
        assert_eq!(out.args, args);
        assert!(!out.redacted && !out.truncated);
    }

    #[test]
    fn limits_apply_after_redaction() {
        let long_secret = format!("--token={}", "x".repeat(2000));
        let out = command_line(&[long_secret.as_str()]);
        assert_eq!(out.args, ["--token=[redacted]"]);
        assert!(!out.truncated);

        let long_plain = "a".repeat(2000);
        let out = command_line(&[long_plain.as_str()]);
        assert_eq!(out.args[0].chars().count(), MAX_ARG_CHARS + 1);
        assert!(out.truncated);

        let many: Vec<String> = (0..500).map(|i| i.to_string()).collect();
        let out = command_line(&many);
        assert_eq!(out.args.len(), MAX_ARGS);
        assert!(out.truncated);
    }

    #[test]
    fn hidden_next_value_survives_the_argument_limit_boundary() {
        let mut args: Vec<String> = (0..MAX_ARGS - 1).map(|i| i.to_string()).collect();
        args.push("--token".into());
        args.push("never-shown".into());
        let out = command_line(&args);
        assert!(!out.args.iter().any(|arg| arg.contains("never-shown")));
    }

    #[test]
    fn keys_split_into_words() {
        assert_eq!(words("apiKey"), ["api", "key"]);
        assert_eq!(
            words("AWS_SECRET_ACCESS_KEY"),
            ["aws", "secret", "access", "key"]
        );
        assert_eq!(words("db.password"), ["db", "password"]);
        assert!(is_sensitive_key("--ACCESSTOKEN"));
        assert!(!is_sensitive_key("--keep-alive"));
        assert!(!is_sensitive_key("monkey"));
    }

    #[test]
    fn non_utf8_arguments_are_decoded_lossily() {
        use std::os::unix::ffi::OsStrExt;
        let raw = OsStr::from_bytes(b"caf\xe9 --token=x");
        assert_eq!(
            command_line(&[raw]).args,
            ["caf\u{fffd} --token=[redacted]"]
        );
    }
}
