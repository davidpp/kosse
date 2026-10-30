//! Endpoint names and display tiers (S21) through the real binary: owned fixtures with several
//! ports in temporary Git projects, central profiles in a temporary configuration directory
//! (`KOSSE_CONFIG_DIR`), a running `kosse serve` that probes only those fixtures, and the
//! commands an agent runs. Only fixture rows are asserted on.
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    net::{IpAddr, Ipv4Addr},
    path::Path,
    process::{Command, Output},
    time::Duration,
};

use kosse_core::{
    model::{DisplayBasis, Endpoint, HttpProbe, Observation, ProbeResult, Service, Snapshot},
    profiles::{
        document::EndpointShow,
        view::{ProfileMatch, ProfileSource},
    },
};
use kosse_fixtures::{
    Fixture, FixtureSpec,
    protocol::Serve,
    repo::{Sandbox, init_repo},
};
use serde_json::{Value, json};
use support::{Server, eventually, markdown_tables, sent_by};

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
/// First scan, the probes and the next scan.
const PROBED_WITHIN: Duration = Duration::from_secs(15);

fn kosse(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", config)
        .args(args)
        .output()
        .expect("run kosse")
}

fn write_central(config: &Path, root: &Path, services: &Value) {
    std::fs::create_dir_all(config).expect("config dir");
    let root = root.to_str().expect("UTF-8 root");
    let document =
        json!({ "version": 1, "projects": { root: { "version": 1, "services": services } } });
    std::fs::write(config.join("config.json"), document.to_string()).expect("write config");
}

fn project(tree: &Sandbox, name: &str) -> std::path::PathBuf {
    let dir = tree.dir(name).expect("dir");
    init_repo(&dir).expect("repo");
    dir
}

/// A fixture listening on `ports` loopback ports, answering every connection with `serve`.
fn spawn(dir: &Path, ports: usize, serve: Serve, name: Option<&str>) -> Fixture {
    let mut spec = FixtureSpec::new().serve(serve).record().cwd(dir);
    for _ in 0..ports {
        spec = spec.listen(&[V4]);
    }
    if let Some(name) = name {
        spec = spec.executable_name(name);
    }
    Fixture::spawn(&spec).expect("fixture")
}

fn ports(fixture: &Fixture) -> Vec<u16> {
    let mut ports: Vec<u16> = fixture
        .record()
        .endpoints
        .iter()
        .map(std::net::SocketAddr::port)
        .collect();
    ports.sort_unstable();
    ports
}

fn service_of<'a>(observation: &'a Observation, fixture: &Fixture) -> Option<&'a Service> {
    observation
        .services
        .iter()
        .find(|service| service.process.identity.pid == fixture.pid())
}

fn endpoint_of(service: &Service, port: u16) -> &Endpoint {
    service
        .endpoints
        .iter()
        .find(|endpoint| endpoint.port == port)
        .unwrap_or_else(|| panic!("no endpoint {port}"))
}

fn answered(endpoint: &Endpoint) -> bool {
    matches!(
        endpoint.http,
        HttpProbe::Probed {
            result: ProbeResult::Response { .. },
            ..
        }
    )
}

/// The instance's snapshot once every port of `fixture` answered HTTP and it matches `id`.
fn probed(server: &Server, fixture: &Fixture, id: &str) -> Snapshot {
    eventually(PROBED_WITHIN, || {
        let snapshot = server.snapshot();
        let service = service_of(snapshot.observation.as_ref()?, fixture)?;
        let matched = service.profile
            == Some(ProfileMatch::Matched {
                service: id.to_owned(),
            });
        (matched && service.endpoints.iter().all(answered)).then_some(snapshot)
    })
    .unwrap_or_else(|| panic!("{id}: every port answered the instance's probes"))
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The issue's acceptance: one process with two ports that both answer HTTP. Listing one as the
/// summary and the rest as details makes `kosse url <service>` answer with the summary port
/// (before, it was ambiguous), and the table says which is which.
#[test]
fn a_summary_endpoint_is_the_one_launch_target_of_its_service() {
    let tree = Sandbox::new().expect("sandbox");
    let root = project(&tree, "storefront");
    let fixture = spawn(&root, 2, Serve::Title, None);
    let [site, internal] = ports(&fixture)[..] else {
        panic!("two ports");
    };
    let config = tree.root().join("config");
    write_central(
        &config,
        &root,
        &json!({ "web": {
            "match": { "cwd": "." },
            "endpoints": { site.to_string(): { "name": "Site" } },
            "otherEndpoints": { "show": "details" }
        } }),
    );
    let server = Server::start_configured(&config, &[&format!("--probe-only={}", fixture.pid())]);
    let snapshot = probed(&server, &fixture, "web");
    let service = service_of(
        snapshot.observation.as_ref().expect("observation"),
        &fixture,
    )
    .expect("fixture observed");
    let site_display = &endpoint_of(service, site).display;
    assert_eq!(site_display.name.as_deref(), Some("Site"));
    assert_eq!(site_display.show, EndpointShow::Summary);
    assert_eq!(site_display.basis, DisplayBasis::Endpoint);
    assert_eq!(site_display.source, Some(ProfileSource::Central));
    let internal_display = &endpoint_of(service, internal).display;
    assert_eq!(internal_display.show, EndpointShow::Details);
    assert_eq!(internal_display.basis, DisplayBasis::OtherEndpoints);

    let instance = server.addr.port().to_string();
    let url = kosse(&config, &["url", "web", "--instance-port", &instance]);
    assert!(url.status.success(), "{}", stderr(&url));
    assert_eq!(stdout(&url), format!("http://localhost:{site}/\n"));
    let explicit = kosse(
        &config,
        &["url", &internal.to_string(), "--instance-port", &instance],
    );
    assert!(explicit.status.success(), "{}", stderr(&explicit));
    assert_eq!(
        stdout(&explicit),
        format!("http://localhost:{internal}/\n"),
        "an explicit port answers for a details endpoint"
    );

    let list = kosse(&config, &["list", "--instance-port", &instance]);
    let table = markdown_tables(&stdout(&list)).remove(0);
    let show = |port: u16| {
        let rows = table.rows_where("PORT", &port.to_string());
        table.cell(rows[0], "SHOW").to_owned()
    };
    assert_eq!(show(site), "summary · Site");
    assert_eq!(show(internal), "details");
    let (status, _) = server.terminate();
    assert!(status.success());
}

/// A child whose ports change on every start (workerd) is matched by its process name, and its
/// unlisted endpoints stay in details across restarts without editing the profile. With no
/// summary endpoint it has no launch target: `kosse url <service>` says so and names its ports.
#[test]
fn a_process_match_keeps_changing_ports_in_details() {
    let tree = Sandbox::new().expect("sandbox");
    let root = project(&tree, "worker-site");
    let config = tree.root().join("config");
    write_central(
        &config,
        &root,
        &json!({ "worker": {
            "match": { "cwd": ".", "process": "workerd" },
            "otherEndpoints": { "show": "details" }
        } }),
    );
    let mut first = spawn(&root, 2, Serve::Title, Some("workerd"));
    let server = Server::start_configured(&config, &[&format!("--probe-only={}", first.pid())]);
    let instance = server.addr.port().to_string();
    let snapshot = probed(&server, &first, "worker");
    let service = service_of(snapshot.observation.as_ref().expect("observation"), &first)
        .expect("fixture observed");
    assert_eq!(service.process.name.as_deref(), Some("workerd"));
    assert!(
        service
            .endpoints
            .iter()
            .all(|endpoint| endpoint.display.show == EndpointShow::Details)
    );

    let url = kosse(&config, &["url", "worker", "--instance-port", &instance]);
    assert_eq!(url.status.code(), Some(1), "{}", stderr(&url));
    assert_eq!(stdout(&url), "");
    for port in ports(&first) {
        assert!(
            stderr(&url).contains(&format!(":{port}")),
            "{}",
            stderr(&url)
        );
    }
    let port = ports(&first)[0];
    let explicit = kosse(
        &config,
        &["url", &port.to_string(), "--instance-port", &instance],
    );
    assert!(explicit.status.success(), "{}", stderr(&explicit));
    assert_eq!(stdout(&explicit), format!("http://localhost:{port}/\n"));

    first.stop().expect("stop the first fixture");
    let second = spawn(&root, 2, Serve::Title, Some("workerd"));
    assert_ne!(ports(&second), ports(&first), "new ephemeral ports");
    let restarted = eventually(PROBED_WITHIN, || {
        let snapshot = server.snapshot();
        let service = service_of(snapshot.observation.as_ref()?, &second)?.clone();
        (service.profile.is_some()).then_some(service)
    })
    .expect("the restarted fixture is observed and matched");
    assert_eq!(
        restarted.profile,
        Some(ProfileMatch::Matched {
            service: "worker".into()
        })
    );
    for endpoint in &restarted.endpoints {
        assert_eq!(endpoint.display.show, EndpointShow::Details, "{endpoint:?}");
        assert_eq!(endpoint.display.basis, DisplayBasis::OtherEndpoints);
    }
    let url = kosse(&config, &["url", "worker", "--instance-port", &instance]);
    assert_eq!(url.status.code(), Some(1), "{}", stderr(&url));
    let (status, _) = server.terminate();
    assert!(status.success());
}

/// A directory-wide entry and a process entry in the same project: the process entry is the more
/// specific one, so the child and its parent each match exactly one entry.
#[test]
fn a_process_entry_takes_its_process_from_a_directory_entry() {
    let tree = Sandbox::new().expect("sandbox");
    let root = project(&tree, "stack");
    let parent = spawn(&root, 1, Serve::Banner, None);
    let child = spawn(&root, 1, Serve::Banner, Some("workerd"));
    let config = tree.root().join("config");
    write_central(
        &config,
        &root,
        &json!({
            "site": { "match": { "cwd": "." } },
            "worker": { "match": { "process": "workerd" } }
        }),
    );
    let output = kosse(&config, &["list", "--json", "--instance-port", "0"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let snapshot: Snapshot = serde_json::from_slice(&output.stdout).expect("snapshot");
    let observation = snapshot.observation.expect("observation");
    let matched = |id: &str| {
        Some(ProfileMatch::Matched {
            service: id.to_owned(),
        })
    };
    assert_eq!(
        service_of(&observation, &parent).expect("parent").profile,
        matched("site")
    );
    assert_eq!(
        service_of(&observation, &child).expect("child").profile,
        matched("worker")
    );

    let preview = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("KOSSE_CONFIG_DIR", &config)
        .args([
            "profile",
            "set",
            root.to_str().expect("UTF-8"),
            "--dry-run",
            "--instance-port",
            "0",
            "--file",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(
                    br#"{"version":1,"services":{"site":{"match":{"cwd":"."}},"worker":{"match":{"process":"workerd"}}}}"#,
                )?;
            child.wait_with_output()
        })
        .expect("preview");
    assert!(
        preview.status.success(),
        "a dry run exits 1 only for an ambiguous process: {}{}",
        stdout(&preview),
        stderr(&preview)
    );
}

/// An endpoint's own `probePath` is requested on that endpoint only, on either tier; the
/// service's `probePath` is the health path of its summary endpoints, never a details one's.
#[test]
fn health_paths_follow_the_endpoint_they_check() {
    let tree = Sandbox::new().expect("sandbox");
    let root = project(&tree, "api");
    let fixture = spawn(&root, 3, Serve::NotFound, None);
    let [own, details, plain] = ports(&fixture)[..] else {
        panic!("three ports");
    };
    let config = tree.root().join("config");
    write_central(
        &config,
        &root,
        &json!({ "api": {
            "match": { "cwd": "." },
            "probePath": "/service-health",
            "endpoints": {
                own.to_string(): { "show": "details", "probePath": "/own-health" },
                details.to_string(): { "show": "details" }
            }
        } }),
    );
    let server = Server::start_configured(&config, &[&format!("--probe-only={}", fixture.pid())]);
    let snapshot = probed(&server, &fixture, "api");
    let token = server.token();
    let identity = service_of(
        snapshot.observation.as_ref().expect("observation"),
        &fixture,
    )
    .expect("fixture")
    .process
    .identity;
    // Verified as HTTP by `GET /`; the next probe of each port ("try HTTP", rather than waiting
    // for the automatic interval) requests its configured path.
    for port in [own, details, plain] {
        let body = json!({ "identity": identity, "port": port }).to_string();
        eventually(PROBED_WITHIN, || {
            let response = server.post_json("/api/probe", &token, &body);
            (response.status == 200).then_some(())
        })
        .unwrap_or_else(|| panic!("requested probe of {port}"));
    }
    let second_request = |port: u16| {
        let received = sent_by(fixture.received(port), &snapshot.instance);
        assert_eq!(received.len(), 2, "{port}: {received:?}");
        assert!(
            received[0].starts_with("GET / HTTP/1.1\r\n"),
            "{received:?}"
        );
        received[1].lines().next().unwrap_or_default().to_owned()
    };
    assert_eq!(second_request(own), "GET /own-health HTTP/1.1");
    assert_eq!(
        second_request(details),
        "GET / HTTP/1.1",
        "the service's health path is not a details endpoint's"
    );
    assert_eq!(second_request(plain), "GET /service-health HTTP/1.1");
    let (status, _) = server.terminate();
    assert!(status.success());
}
