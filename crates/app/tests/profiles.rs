//! Project profiles through the real binary: `kosse list`, `kosse config` and a running
//! `kosse serve`, with owned fixtures in temporary repositories and a temporary configuration
//! directory (`KOSSE_CONFIG_DIR`), never the user's own. Only fixture rows are asserted on.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    collections::BTreeMap,
    fs,
    io::{ErrorKind, Read as _},
    net::{IpAddr, Ipv4Addr, TcpListener},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, SystemTime},
};

use kosse_core::{
    contract::SavedProfile,
    model::{
        HttpProbe, Observation, ProbeResult, ProbeSkip, ProcessIdentity, ScanWarning, Service,
        Snapshot,
    },
    profiles::view::{LinkReach, ProfileMatch, ProfileProblem, ProfileSource, ProjectMetadata},
};
use kosse_fixtures::{
    Fixture, FixtureSpec,
    protocol::Serve,
    repo::{Sandbox, git, init_repo},
};
use serde_json::{Value, json};
use support::{RawRequest, Server, eventually, sent_by};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
/// A scan (3 s cadence), the file check interval and slack.
const APPLIED_WITHIN: Duration = Duration::from_secs(12);

fn kosse(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", config)
        .args(args)
        .output()
        .expect("run kosse")
}

fn list_json(config: &Path) -> Observation {
    let output = kosse(config, &["list", "--json", "--port", "0"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: Snapshot = serde_json::from_slice(&output.stdout).expect("snapshot");
    snapshot.observation.expect("an observation")
}

fn list_table(config: &Path) -> String {
    let output = kosse(config, &["list", "--port", "0"]);
    assert!(output.status.success());
    String::from_utf8(output.stdout).expect("UTF-8")
}

fn spawn(dir: &Path) -> Fixture {
    Fixture::spawn(&FixtureSpec::new().listen(&[V4]).cwd(dir)).expect("fixture")
}

fn port_of(fixture: &Fixture) -> u16 {
    fixture.record().endpoints[0].port()
}

fn identity(fixture: &Fixture) -> ProcessIdentity {
    ProcessIdentity {
        pid: fixture.pid(),
        started_at_secs: Some(fixture.record().started_at_secs),
    }
}

fn service_of<'a>(observation: &'a Observation, fixture: &Fixture) -> &'a Service {
    observation
        .services
        .iter()
        .find(|service| service.process.identity.pid == fixture.pid())
        .unwrap_or_else(|| panic!("fixture {} missing", fixture.pid()))
}

fn metadata_of<'a>(observation: &'a Observation, root: &Path) -> Option<&'a ProjectMetadata> {
    observation
        .profiles
        .iter()
        .find(|metadata| Path::new(&metadata.root) == root)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "compared with `Service::profile` as is"
)]
fn matched(id: &str) -> Option<ProfileMatch> {
    Some(ProfileMatch::Matched {
        service: id.to_owned(),
    })
}

fn text(path: &Path) -> String {
    path.to_str().expect("UTF-8 path").to_owned()
}

fn write_central(config: &Path, projects: &Value) {
    fs::create_dir_all(config).expect("config dir");
    let document = json!({ "version": 1, "projects": projects });
    fs::write(config.join("config.json"), document.to_string()).expect("write config");
}

fn row_of<'a>(table: &'a str, fixture: &Fixture) -> &'a str {
    let pid = fixture.pid().to_string();
    table
        .lines()
        .find(|line| line.split_whitespace().next() == Some(pid.as_str()))
        .unwrap_or_else(|| panic!("no row for {pid} in\n{table}"))
}

/// Every file under `dir` except `.git`, with size and modification time.
fn listing(dir: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).expect("read dir") {
            let entry = entry.expect("entry");
            if entry.file_name() == ".git" {
                continue;
            }
            let metadata = entry.metadata().expect("stat");
            if metadata.is_dir() {
                pending.push(entry.path());
            }
            files.insert(
                entry.path(),
                (metadata.len(), metadata.modified().expect("mtime")),
            );
        }
    }
    files
}

const REPO_PROFILE: &str = r#"{
  "$schema": "./kosse.schema.json",
  "version": 1,
  "name": "Acme",
  "services": {
    "web": {
      "name": "Repo Web",
      "match": { "cwd": "apps/web" },
      "protocol": "http",
      "probePath": "/health",
      "links": [
        { "label": "Docs", "url": "http://localhost:3001/docs" },
        { "label": "LocalCan", "url": "https://acme.local/", "kind": "local" }
      ]
    },
    "api": { "name": "API", "match": { "cwd": "apps/api" } }
  }
}"#;

#[test]
fn central_profile_overrides_the_repository_file_with_provenance() {
    let tree = Sandbox::new().expect("sandbox");
    let repo = tree.dir("acme").expect("dir");
    init_repo(&repo).expect("repo");
    tree.file("acme/apps/web/package.json", "{}").expect("file");
    tree.file("acme/apps/api/package.json", "{}").expect("file");
    tree.file("acme/kosse.json", REPO_PROFILE).expect("file");
    git(&repo, &["add", "-A"]).expect("add");
    git(&repo, &["commit", "-q", "-m", "profile"]).expect("commit");
    let config = tree.root().join("config");
    write_central(
        &config,
        &json!({ text(&repo): { "version": 1, "services": { "web": { "name": "Central Web" } } } }),
    );
    let web = spawn(&repo.join("apps/web"));
    let before = git(&repo, &["status", "--porcelain"]).expect("status");

    let observation = list_json(&config);
    assert_eq!(service_of(&observation, &web).profile, matched("web"));
    let metadata = metadata_of(&observation, &repo).expect("metadata");
    assert!(metadata.observed);
    assert_eq!(
        metadata.sources,
        [ProfileSource::Central, ProfileSource::Repository]
    );
    assert_eq!(metadata.name.as_deref(), Some("Acme"));
    assert_eq!(metadata.name_source, Some(ProfileSource::Repository));

    let entry = |id: &str| {
        metadata
            .services
            .iter()
            .find(|entry| entry.id == id)
            .expect("entry")
    };
    let web_entry = entry("web");
    assert_eq!(web_entry.name.as_deref(), Some("Central Web"));
    assert_eq!(web_entry.sources.name, Some(ProfileSource::Central));
    assert_eq!(web_entry.sources.matcher, Some(ProfileSource::Repository));
    assert_eq!(web_entry.probe_path.as_deref(), Some("/health"));
    assert_eq!(
        web_entry.sources.probe_path,
        Some(ProfileSource::Repository)
    );
    assert_eq!(web_entry.matched, [identity(&web)]);
    let reaches: Vec<(&str, &str, LinkReach)> = web_entry
        .links
        .iter()
        .map(|link| (link.url.as_str(), link.host.as_str(), link.reach))
        .collect();
    assert_eq!(
        reaches,
        [
            (
                "http://localhost:3001/docs",
                "localhost",
                LinkReach::Loopback
            ),
            ("https://acme.local/", "acme.local", LinkReach::Declared),
        ],
        "a custom domain is declared, never assumed local"
    );
    let api = entry("api");
    assert!(api.matched.is_empty(), "configured, not observed");

    let table = list_table(&config);
    assert!(
        row_of(&table, &web).contains("Central Web (web) [central]"),
        "{table}"
    );
    let unobserved = table
        .split("CONFIGURED, NOT OBSERVED")
        .nth(1)
        .unwrap_or_else(|| panic!("no unobserved section in\n{table}"));
    let api_row = unobserved
        .lines()
        .find(|line| line.contains("API (api)"))
        .unwrap_or_else(|| panic!("no api row in\n{table}"));
    assert!(
        api_row.contains("repository") && api_row.contains("cwd apps/api"),
        "{api_row}"
    );

    assert_eq!(
        git(&repo, &["status", "--porcelain"]).expect("status"),
        before
    );
    assert_eq!(
        fs::read_to_string(repo.join("kosse.json")).expect("read"),
        REPO_PROFILE
    );
}

#[test]
fn matching_never_leaves_the_project() {
    let tree = Sandbox::new().expect("sandbox");
    let repo = tree.dir("app").expect("dir");
    let other = tree.dir("other").expect("dir");
    init_repo(&repo).expect("repo");
    init_repo(&other).expect("repo");
    let web_dir = tree.dir("app/apps/web").expect("dir");
    let both_dir = tree.dir("app/apps/both").expect("dir");
    let api_dir = tree.dir("app/apps/api").expect("dir");
    symlink(&other, repo.join("apps/escape")).expect("symlink out");
    symlink(&api_dir, repo.join("apps/link")).expect("symlink in");

    let (first, second) = (spawn(&web_dir), spawn(&web_dir));
    let in_other = spawn(&other);
    let in_both = spawn(&both_dir);
    let in_api = spawn(&api_dir);
    let config = tree.root().join("config");
    write_central(
        &config,
        &json!({ text(&repo): { "version": 1, "services": {
            "web": { "match": { "cwd": "apps/web" } },
            "linked": { "match": { "cwd": "apps/link" } },
            "escape": { "match": { "cwd": "apps/escape" } },
            "portOnly": { "match": { "port": port_of(&in_other) } },
            "one": { "match": { "cwd": "apps/both" } },
            "two": { "match": { "cwd": "apps/both", "port": port_of(&in_both) } },
        } } }),
    );

    let observation = list_json(&config);
    // Two processes in one directory: both labeled, each its own identity.
    assert_eq!(service_of(&observation, &first).profile, matched("web"));
    assert_eq!(service_of(&observation, &second).profile, matched("web"));
    let metadata = metadata_of(&observation, &repo).expect("metadata");
    let entry = |id: &str| {
        metadata
            .services
            .iter()
            .find(|entry| entry.id == id)
            .expect("entry")
    };
    let mut both = vec![identity(&first), identity(&second)];
    both.sort_by_key(|identity| identity.pid);
    assert_eq!(entry("web").matched, both);
    // A symbolic link inside the project resolves; one leading out matches nothing.
    assert_eq!(service_of(&observation, &in_api).profile, matched("linked"));
    assert!(
        entry("escape").matched.is_empty(),
        "a link out of the project matches nothing"
    );
    assert!(
        observation
            .warnings
            .contains(&ScanWarning::ProfileMatchOutsideProject {
                root: text(&repo),
                service: "escape".into(),
            })
    );
    // A port alone never claims another project's process.
    assert_eq!(service_of(&observation, &in_other).profile, None);
    assert!(
        entry("portOnly").matched.is_empty(),
        "configured, not observed"
    );
    // Several matching entries: visible ambiguity, no arbitrary winner.
    assert_eq!(
        service_of(&observation, &in_both).profile,
        Some(ProfileMatch::Ambiguous {
            services: vec!["one".into(), "two".into()]
        })
    );
    let table = list_table(&config);
    assert!(
        row_of(&table, &in_both).contains("ambiguous: one, two"),
        "{table}"
    );
}

/// A name, how to write the file, and the problem expected.
type Case<'a> = (&'a str, Box<dyn Fn() + 'a>, fn(&ProfileProblem) -> bool);

#[test]
fn hostile_repository_files_are_refused() {
    let tree = Sandbox::new().expect("sandbox");
    let project = tree.dir("project").expect("dir");
    tree.file("project/package.json", "{}").expect("file");
    let config = tree.root().join("config");
    let fixture = spawn(&project);
    let profile_file = project.join("kosse.json");

    let valid_elsewhere = tree
        .file("elsewhere.json", r#"{"version":1,"name":"Elsewhere"}"#)
        .expect("file");
    let cases: Vec<Case<'_>> = vec![
        (
            "javascript link",
            Box::new(|| {
                fs::write(&profile_file, r#"{"version":1,"services":{"web":{"match":{"cwd":"."},"links":[{"label":"x","url":"javascript:alert(1)"}]}}}"#).expect("write");
            }),
            |problem| matches!(problem, ProfileProblem::Invalid { reason } if reason.contains("links[0].url")),
        ),
        (
            "traversal",
            Box::new(|| {
                fs::write(
                    &profile_file,
                    r#"{"version":1,"services":{"web":{"match":{"cwd":"../elsewhere"}}}}"#,
                )
                .expect("write");
            }),
            |problem| matches!(problem, ProfileProblem::Invalid { reason } if reason.contains("..")),
        ),
        (
            "duplicate IDs",
            Box::new(|| {
                fs::write(
                    &profile_file,
                    r#"{"version":1,"services":{"web":{},"web":{}}}"#,
                )
                .expect("write");
            }),
            |problem| matches!(problem, ProfileProblem::Invalid { reason } if reason.contains("duplicate")),
        ),
        (
            "newer version",
            Box::new(|| {
                fs::write(&profile_file, r#"{"version":2,"services":[]}"#).expect("write");
            }),
            |problem| matches!(problem, ProfileProblem::NewerVersion { version } if version == "2"),
        ),
        (
            "too large",
            Box::new(|| {
                let padding = "x".repeat(70 * 1024);
                fs::write(
                    &profile_file,
                    format!(r#"{{"version":1,"pad":"{padding}"}}"#),
                )
                .expect("write");
            }),
            |problem| matches!(problem, ProfileProblem::TooLarge { .. }),
        ),
        (
            "symbolic link",
            Box::new(|| {
                symlink(&valid_elsewhere, &profile_file).expect("symlink");
            }),
            |problem| matches!(problem, ProfileProblem::NotAFile),
        ),
    ];
    for (case, write, expected) in cases {
        let _ = fs::remove_file(&profile_file);
        write();
        let observation = list_json(&config);
        let warning = observation
            .warnings
            .iter()
            .find_map(|warning| match warning {
                ScanWarning::ProfileUnusable {
                    source: ProfileSource::Repository,
                    path,
                    problem,
                    last_good_retained,
                } if Path::new(path) == profile_file => Some((problem, *last_good_retained)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{case}: no warning in {:?}", observation.warnings));
        assert!(expected(warning.0), "{case}: {:?}", warning.0);
        assert!(
            !warning.1,
            "{case}: a one-shot scan has no last good version"
        );
        assert!(metadata_of(&observation, &project).is_none(), "{case}");
        assert_eq!(service_of(&observation, &fixture).profile, None, "{case}");
    }
}

/// A valid profile pointing at a listener the test owns, with fields that would execute, include
/// or fetch elsewhere: applied as data, and nothing connects or runs.
#[test]
fn profiles_are_inert_data() {
    let tree = Sandbox::new().expect("sandbox");
    let project = tree.dir("project").expect("dir");
    tree.file("project/package.json", "{}").expect("file");
    let config = tree.root().join("config");
    let fixture = spawn(&project);
    let profile_file = project.join("kosse.json");
    let canary = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("canary");
    canary.set_nonblocking(true).expect("nonblocking");
    let canary_url = format!(
        "http://127.0.0.1:{}",
        canary.local_addr().expect("addr").port()
    );
    let marker = tree.root().join("executed");
    let inert = json!({
        "$schema": format!("{canary_url}/kosse.schema.json"),
        "version": 1,
        "extends": format!("{canary_url}/base.json"),
        "scripts": { "dev": format!("touch {}", text(&marker)) },
        "services": { "web": {
            "match": { "cwd": "." },
            "protocol": "http",
            "probePath": "/health",
            "links": [{ "label": "Canary", "url": format!("{canary_url}/") }],
            "command": format!("touch {}", text(&marker)),
        } },
    });
    fs::write(&profile_file, inert.to_string()).expect("write");
    let observation = list_json(&config);
    assert_eq!(service_of(&observation, &fixture).profile, matched("web"));
    let server = Server::start_configured(&config, &[]);
    eventually(APPLIED_WITHIN, || {
        let snapshot = server.snapshot();
        let observation = snapshot.observation?;
        (service_of(&observation, &fixture).profile == matched("web")).then_some(())
    })
    .expect("serve applies the profile too");
    let ours = server.snapshot().instance;
    let (status, _) = server.terminate();
    assert!(status.success());
    let intrusions = connections_except_foreign_probes(&canary, &ours);
    assert!(
        intrusions.is_empty(),
        "nothing may connect to a $schema, include or link host: {intrusions:?}"
    );
    assert!(!marker.exists(), "nothing in a profile is executed");
}

/// What each pending connection to `listener` sent, except probes by a Kossé instance other than
/// `ours`: this test process runs in a project, so a developer's own running Kossé probes its
/// listeners too. Accepted connections stay open until all are read, so no prober sees a close
/// and retries over TLS mid-drain.
fn connections_except_foreign_probes(listener: &TcpListener, ours: &str) -> Vec<String> {
    let marker = format!("\r\n{}: ", kosse_core::probe::INSTANCE_HEADER);
    let mut open = Vec::new();
    let mut others = Vec::new();
    loop {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock => return others,
            Err(error) => panic!("accept: {error}"),
        };
        stream.set_nonblocking(false).expect("blocking");
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("read timeout");
        let mut head = [0; 4096];
        let read = stream.read(&mut head).unwrap_or(0);
        let text = String::from_utf8_lossy(&head[..read]).to_ascii_lowercase();
        let foreign = text.split(&marker).skip(1).any(|rest| {
            rest.split("\r\n")
                .next()
                .is_some_and(|instance| instance != ours)
        });
        if !foreign {
            others.push(text);
        }
        open.push(stream);
    }
}

fn post_profile(
    server: &Server,
    token: &str,
    root: &str,
    profile: &Value,
) -> support::HttpResponse {
    server.post_json_admitted(
        "/api/profile",
        token,
        &json!({ "root": root, "profile": profile }).to_string(),
    )
}

#[test]
fn customizing_writes_only_the_central_configuration() {
    let tree = Sandbox::new().expect("sandbox");
    let repo = tree.dir("work").expect("dir");
    init_repo(&repo).expect("repo");
    tree.file("work/package.json", "{}").expect("file");
    tree.file("work/src/index.js", "export {};\n")
        .expect("file");
    git(&repo, &["add", "-A"]).expect("add");
    git(&repo, &["commit", "-q", "-m", "app"]).expect("commit");
    let fixture = spawn(&repo);
    let config = tree.root().join("config");
    let server = Server::start_configured(&config, &[]);
    let root = text(&repo);
    eventually(APPLIED_WITHIN, || {
        let observation = server.snapshot().observation?;
        (service_of(&observation, &fixture).project.as_deref() == Some(root.as_str())).then_some(())
    })
    .expect("fixture attributed");
    let status_before = git(&repo, &["status", "--porcelain"]).expect("status");
    let files_before = listing(&repo);
    let token = server.token();
    let profile = json!({ "version": 1, "name": "Customized", "services": { "web": {
        "name": "Web", "match": { "cwd": "." },
        "links": [{ "label": "Preview", "url": "https://work.localhost/" }],
    } } });

    // The same guard as every unsafe request.
    let body = json!({ "root": root, "profile": profile }).to_string();
    let no_token = server.send(
        &RawRequest::new("POST", "/api/profile")
            .host(&server.authority())
            .header("Origin", &server.origin())
            .header("Content-Type", "application/json")
            .body(&body),
    );
    assert_eq!(no_token.status, 403, "{}", no_token.text());
    let cross_site = server.send(
        &RawRequest::new("POST", "/api/profile")
            .host(&server.authority())
            .header("Origin", "http://evil.example")
            .header("Content-Type", "application/json")
            .header("X-Kosse-Token", &token)
            .body(&body),
    );
    assert_eq!(cross_site.status, 403);
    let unknown = post_profile(&server, &token, "/nowhere/else", &profile);
    assert_eq!(unknown.status, 404, "{}", unknown.text());
    assert_eq!(unknown.error_code(), "unknownProject");
    assert!(!config.exists(), "refusals write nothing");

    let saved = post_profile(&server, &token, &root, &profile);
    assert_eq!(saved.status, 200, "{}", saved.text());
    let saved: SavedProfile = serde_json::from_slice(&saved.body).expect("saved");
    assert_eq!(
        saved.profile.and_then(|profile| profile.name).as_deref(),
        Some("Customized")
    );
    eventually(APPLIED_WITHIN, || {
        let observation = server.snapshot().observation?;
        let metadata = metadata_of(&observation, &repo)?;
        (service_of(&observation, &fixture).profile == matched("web")
            && metadata.sources == [ProfileSource::Central])
        .then_some(())
    })
    .expect("the next snapshot applies the saved profile");
    let file = config.join("config.json");
    let mode = |path: &Path| fs::metadata(path).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode(&config), 0o700);
    assert_eq!(mode(&file), 0o600);
    let stored: Value = serde_json::from_slice(&fs::read(&file).expect("read")).expect("JSON");
    assert_eq!(stored["projects"][&root]["name"], "Customized");

    // An invalid profile is refused with its location; the file stays as it was.
    let written = fs::read(&file).expect("read");
    let hostile = json!({ "version": 1, "services": { "web": {
        "links": [{ "label": "x", "url": "javascript:alert(1)" }],
    } } });
    let refused = post_profile(&server, &token, &root, &hostile);
    assert_eq!(refused.status, 422, "{}", refused.text());
    assert_eq!(refused.error_code(), "invalidProfile");
    assert!(
        refused.text().contains("services.web.links[0].url"),
        "{}",
        refused.text()
    );
    assert_eq!(fs::read(&file).expect("read"), written);

    // Removing the profile: the next snapshot no longer labels the process.
    let removed = post_profile(&server, &token, &root, &Value::Null);
    assert_eq!(removed.status, 200, "{}", removed.text());
    eventually(APPLIED_WITHIN, || {
        let observation = server.snapshot().observation?;
        (service_of(&observation, &fixture).profile.is_none()
            && metadata_of(&observation, &repo).is_none())
        .then_some(())
    })
    .expect("profile removed");

    assert_eq!(
        git(&repo, &["status", "--porcelain"]).expect("status"),
        status_before
    );
    assert_eq!(listing(&repo), files_before, "the repository is untouched");
    let (status, _) = server.terminate();
    assert!(status.success());
}

/// This test's warning only: other suites' projects are observed too.
fn repository_warning(observation: &Observation, file: &Path) -> Option<(ProfileProblem, bool)> {
    observation
        .warnings
        .iter()
        .find_map(|warning| match warning {
            ScanWarning::ProfileUnusable {
                source: ProfileSource::Repository,
                path,
                problem,
                last_good_retained,
            } if Path::new(path) == file => Some((problem.clone(), *last_good_retained)),
            _ => None,
        })
}

fn central_warning(observation: &Observation) -> Option<ProfileProblem> {
    observation
        .warnings
        .iter()
        .find_map(|warning| match warning {
            ScanWarning::ProfileUnusable {
                source: ProfileSource::Central,
                problem,
                last_good_retained: false,
                ..
            } => Some(problem.clone()),
            _ => None,
        })
}

/// Waits for a snapshot whose observation satisfies `condition`; fails with what it last saw.
fn until(server: &Server, condition: &dyn Fn(&Observation) -> bool, what: &str) {
    eventually(APPLIED_WITHIN, || {
        let observation = server.snapshot().observation?;
        condition(&observation).then_some(())
    })
    .unwrap_or_else(|| {
        let last = server.snapshot().observation;
        let (warnings, profiles) = last.map(|o| (o.warnings, o.profiles)).unwrap_or_default();
        panic!("{what}; last warnings {warnings:?}, profiles {profiles:?}")
    });
}

#[test]
fn reloads_keep_the_last_good_profile_and_follow_deletion() {
    let tree = Sandbox::new().expect("sandbox");
    let project = tree.dir("project").expect("dir");
    tree.file("project/package.json", "{}").expect("file");
    let fixture = spawn(&project);
    let profile_file = project.join("kosse.json");
    let profile = |name: &str| {
        json!({ "version": 1, "services": { "web": { "name": name, "match": { "cwd": "." } } } })
            .to_string()
    };
    fs::write(&profile_file, profile("First")).expect("write");
    let config = tree.root().join("config");
    let server = Server::start_configured(&config, &[]);
    let name_of = |observation: &Observation| {
        metadata_of(observation, &project)
            .and_then(|metadata| metadata.services.first())
            .and_then(|entry| entry.name.clone())
    };
    until(
        &server,
        &|o| name_of(o).as_deref() == Some("First"),
        "first profile applied",
    );
    fs::write(&profile_file, "{\"version\": 1, \"services\": ").expect("corrupt");
    until(
        &server,
        &|o| {
            matches!(
                repository_warning(o, &profile_file),
                Some((ProfileProblem::Invalid { .. }, true))
            ) && name_of(o).as_deref() == Some("First")
                && service_of(o, &fixture).profile == matched("web")
        },
        "an invalid update keeps the last good profile, with a warning",
    );
    fs::write(&profile_file, r#"{"version": 3}"#).expect("newer");
    until(
        &server,
        &|o| {
            matches!(
                repository_warning(o, &profile_file),
                Some((ProfileProblem::NewerVersion { .. }, true))
            ) && name_of(o).as_deref() == Some("First")
        },
        "a newer version is refused, the last good kept",
    );
    fs::remove_file(&profile_file).expect("delete");
    until(
        &server,
        &|o| {
            repository_warning(o, &profile_file).is_none()
                && metadata_of(o, &project).is_none()
                && service_of(o, &fixture).profile.is_none()
        },
        "deleting the file removes the profile without a warning",
    );
    fs::write(&profile_file, profile("Second")).expect("recreate");
    until(
        &server,
        &|o| name_of(o).as_deref() == Some("Second"),
        "recreated file applied",
    );
    let (status, _) = server.terminate();
    assert!(status.success());
}

/// The central file: corrupt and newer versions are reported and never overwritten; deleting it
/// is no problem, and the next save writes a fresh one.
#[test]
fn central_problems_are_reported_and_never_overwritten() {
    let tree = Sandbox::new().expect("sandbox");
    let project = tree.dir("project").expect("dir");
    tree.file("project/package.json", "{}").expect("file");
    let fixture = spawn(&project);
    let config = tree.root().join("config");
    let server = Server::start_configured(&config, &[]);
    let root = text(&project);
    until(
        &server,
        &|o| service_of(o, &fixture).project.as_deref() == Some(root.as_str()),
        "fixture attributed",
    );
    let token = server.token();
    let central = config.join("config.json");
    fs::create_dir_all(&config).expect("dir");
    for (contents, newer) in [
        ("{\"version\": 1, \"projects\": {", false),
        (r#"{"version": 9, "projects": "elsewhere"}"#, true),
    ] {
        fs::write(&central, contents).expect("write");
        until(
            &server,
            &|o| {
                central_warning(o).is_some_and(|problem| {
                    matches!(problem, ProfileProblem::NewerVersion { .. }) == newer
                })
            },
            "central problem reported",
        );
        let refused = post_profile(&server, &token, &root, &json!({ "version": 1 }));
        assert_eq!(refused.status, 409, "{}", refused.text());
        assert_eq!(refused.error_code(), "configUnavailable");
        assert_eq!(fs::read_to_string(&central).expect("read"), contents);
    }
    fs::remove_file(&central).expect("delete");
    until(
        &server,
        &|o| central_warning(o).is_none(),
        "deleted central file is no problem",
    );
    let saved = post_profile(
        &server,
        &token,
        &root,
        &json!({ "version": 1, "name": "Mine" }),
    );
    assert_eq!(saved.status, 200, "{}", saved.text());
    until(
        &server,
        &|o| {
            metadata_of(o, &project).is_some_and(|metadata| {
                metadata.name.as_deref() == Some("Mine")
                    && metadata.sources == [ProfileSource::Central]
            })
        },
        "a fresh central file is written and applied",
    );
    let (status, _) = server.terminate();
    assert!(status.success());
}

/// A repository file can name a probe path and hint at HTTP, but cannot steer probing: its
/// `protocol` never makes an endpoint a candidate, and its `probePath` is requested only after a
/// plain `GET /` got an HTTP response. Only the user's central profile declares HTTP.
#[test]
fn only_a_central_declaration_makes_a_probe_candidate_and_paths_wait_for_verified_http() {
    let tree = Sandbox::new().expect("sandbox");
    let project = tree.dir("db-ish").expect("dir");
    tree.file("db-ish/package.json", "{}").expect("file");
    // Named like a database server, so it is not probed unless the user declares HTTP.
    let fixture = Fixture::spawn(
        &FixtureSpec::new()
            .listen(&[V4])
            .serve(Serve::NotFound)
            .record()
            .executable_name("postgres")
            .cwd(&project),
    )
    .expect("fixture");
    fs::write(
        project.join("kosse.json"),
        r#"{"version":1,"services":{"api":{"match":{"cwd":"."},"protocol":"http","probePath":"/healthz?ready=1"}}}"#,
    )
    .expect("write");
    let config = tree.root().join("config");
    let server = Server::start_configured(&config, &[&format!("--probe-only={}", fixture.pid())]);
    let http_of =
        |observation: &Observation| service_of(observation, &fixture).endpoints[0].http.clone();
    until(
        &server,
        &|o| {
            service_of(o, &fixture).profile == matched("api")
                && http_of(o)
                    == HttpProbe::Skipped {
                        reason: ProbeSkip::KnownNonHttpProcess,
                    }
        },
        "a repository protocol is only a hint",
    );
    let current = server.snapshot();
    server.after_scans(current.revision, 2);
    let sent = sent_by(fixture.received(port_of(&fixture)), &current.instance);
    assert!(sent.is_empty(), "probed on a repository's word: {sent:?}");

    let token = server.token();
    let saved = post_profile(
        &server,
        &token,
        &text(&project),
        &json!({ "version": 1, "services": { "api": { "protocol": "http" } } }),
    );
    assert_eq!(saved.status, 200, "{}", saved.text());
    let result = eventually(APPLIED_WITHIN, || {
        let observation = server.snapshot().observation?;
        match http_of(&observation) {
            HttpProbe::Probed { result, .. } => Some(result),
            _ => None,
        }
    })
    .expect("probed once the user declared HTTP");
    assert!(
        matches!(result, ProbeResult::Response { status: 404, .. }),
        "{result:?}"
    );
    let instance = server.snapshot().instance;
    let first = sent_by(fixture.received(port_of(&fixture)), &instance);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(first[0].starts_with("GET / HTTP/1.1\r\n"), "{first:?}");

    // Verified as HTTP: the next probe ("try HTTP", so the test need not wait 15 s) requests the
    // profile's path.
    let identity = identity(&fixture);
    let body = json!({ "identity": identity, "port": port_of(&fixture) }).to_string();
    let probed = eventually(APPLIED_WITHIN, || {
        let response = server.post_json("/api/probe", &token, &body);
        (response.status == 200).then_some(response)
    })
    .expect("requested probe");
    assert!(probed.text().contains("404"), "{}", probed.text());
    let received = sent_by(fixture.received(port_of(&fixture)), &instance);
    assert_eq!(received.len(), 2, "{received:?}");
    assert!(
        received[1].starts_with("GET /healthz?ready=1 HTTP/1.1\r\n"),
        "{received:?}"
    );
    let (status, _) = server.terminate();
    assert!(status.success());
}

#[test]
fn moved_projects_need_explicit_reassociation() {
    let tree = Sandbox::new().expect("sandbox");
    let old = tree.dir("old/app").expect("dir");
    tree.file("old/app/package.json", "{}").expect("file");
    let config = tree.root().join("config");
    write_central(
        &config,
        &json!({ text(&old): { "version": 1, "services": { "web": { "name": "Web", "match": { "cwd": "." } } } } }),
    );
    let new = tree.root().join("new/app");
    fs::create_dir_all(new.parent().expect("parent")).expect("dir");
    fs::rename(&old, &new).expect("move");
    let new = new.canonicalize().expect("canonical");
    let fixture = spawn(&new);

    let observation = list_json(&config);
    assert_eq!(
        service_of(&observation, &fixture).profile,
        None,
        "never attached by name"
    );
    let stale = metadata_of(&observation, &old).expect("saved profile still listed");
    assert!(!stale.observed);
    assert!(
        stale.services[0].matched.is_empty(),
        "configured, not observed"
    );
    let table = list_table(&config);
    assert!(
        table.contains(&format!("{} (project not observed)", text(&old))),
        "{table}"
    );

    let moved = kosse(&config, &["config", "move", &text(&old), &text(&new)]);
    assert!(
        moved.status.success(),
        "{}",
        String::from_utf8_lossy(&moved.stderr)
    );
    let observation = list_json(&config);
    assert_eq!(service_of(&observation, &fixture).profile, matched("web"));
    assert!(metadata_of(&observation, &old).is_none());

    let again = kosse(&config, &["config", "move", &text(&old), &text(&new)]);
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("no profile is saved"));
}

#[test]
fn worktrees_keep_separate_profiles() {
    let tree = Sandbox::new().expect("sandbox");
    let main = tree.dir("repo").expect("dir");
    init_repo(&main).expect("repo");
    tree.file(
        "repo/kosse.json",
        r#"{"version":1,"services":{"web":{"name":"Repo Web","match":{"cwd":"."}}}}"#,
    )
    .expect("file");
    git(&main, &["add", "-A"]).expect("add");
    git(&main, &["commit", "-q", "-m", "profile"]).expect("commit");
    let linked = tree.root().join("repo-feature");
    git(
        &main,
        &["worktree", "add", "-q", "-b", "feature", &text(&linked)],
    )
    .expect("worktree");
    let linked = linked.canonicalize().expect("canonical");
    let config = tree.root().join("config");
    write_central(
        &config,
        &json!({ text(&linked): { "version": 1, "services": { "web": { "name": "Feature Web" } } } }),
    );
    let (in_main, in_linked) = (spawn(&main), spawn(&linked));

    let observation = list_json(&config);
    let name = |root: &Path| {
        let metadata = metadata_of(&observation, root).expect("metadata");
        (
            metadata.services[0].name.clone(),
            metadata.services[0].sources.name,
        )
    };
    assert_eq!(
        name(&main),
        (Some("Repo Web".into()), Some(ProfileSource::Repository))
    );
    assert_eq!(
        name(&linked),
        (Some("Feature Web".into()), Some(ProfileSource::Central))
    );
    assert_eq!(service_of(&observation, &in_main).profile, matched("web"));
    assert_eq!(service_of(&observation, &in_linked).profile, matched("web"));
}

#[test]
fn config_commands_emit_the_schema_and_locate_the_file() {
    let tree = Sandbox::new().expect("sandbox");
    let config = tree.root().join("config");
    let schema = kosse(&config, &["config", "schema"]);
    assert!(schema.status.success());
    let committed =
        fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/kosse.schema.json"))
            .expect("committed schema");
    assert_eq!(schema.stdout, committed);

    let path = kosse(&config, &["config", "path"]);
    assert!(path.status.success());
    assert_eq!(
        String::from_utf8(path.stdout).expect("UTF-8").trim_end(),
        text(&config.join("config.json"))
    );
    assert!(!config.exists(), "locating the file creates nothing");

    let relative = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", "relative/dir")
        .args(["config", "path"])
        .output()
        .expect("run");
    assert_eq!(relative.status.code(), Some(1));
}

#[test]
fn non_canonical_central_keys_are_reported() {
    let tree = Sandbox::new().expect("sandbox");
    let project = tree.dir("project").expect("dir");
    tree.file("project/package.json", "{}").expect("file");
    // macOS temporary directories live under /private; /var links to /private/var.
    let alias = text(&project)
        .strip_prefix("/private")
        .map(str::to_owned)
        .expect("a /private temporary directory");
    let config = tree.root().join("config");
    write_central(
        &config,
        &json!({ alias.clone(): { "version": 1, "name": "Hand edited" } }),
    );
    let fixture = spawn(&project);
    let observation = list_json(&config);
    assert!(
        observation
            .warnings
            .contains(&ScanWarning::ProfileRootNotCanonical {
                root: alias,
                canonical: text(&project),
            }),
        "{:?}",
        observation.warnings
    );
    assert_eq!(service_of(&observation, &fixture).profile, None);
}
