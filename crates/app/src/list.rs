//! `kosse list`: a snapshot (see [`crate::scope::observe`]), filtered, printed as Markdown tables
//! or as the JSON snapshot contract.
//!
//! The service table has one row per listening port, with the columns [`COLUMNS`]:
//! - `PROJECT`: project name, plus `/<package path>` when the nearest package is not the root;
//! - `BRANCH`: branch, `detached@<commit>`, `?` when Git did not say, `-` outside Git;
//! - `SERVICE`: the configured service, `Name (id) [source]`, or `ambiguous: a, b`;
//! - `PORT`;
//! - `SHOW`: `summary` (a launch target) or `details`, then ` · <name>` when the endpoint is named
//!   and ` (inferred)` for the default inspector port no profile describes;
//! - `HTTP`: `<status> <url>` when it answered HTTP, otherwise the probe state (`not probed (no
//!   instance)` for a candidate of a one-shot scan, which never probes);
//! - `PROCESS`: the OS process name;
//! - `PID.START`: the process identity (PID and start time in Unix seconds), as actions take it;
//! - `AGE`: time since the process started.
//!
//! A `## Configured, not observed` table follows when configured services match no process
//! (`PROJECT` directory name, `SERVICE`, `SOURCE`, `MATCH`). Unknown cells print as `-`; warnings go to
//! stderr. Command lines appear only in `--json`, redacted.

use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

use kosse_core::{
    model::{
        DisplayBasis, Endpoint, EndpointDisplay, GitHead, HttpProbe, Observation, ProbeResult,
        ProbeSkip, ProcessIdentity, Project, Service, Snapshot, TlsFailure,
    },
    profiles::{
        document::{EndpointShow, ServiceMatch},
        view::{ConfiguredService, ProfileMatch, ProjectMetadata},
    },
};

use crate::{
    cli::ListArgs,
    failure::{self, Failure},
    markdown::{Table, UNKNOWN},
    prober,
    scope::{self, Filter},
    text::printable,
};

pub const COLUMNS: [&str; 9] = [
    "PROJECT",
    "BRANCH",
    "SERVICE",
    "PORT",
    "SHOW",
    "HTTP",
    "PROCESS",
    "PID.START",
    "AGE",
];

/// The `HTTP` cell of a probe candidate in an observation no prober follows (a one-shot scan).
pub const NOT_PROBED: &str = "not probed (no instance)";

pub fn run(args: &ListArgs) -> ExitCode {
    failure::exit(list(args), args.json)
}

fn list(args: &ListArgs) -> Result<(), Failure> {
    let mut snapshot = scope::observe(args.instance.instance_port, args.json)?;
    let filter = Filter {
        root: scope::root(&args.scope, &snapshot)?,
        port: args.port,
        service: args.service.clone(),
    };
    filter.apply(&mut snapshot);
    let mut stdout = io::stdout().lock();
    if args.json {
        return failure::written(write_json(&mut stdout, &snapshot));
    }
    // A one-shot scan always yields an observation, and an instance's snapshot without one is
    // never used (see `scope::observe`).
    let Some(observation) = snapshot.observation.as_ref() else {
        return Ok(());
    };
    failure::written(
        services_table(observation, args.port, probes_run(&snapshot))
            .write(&mut stdout)
            .and_then(|()| write_unobserved(&mut stdout, &observation.profiles)),
    )?;
    for warning in &observation.warnings {
        // Profile warnings can quote a repository file: untrusted text.
        report!("kosse: warning: {}", printable(&warning.to_string()));
    }
    Ok(())
}

pub fn write_json(out: &mut impl Write, value: &impl serde::Serialize) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, value)?;
    out.write_all(b"\n")
}

/// Whether HTTP probes run for this snapshot: a running instance's do, a one-shot scan's never.
pub fn probes_run(snapshot: &Snapshot) -> bool {
    snapshot.scanner.interval_ms.is_some()
}

/// One row per port of each service (only `port` when given). `probes_run` is false for an
/// observation no prober follows, so a candidate reads `not probed (no instance)` rather than
/// `pending`.
pub fn services_table(observation: &Observation, port: Option<u16>, probes_run: bool) -> Table<9> {
    let projects: HashMap<&str, &Project> = observation
        .projects
        .iter()
        .map(|project| (project.root.as_str(), project))
        .collect();
    let mut table = Table::new(COLUMNS);
    for service in &observation.services {
        let project = service
            .project
            .as_deref()
            .and_then(|root| projects.get(root).copied());
        let (project_label, branch) = labels(service, project);
        let configured = configured_label(service, &observation.profiles);
        let process = service.process.name.as_deref().unwrap_or(UNKNOWN);
        let identity = identity_label(service.process.identity);
        let age = service
            .process
            .age_secs
            .map_or_else(|| UNKNOWN.to_owned(), age);
        for endpoint in &service.endpoints {
            if port.is_some_and(|port| port != endpoint.port) {
                continue;
            }
            table.row([
                &project_label,
                &branch,
                &configured,
                &endpoint.port.to_string(),
                &show_label(&endpoint.display),
                &http_state(endpoint, probes_run),
                process,
                &identity,
                &age,
            ]);
        }
    }
    table
}

/// The URL of an endpoint that answered HTTP, any status: always `prober::open_url`'s.
pub fn reachable_url(endpoint: &Endpoint) -> Option<String> {
    match &endpoint.http {
        HttpProbe::Probed {
            result: ProbeResult::Response { tls, .. },
            ..
        } => Some(prober::open_url(*tls, endpoint.port)),
        _ => None,
    }
}

/// The `SHOW` cell: `summary`, `details · Inspector (inferred)`, `summary · Site`.
pub fn show_label(display: &EndpointDisplay) -> String {
    let mut label = match display.show {
        EndpointShow::Summary => "summary",
        EndpointShow::Details => "details",
    }
    .to_owned();
    if let Some(name) = &display.name {
        let _ = write!(label, " · {name}");
    }
    if display.basis == DisplayBasis::InspectorPort {
        label.push_str(" (inferred)");
    }
    label
}

/// `200 http://localhost:3000/`, or what stands in the way: `pending` (`not probed (no instance)`
/// when no prober runs), `skipped: …`, `timed out`.
pub fn http_state(endpoint: &Endpoint, probes_run: bool) -> String {
    let result = match &endpoint.http {
        HttpProbe::Skipped { reason } => {
            return format!(
                "skipped: {}",
                match reason {
                    ProbeSkip::NotOnLoopback => "not on loopback",
                    ProbeSkip::OtherUser => "other user",
                    ProbeSkip::KnownNonHttpPort => "non-HTTP port",
                    ProbeSkip::KnownNonHttpProcess => "non-HTTP process",
                    ProbeSkip::NotAttributed => "not in a project",
                }
            );
        }
        HttpProbe::Pending if probes_run => return "pending".to_owned(),
        HttpProbe::Pending => return NOT_PROBED.to_owned(),
        HttpProbe::Probed { result, .. } => result,
    };
    match result {
        ProbeResult::Response { status, tls, .. } => {
            format!("{status} {}", prober::open_url(*tls, endpoint.port))
        }
        ProbeResult::TimedOut => "timed out".to_owned(),
        ProbeResult::Refused => "refused".to_owned(),
        ProbeResult::Closed => "closed without answer".to_owned(),
        ProbeResult::TlsFailed { failure, .. } => format!(
            "TLS failed: {}",
            match failure {
                TlsFailure::Certificate => "certificate",
                TlsFailure::Handshake => "handshake",
            }
        ),
        ProbeResult::NotHttp => "not HTTP".to_owned(),
        ProbeResult::InvalidPath => "invalid probe path".to_owned(),
    }
}

/// The `PID.START` cell, as `stop` and `force` take it: `<pid>.<start>`, `-` for an unknown start.
pub fn identity_label(identity: ProcessIdentity) -> String {
    format!(
        "{}.{}",
        identity.pid,
        identity
            .started_at_secs
            .map_or_else(|| UNKNOWN.to_owned(), |start| start.to_string())
    )
}

/// `cwd apps/web, port 5173`: what a configured entry's `match` narrows.
pub fn matcher_label(matcher: &ServiceMatch) -> String {
    let mut parts = Vec::new();
    if let Some(cwd) = &matcher.cwd {
        parts.push(format!("cwd {cwd}"));
    }
    if let Some(port) = matcher.port {
        parts.push(format!("port {port}"));
    }
    if let Some(process) = &matcher.process {
        parts.push(format!("process {process}"));
    }
    parts.join(", ")
}

/// `ambiguous: api, web`: a process several entries match.
pub fn ambiguous_label(services: &[String]) -> String {
    format!("ambiguous: {}", services.join(", "))
}

/// `42s`, `5m`, `3h`, `2d`: the largest whole unit.
fn age(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// The configured service of a process and the source of its name (or of its match):
/// `Frontend (web) [central]`, `ambiguous: api, web`, or `-`.
pub fn configured_label(service: &Service, profiles: &[ProjectMetadata]) -> String {
    match &service.profile {
        None => UNKNOWN.to_owned(),
        Some(ProfileMatch::Ambiguous { services }) => ambiguous_label(services),
        Some(ProfileMatch::Matched { service: id }) => {
            configured_entry(service, profiles, id).map_or_else(|| id.clone(), service_label)
        }
    }
}

pub fn configured_entry<'a>(
    service: &Service,
    profiles: &'a [ProjectMetadata],
    id: &str,
) -> Option<&'a ConfiguredService> {
    scope::project_entries(service, profiles).find(|entry| entry.id == id)
}

fn service_label(entry: &ConfiguredService) -> String {
    let source = entry.sources.name.or(entry.sources.matcher);
    let mut label = match &entry.name {
        Some(name) => format!("{name} ({})", entry.id),
        None => entry.id.clone(),
    };
    if let Some(source) = source {
        let _ = write!(label, " [{source}]");
    }
    label
}

/// Configured services no process matches, and saved profiles of projects not observed. They are
/// neither running nor known to be stopped.
fn write_unobserved(out: &mut impl Write, profiles: &[ProjectMetadata]) -> io::Result<()> {
    let mut table = Table::new(["PROJECT", "SERVICE", "SOURCE", "MATCH"]);
    for metadata in profiles {
        let pending: Vec<&ConfiguredService> = metadata
            .services
            .iter()
            .filter(|entry| !entry.disabled && entry.matched.is_empty())
            .collect();
        // The directory name, as the service table shows it; `--json` has the root.
        let name = Path::new(&metadata.root).file_name().map_or_else(
            || metadata.root.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
        let project = if metadata.observed {
            name
        } else {
            format!("{name} (project not observed)")
        };
        if pending.is_empty() && !metadata.observed {
            let sources: Vec<String> = metadata.sources.iter().map(ToString::to_string).collect();
            table.row([&project, UNKNOWN, &sources.join("+"), UNKNOWN]);
        }
        for entry in pending {
            let source = entry
                .sources
                .matcher
                .or(entry.sources.name)
                .map_or_else(|| UNKNOWN.to_owned(), |source| source.to_string());
            let matcher = entry
                .matcher
                .as_ref()
                .map_or_else(|| "no match (declared only)".to_owned(), matcher_label);
            table.row([&project, &service_label(entry), &source, &matcher]);
        }
    }
    if table.is_empty() {
        return Ok(());
    }
    writeln!(out, "\n## Configured, not observed\n")?;
    table.write(out)
}

/// `project/package/path` and the branch (or `detached@commit`); `-` when unknown.
fn labels(service: &Service, project: Option<&Project>) -> (String, String) {
    let Some(project) = project else {
        return (UNKNOWN.to_owned(), UNKNOWN.to_owned());
    };
    let mut label = project.name.clone();
    if let Some(package) = service
        .package
        .as_ref()
        .filter(|package| !package.path.is_empty())
    {
        label.push('/');
        label.push_str(&package.path);
    }
    let branch = match project.git.as_ref().map(|git| &git.head) {
        Some(GitHead::Branch { name }) => name.clone(),
        Some(GitHead::Detached { commit }) => format!("detached@{commit}"),
        Some(GitHead::Unknown) => "?".to_owned(),
        None => UNKNOWN.to_owned(),
    };
    (label, branch)
}
