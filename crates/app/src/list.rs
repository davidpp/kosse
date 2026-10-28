//! `kosse list`: a snapshot, printed as a table or as the JSON snapshot contract.
//!
//! When a verified Kossé instance runs on the port (see [`crate::instance`]) and its snapshot has
//! this build's schema version, that snapshot is printed: no second scan. Otherwise this runs its
//! own one-shot scan (revision 1, no interval), with the central and repository profiles applied
//! as `serve` applies them. The table names each process's configured service with its source,
//! then lists configured services that no process matches as "configured, not observed".

use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{self, Write},
};

use kosse_core::{
    model::{GitHead, Observation, Project, Service, Snapshot},
    profiles::{
        CentralStore, Profiles, StoreError,
        view::{ConfiguredService, ProfileMatch, ProjectMetadata},
    },
    scan::Scanner,
    snapshot::{AttemptTimer, SnapshotTracker},
    sockets::SocketsError,
};

use crate::{
    cli::ListArgs,
    instance::{self, Found},
    text::printable,
};

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error(transparent)]
    Scan(#[from] SocketsError),
    #[error(transparent)]
    Config(#[from] StoreError),
    #[error("could not write the listing: {0}")]
    Write(#[source] io::Error),
}

pub fn run(args: &ListArgs) -> Result<(), ListError> {
    let snapshot = match running_snapshot(args.port) {
        Some(snapshot) => snapshot,
        None => scan_once()?,
    };
    let observation = snapshot.observation.as_ref();
    if !args.json
        && let (Some(failure), Some(observation)) = (&snapshot.scanner.failure, observation)
    {
        // A resident instance whose latest scan failed: its data is the last good scan.
        let age = kosse_core::scan::unix_millis(std::time::SystemTime::now())
            .saturating_sub(observation.scanned_at_ms)
            / 1000;
        report!(
            "kosse: warning: the latest scan failed ({}); showing the last good scan, from {age} s ago",
            printable(failure)
        );
    }
    let mut stdout = io::stdout().lock();
    let written = if args.json {
        write_json(&mut stdout, &snapshot)
    } else {
        // A one-shot scan always yields an observation, and an instance's snapshot without one
        // is never used (see `running_snapshot`).
        observation.map_or(Ok(()), |observation| write_table(&mut stdout, observation))
    };
    match written.and_then(|()| stdout.flush()) {
        // A closed pipe (`kosse list | head`) just means the reader has seen enough.
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            return Err(ListError::Write(error));
        }
        _ => {}
    }
    if !args.json {
        for warning in observation
            .iter()
            .flat_map(|observation| &observation.warnings)
        {
            // Profile warnings can quote a repository file: untrusted text.
            report!("kosse: warning: {}", printable(&warning.to_string()));
        }
    }
    Ok(())
}

/// The snapshot of a verified instance on `port`, if it is compatible with this build and has an
/// observation (an instance whose first scan failed has none; scanning once says more).
fn running_snapshot(port: u16) -> Option<Snapshot> {
    if port == 0 {
        return None;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    match runtime.block_on(instance::find(port)) {
        Found::Kosse(existing) => {
            let url = existing.url.clone();
            let snapshot = (*existing).compatible_snapshot();
            if snapshot.is_none() {
                report!(
                    "kosse: the Kossé at {url} uses another snapshot version; scanning once instead"
                );
            }
            snapshot.filter(|snapshot| snapshot.observation.is_some())
        }
        Found::Nothing | Found::Refused(_) => None,
    }
}

fn scan_once() -> Result<Snapshot, ListError> {
    let store = CentralStore::from_env()?;
    let timer = AttemptTimer::start();
    // CPU needs two observations of a process, so a one-shot scan reports it as warming up.
    let mut scanner = Scanner::new();
    let mut observation = scanner.scan()?;
    Profiles::new(store).apply(&mut observation, scanner.own_user());
    Ok(SnapshotTracker::new(None).succeeded(observation, timer.finish()))
}

fn write_json(out: &mut impl Write, snapshot: &Snapshot) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, snapshot)?;
    out.write_all(b"\n")
}

fn write_table(out: &mut impl Write, observation: &Observation) -> io::Result<()> {
    let projects: HashMap<&str, &Project> = observation
        .projects
        .iter()
        .map(|project| (project.root.as_str(), project))
        .collect();
    writeln!(
        out,
        "{:<8} {:<24} {:<6} {:<32} {:<20} {:<28} ADDRESSES",
        "PID", "PROCESS", "PORT", "PROJECT", "BRANCH", "SERVICE"
    )?;
    for service in &observation.services {
        let name = service
            .process
            .name
            .as_deref()
            .map_or_else(|| "?".to_owned(), printable);
        let project = service
            .project
            .as_deref()
            .and_then(|root| projects.get(root).copied());
        let (project_label, branch) = labels(service, project);
        let configured = configured_label(service, &observation.profiles);
        for endpoint in &service.endpoints {
            writeln!(
                out,
                "{:<8} {:<24} {:<6} {:<32} {:<20} {:<28} {}",
                service.process.identity.pid,
                name,
                endpoint.port,
                project_label,
                branch,
                configured,
                endpoint.bind_addresses.join(", ")
            )?;
        }
    }
    write_unobserved(out, &observation.profiles)
}

/// The configured service of a process and the source of its name (or of its match):
/// `Frontend (web) [central]`, `ambiguous: api, web`, or `-`.
fn configured_label(service: &Service, profiles: &[ProjectMetadata]) -> String {
    match &service.profile {
        None => "-".to_owned(),
        Some(ProfileMatch::Ambiguous { services }) => {
            printable(&format!("ambiguous: {}", services.join(", ")))
        }
        Some(ProfileMatch::Matched { service: id }) => profiles
            .iter()
            .filter(|metadata| service.project.as_deref() == Some(metadata.root.as_str()))
            .flat_map(|metadata| &metadata.services)
            .find(|entry| &entry.id == id)
            .map_or_else(|| printable(id), service_label),
    }
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
    printable(&label)
}

/// Configured services no process matches, and saved profiles of projects not observed. They are
/// neither running nor known to be stopped.
fn write_unobserved(out: &mut impl Write, profiles: &[ProjectMetadata]) -> io::Result<()> {
    let mut rows = Vec::new();
    for metadata in profiles {
        let pending: Vec<&ConfiguredService> = metadata
            .services
            .iter()
            .filter(|entry| !entry.disabled && entry.matched.is_empty())
            .collect();
        let project = if metadata.observed {
            printable(&metadata.root)
        } else {
            printable(&format!("{} (project not observed)", metadata.root))
        };
        if pending.is_empty() && !metadata.observed {
            let sources: Vec<String> = metadata.sources.iter().map(ToString::to_string).collect();
            rows.push((
                project.clone(),
                "-".to_owned(),
                sources.join("+"),
                "-".to_owned(),
            ));
        }
        for entry in pending {
            let source = entry
                .sources
                .matcher
                .or(entry.sources.name)
                .map_or_else(|| "-".to_owned(), |source| source.to_string());
            let matcher = entry.matcher.as_ref().map_or_else(
                || "no match (declared only)".to_owned(),
                |matcher| {
                    let mut parts = Vec::new();
                    if let Some(cwd) = &matcher.cwd {
                        parts.push(format!("cwd {cwd}"));
                    }
                    if let Some(port) = matcher.port {
                        parts.push(format!("port {port}"));
                    }
                    printable(&parts.join(", "))
                },
            );
            rows.push((project.clone(), service_label(entry), source, matcher));
        }
    }
    if rows.is_empty() {
        return Ok(());
    }
    writeln!(out, "\nCONFIGURED, NOT OBSERVED")?;
    writeln!(
        out,
        "{:<48} {:<28} {:<12} MATCH",
        "PROJECT", "SERVICE", "SOURCE"
    )?;
    for (project, service, source, matcher) in rows {
        writeln!(out, "{project:<48} {service:<28} {source:<12} {matcher}")?;
    }
    Ok(())
}

/// `project/package/path` and the branch (or `detached@commit`); `-` when unknown.
fn labels(service: &Service, project: Option<&Project>) -> (String, String) {
    let Some(project) = project else {
        return ("-".to_owned(), "-".to_owned());
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
        Some(GitHead::Branch { name }) => printable(name),
        Some(GitHead::Detached { commit }) => format!("detached@{}", printable(commit)),
        Some(GitHead::Unknown) => "?".to_owned(),
        None => "-".to_owned(),
    };
    (printable(&label), branch)
}
