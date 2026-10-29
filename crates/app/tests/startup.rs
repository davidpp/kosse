//! `kosse startup` and `kosse doctor` through the real binary.
//!
//! Login startup is sandboxed: `HOME` points at a temporary directory (so the launch agent is
//! written under it, never to the real `~/Library/LaunchAgents`) and the debug-only
//! `KOSSE_TEST_LAUNCHCTL` replaces `/bin/launchctl` with a script that records its arguments and
//! answers `print` from what it recorded, so no test loads, unloads or inspects a real agent. The
//! generated plist itself is checked with the system's `plutil`. A real login cycle is a human
//! check (docs/evidence/S09.md).
#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod support;

use std::{
    fs,
    net::{Ipv4Addr, TcpListener},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use kosse_fixtures::repo::Sandbox;
use serde_json::Value;
use support::{NO_CONFIG, Server};

const LABEL: &str = "com.davidpp.kosse";

struct Home {
    sandbox: Sandbox,
    launchctl: PathBuf,
    log: PathBuf,
}

impl Home {
    fn new() -> Self {
        let sandbox = Sandbox::new().expect("sandbox");
        let log = sandbox.root().join("launchctl.log");
        let loaded = sandbox.root().join("loaded");
        // `print` answers like launchctl for a loaded, running job (exit 0, `pid = …`) or an
        // unknown one (exit 113); `bootstrap` fails with launchd's EIO when `fail-bootstrap`
        // exists, and once (the marker is consumed) when `fail-bootstrap-once` does.
        let script = format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
case "$1" in
  print) [ -f '{loaded}' ] || exit 113; printf '\tstate = running\n\tpid = 4242\n' ;;
  bootstrap)
    [ -f '{root}/fail-bootstrap' ] && {{ echo 'Bootstrap failed: 5: Input/output error' >&2; exit 5; }}
    [ -f '{root}/fail-bootstrap-once' ] && {{ rm -f '{root}/fail-bootstrap-once'; echo 'Bootstrap failed: 5: Input/output error' >&2; exit 5; }}
    touch '{loaded}' ;;
  bootout) rm -f '{loaded}' ;;
esac
"#,
            log = log.display(),
            loaded = loaded.display(),
            root = sandbox.root().display(),
        );
        let launchctl = sandbox.file("launchctl.sh", &script).expect("script");
        fs::set_permissions(&launchctl, fs::Permissions::from_mode(0o700)).expect("chmod");
        Self {
            sandbox,
            launchctl,
            log,
        }
    }

    fn plist(&self) -> PathBuf {
        self.sandbox
            .root()
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    }

    fn kosse(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_kosse"))
            .env("HOME", self.sandbox.root())
            .env("KOSSE_TEST_LAUNCHCTL", &self.launchctl)
            .env("KOSSE_CONFIG_DIR", NO_CONFIG)
            .args(args)
            .output()
            .expect("run kosse")
    }

    /// The `launchctl` calls other than `print`.
    fn changes(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.starts_with("print "))
            .map(str::to_owned)
            .collect()
    }
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn uid() -> String {
    let output = Command::new("/usr/bin/id").arg("-u").output().expect("id");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn plutil(plist: &Path, key: &str) -> String {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .output()
        .expect("plutil");
    assert!(output.status.success(), "{key}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn startup_enable_writes_a_valid_agent_for_this_binary_and_disable_removes_it() {
    let home = Home::new();
    let binary = env!("CARGO_BIN_EXE_kosse");
    let domain = format!("gui/{}", uid());

    assert_eq!(stdout(&home.kosse(&["startup", "status"])), "disabled");
    assert_eq!(home.changes(), Vec::<String>::new());

    let enabled = stdout(&home.kosse(&["startup", "enable"]));
    assert!(
        enabled.starts_with(&format!("startup enabled: {binary} --no-open")),
        "{enabled}"
    );
    let plist = home.plist();
    let lint = Command::new("/usr/bin/plutil")
        .arg("-lint")
        .arg(&plist)
        .output()
        .expect("plutil -lint");
    assert!(lint.status.success(), "{lint:?}");
    assert_eq!(plutil(&plist, "Label"), LABEL);
    assert_eq!(plutil(&plist, "ProgramArguments.0"), binary);
    assert_eq!(plutil(&plist, "ProgramArguments.1"), "--no-open");
    assert_eq!(plutil(&plist, "ProgramArguments"), "2");
    assert_eq!(plutil(&plist, "RunAtLoad"), "true");
    assert_eq!(plutil(&plist, "LimitLoadToSessionType"), "Aqua");
    // No KeepAlive: Quit stays quit until the next login.
    let keep_alive = Command::new("/usr/bin/plutil")
        .args(["-extract", "KeepAlive", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .expect("plutil");
    assert!(!keep_alive.status.success());
    assert_eq!(
        fs::metadata(&plist).expect("plist").permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(
        home.changes(),
        [format!("bootstrap {domain} {}", plist.display())]
    );

    let status = stdout(&home.kosse(&["startup", "status"]));
    assert!(
        status.starts_with(&format!(
            "enabled at login: {binary} --no-open (loaded, running as PID 4242;"
        )),
        "{status}"
    );

    // Enabling again changes nothing.
    let again = stdout(&home.kosse(&["startup", "enable"]));
    assert!(again.starts_with("startup already enabled"), "{again}");
    assert_eq!(home.changes().len(), 1);

    let disabled = stdout(&home.kosse(&["startup", "disable"]));
    assert!(disabled.starts_with("startup disabled"), "{disabled}");
    assert!(!plist.exists());
    assert_eq!(home.changes()[1..], [format!("bootout {domain}/{LABEL}")]);
    assert_eq!(stdout(&home.kosse(&["startup", "status"])), "disabled");
    assert_eq!(
        stdout(&home.kosse(&["startup", "disable"])),
        "startup was not enabled"
    );
    assert_eq!(home.changes().len(), 2);
}

#[test]
fn a_failed_bootstrap_leaves_no_agent_behind() {
    let home = Home::new();
    home.sandbox.file("fail-bootstrap", "").expect("marker");
    let output = home.kosse(&["startup", "enable"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("launchctl bootstrap failed") && stderr.contains("Input/output error"),
        "{stderr}"
    );
    assert!(!home.plist().exists());
    assert_eq!(stdout(&home.kosse(&["startup", "status"])), "disabled");
}

/// A plist of another kosse (as an older install or a hand edit would leave it).
fn write_agent(plist: &Path, program: &Path) {
    fs::create_dir_all(plist.parent().expect("dir")).expect("agents dir");
    let status = Command::new("/usr/bin/plutil")
        .args(["-create", "xml1"])
        .arg(plist)
        .status()
        .expect("plutil -create");
    assert!(status.success());
    for args in [
        vec!["-insert", "Label", "-string", LABEL],
        vec!["-insert", "ProgramArguments", "-array"],
        vec![
            "-insert",
            "ProgramArguments.0",
            "-string",
            program.to_str().expect("utf-8"),
        ],
    ] {
        let status = Command::new("/usr/bin/plutil")
            .args(&args)
            .arg(plist)
            .status()
            .expect("plutil");
        assert!(status.success());
    }
}

#[test]
fn status_and_doctor_flag_an_agent_whose_program_is_gone() {
    let home = Home::new();
    let missing = home.sandbox.root().join("gone/kosse");
    write_agent(&home.plist(), &missing);
    let status = stdout(&home.kosse(&["startup", "status"]));
    assert!(
        status.contains(&format!("but its program {} is missing", missing.display())),
        "{status}"
    );

    let doctor = home.kosse(&["doctor", "--json", "--port", "0"]);
    let report: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json");
    let startup = check(&report, "startup");
    assert_eq!(startup["level"], "warn", "{startup}");
    assert_eq!(home.changes(), Vec::<String>::new());
}

fn check<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["name"] == name)
        .unwrap_or_else(|| panic!("no {name} check in {report}"))
}

fn doctor(home: &Home, config: &Path, port: u16) -> (Option<i32>, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_kosse"))
        .env("HOME", home.sandbox.root())
        .env("KOSSE_TEST_LAUNCHCTL", &home.launchctl)
        .env("KOSSE_CONFIG_DIR", config)
        .args(["doctor", "--json", "--port", &port.to_string()])
        .output()
        .expect("run doctor");
    let report = serde_json::from_slice(&output.stdout).expect("doctor json");
    (output.status.code(), report)
}

#[test]
fn doctor_reports_the_config_and_port_without_changing_them() {
    let home = Home::new();
    let config = home.sandbox.dir("config").expect("config dir");
    fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).expect("chmod");

    // Nothing saved yet and a free port: every check passes.
    let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let free_port = free.local_addr().expect("addr").port();
    drop(free);
    let (code, report) = doctor(&home, &config, free_port);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["ok"], true);
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(check(&report, "platform")["level"], "ok");
    assert_eq!(check(&report, "config")["level"], "ok");
    assert!(
        check(&report, "config")["detail"]
            .as_str()
            .is_some_and(|detail| detail.ends_with("config.json: no profiles saved yet")),
        "{report}"
    );
    assert_eq!(
        check(&report, "port")["detail"],
        format!("127.0.0.1:{free_port} is free")
    );
    assert_eq!(check(&report, "startup")["detail"], "disabled");

    // A port held by something that is not Kossé.
    let other = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let other_port = other.local_addr().expect("addr").port();
    let (code, report) = doctor(&home, &config, other_port);
    assert_eq!(code, Some(0));
    assert_eq!(check(&report, "port")["level"], "warn");
    assert!(
        check(&report, "port")["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("is in use by a process that is not Kossé")),
        "{report}"
    );
    drop(other);

    // A running Kossé on the port is identified, with its mode.
    let server = Server::start();
    let (_, report) = doctor(&home, &config, server.addr.port());
    assert_eq!(
        check(&report, "port")["detail"],
        format!(
            "Kossé {} (headless) is running at http://{}/",
            env!("CARGO_PKG_VERSION"),
            server.addr
        )
    );
    let _ = server.terminate();

    // A corrupt configuration fails the check, and is left exactly as it was.
    let file = config.join("config.json");
    fs::write(&file, "{ not json").expect("write");
    let (code, report) = doctor(&home, &config, free_port);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["ok"], false);
    assert_eq!(check(&report, "config")["level"], "fail");
    assert_eq!(fs::read_to_string(&file).expect("read"), "{ not json");

    // A valid file readable by other users is a warning.
    fs::write(&file, r#"{"version": 1}"#).expect("write");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("chmod");
    let (code, report) = doctor(&home, &config, free_port);
    assert_eq!(code, Some(0));
    assert_eq!(check(&report, "config")["level"], "warn", "{report}");
    assert_eq!(home.changes(), Vec::<String>::new());
}

#[test]
fn doctor_prints_one_line_per_check() {
    let home = Home::new();
    let output = home.kosse(&["doctor", "--port", "0"]);
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(format!("Kossé {} doctor", env!("CARGO_PKG_VERSION")).as_str())
    );
    let names: Vec<&str> = lines
        .map(|line| line.split_whitespace().nth(1).unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        [
            "platform", "binary", "config", "port", "git", "lsof", "startup", "gui"
        ]
    );
}

#[test]
fn enabling_over_a_loaded_agent_of_another_kosse_unloads_it_first() {
    let home = Home::new();
    let domain = format!("gui/{}", uid());
    let plist = home.plist();
    write_agent(&plist, &home.sandbox.root().join("old/kosse"));
    home.sandbox.file("loaded", "").expect("loaded");
    // launchd still tearing the old job down: the first bootstrap gets EIO and is retried.
    home.sandbox
        .file("fail-bootstrap-once", "")
        .expect("marker");

    let enabled = stdout(&home.kosse(&["startup", "enable"]));
    assert!(enabled.starts_with("startup enabled"), "{enabled}");
    assert_eq!(
        plutil(&plist, "ProgramArguments.0"),
        env!("CARGO_BIN_EXE_kosse")
    );
    let bootstrap = format!("bootstrap {domain} {}", plist.display());
    assert_eq!(
        home.changes(),
        [
            format!("bootout {domain}/{LABEL}"),
            bootstrap.clone(),
            bootstrap
        ]
    );
}

#[test]
fn a_failed_replacement_says_the_previous_agent_was_unloaded() {
    let home = Home::new();
    let plist = home.plist();
    write_agent(&plist, &home.sandbox.root().join("old/kosse"));
    home.sandbox.file("loaded", "").expect("loaded");
    home.sandbox.file("fail-bootstrap", "").expect("marker");

    let output = home.kosse(&["startup", "enable"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Input/output error")
            && stderr.contains("the previous agent was unloaded")
            && stderr.contains("startup is now off"),
        "{stderr}"
    );
    assert!(!plist.exists());
    // One bootout, then the first bootstrap and its retries.
    let changes = home.changes();
    assert!(changes[0].starts_with("bootout "), "{changes:?}");
    assert_eq!(changes.len(), 5, "{changes:?}");
    assert_eq!(stdout(&home.kosse(&["startup", "status"])), "disabled");
}
