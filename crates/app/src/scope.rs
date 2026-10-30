//! What the read commands answer from, and which part of it.
//!
//! [`observe`] returns the snapshot of a verified Kossé instance (see [`crate::instance`]) when
//! its schema version is this build's and it has an observation: no second scan. Otherwise it
//! runs its own one-shot scan (revision 1, no interval, no probes), with the central and
//! repository profiles applied as `serve` applies them.
//!
//! A [`Filter`] then keeps only the matching services, in the same `Snapshot` contract.
//! `--here` and `--project` resolve a directory to its project root with the same stat-only walk
//! and `git` as attribution, so a linked worktree is its own project and a directory outside any
//! project is an error, never "everything".

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Instant,
};

use kosse_core::{
    agent::CommandErrorCode,
    attribution::Attributor,
    model::{Project, ScanWarning, Service, Snapshot},
    profiles::{
        CentralStore, Profiles,
        view::{ConfiguredService, ProfileMatch, ProfileSource, ProjectMetadata},
    },
    scan::Scanner,
    snapshot::{AttemptTimer, SnapshotTracker},
};

use crate::{
    cli::Scope,
    failure::Failure,
    instance::{self, Found},
    text::printable,
};

/// The running instance's snapshot on `instance_port`, or a one-shot scan. `quiet` (JSON output)
/// leaves the stale-scan note out of stderr; the snapshot's `scanner.failure` carries it.
pub fn observe(instance_port: u16, quiet: bool) -> Result<Snapshot, Failure> {
    let snapshot = match running_snapshot(instance_port) {
        Some(snapshot) => snapshot,
        None => scan_once()?,
    };
    if !quiet
        && let (Some(failure), Some(observation)) =
            (&snapshot.scanner.failure, &snapshot.observation)
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
    Ok(snapshot)
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
        Found::Kosse(mut existing) => {
            let url = existing.url.clone();
            let snapshot = existing.compatible_snapshot();
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

fn scan_once() -> Result<Snapshot, Failure> {
    let store = CentralStore::from_env()?;
    let timer = AttemptTimer::start();
    // CPU needs two observations of a process, so a one-shot scan reports it as warming up.
    let mut scanner = Scanner::new();
    let mut observation = scanner.scan()?;
    Profiles::new(store).apply(&mut observation, scanner.own_user());
    Ok(SnapshotTracker::new(None).succeeded(observation, timer.finish()))
}

/// The project root `scope` names, if any: `--here` is the current directory's project,
/// `--project` a root already known to the snapshot (observed, or with a saved profile) or the
/// project of that directory.
pub fn root(scope: &Scope, snapshot: &Snapshot) -> Result<Option<String>, Failure> {
    let dir = if scope.here {
        std::env::current_dir().map_err(|error| {
            Failure::new(
                CommandErrorCode::ProjectUnknown,
                format!("--here: the current directory is unavailable: {error}"),
            )
        })?
    } else if let Some(dir) = &scope.project {
        if let Some(known) = known_root(dir, snapshot) {
            return Ok(Some(known));
        }
        dir.clone()
    } else {
        return Ok(None);
    };
    project_of(&dir).map(|project| Some(project.root))
}

fn known_root(dir: &Path, snapshot: &Snapshot) -> Option<String> {
    let text = dir.to_str()?;
    let observation = snapshot.observation.as_ref()?;
    let known = observation
        .projects
        .iter()
        .map(|project| &project.root)
        .chain(observation.profiles.iter().map(|metadata| &metadata.root));
    known.into_iter().find(|root| *root == text).cloned()
}

/// The project `dir` is in, as attribution resolves a process's working directory.
pub fn project_of(dir: &Path) -> Result<Project, Failure> {
    let shown = printable(&dir.display().to_string());
    let canonical: PathBuf = dir.canonicalize().map_err(|error| {
        Failure::new(
            CommandErrorCode::NotInProject,
            format!("{shown} is not a directory Kossé can read: {error}"),
        )
    })?;
    let Some(text) = canonical.to_str() else {
        return Err(Failure::new(
            CommandErrorCode::NotInProject,
            format!("{shown} is not a UTF-8 path, so it is in no project Kossé reports"),
        ));
    };
    let mut attributor = Attributor::for_current_user();
    match attributor.attribute([text], Instant::now()).remove(text) {
        Some(Ok(Some(attribution))) => Ok(attribution.project),
        Some(Ok(None)) | None => Err(Failure::new(
            CommandErrorCode::NotInProject,
            format!(
                "{shown} is in no project (no Git worktree or package manifest at or above it, below the home directory)"
            ),
        )),
        Some(Err(reason)) => Err(Failure::new(
            CommandErrorCode::ProjectUnknown,
            format!("the project of {shown} is unknown: {reason}"),
        )),
    }
}

/// Which services a read command is about. Empty keeps everything.
#[derive(Debug, Default)]
pub struct Filter {
    pub root: Option<String>,
    pub port: Option<u16>,
    /// A configured service, by ID or name.
    pub service: Option<String>,
}

impl Filter {
    fn is_empty(&self) -> bool {
        self.root.is_none() && self.port.is_none() && self.service.is_none()
    }

    /// Keeps the matching services, the projects they run in, the profiles of the projects in
    /// scope (narrowed to the matching configured services) and the warnings that can concern
    /// them. A warning about a process whose project is unknown is kept under a project filter:
    /// it might be in that project, and an unknown is reported, never dropped.
    pub fn apply(&self, snapshot: &mut Snapshot) {
        if self.is_empty() {
            return;
        }
        let Some(observation) = snapshot.observation.as_mut() else {
            return;
        };
        let services = std::mem::take(&mut observation.services);
        let (kept, dropped): (Vec<Service>, Vec<Service>) = services
            .into_iter()
            .partition(|service| self.keeps(service, &observation.profiles));
        let kept_pids: HashSet<u32> = kept
            .iter()
            .map(|service| service.process.identity.pid)
            .collect();
        // Under a project filter with a port, a process on that port whose project is unknown
        // might be in scope; without a port, every unknown-project process would be listed.
        let unknown_in_scope: HashSet<u32> = dropped
            .iter()
            .filter(|service| {
                service.project.is_none()
                    && (self.root.is_none() || self.port.is_some())
                    && self.keeps_port(service)
            })
            .map(|service| service.process.identity.pid)
            .collect();
        let used_roots: HashSet<&str> = kept
            .iter()
            .filter_map(|service| service.project.as_deref())
            .collect();
        let scope_roots: HashSet<String> = match &self.root {
            Some(root) => HashSet::from([root.clone()]),
            None => used_roots.iter().map(|&root| root.to_owned()).collect(),
        };
        observation
            .projects
            .retain(|project| used_roots.contains(project.root.as_str()));
        observation
            .profiles
            .retain(|metadata| scope_roots.contains(&metadata.root));
        for metadata in &mut observation.profiles {
            metadata
                .services
                .retain(|entry| self.keeps_entry(entry, &kept));
        }
        observation.warnings.retain(|warning| match warning {
            ScanWarning::ProcessExited { ports, .. } => {
                self.port.is_none_or(|port| ports.contains(&port))
            }
            ScanWarning::MetadataUnavailable { pid, .. }
            | ScanWarning::AttributionUnavailable { pid, .. } => {
                kept_pids.contains(pid) || unknown_in_scope.contains(pid)
            }
            ScanWarning::ProfileMatchOutsideProject { root, .. }
            | ScanWarning::ProfileRootNotCanonical { root, .. } => {
                self.root.is_none() || scope_roots.contains(root)
            }
            // A repository file sits in its project; the central file concerns every project.
            ScanWarning::ProfileUnusable { source, path, .. } => {
                *source == ProfileSource::Central
                    || self.root.is_none()
                    || scope_roots
                        .iter()
                        .any(|root| Path::new(path).starts_with(root))
            }
            ScanWarning::OtherUsersNotVisible => true,
        });
        observation.services = kept;
    }

    fn keeps(&self, service: &Service, profiles: &[ProjectMetadata]) -> bool {
        self.root
            .as_ref()
            .is_none_or(|root| service.project.as_ref() == Some(root))
            && self.keeps_port(service)
            && self
                .service
                .as_deref()
                .is_none_or(|name| is_configured_as(service, profiles, name))
    }

    fn keeps_port(&self, service: &Service) -> bool {
        self.port.is_none_or(|port| {
            service
                .endpoints
                .iter()
                .any(|endpoint| endpoint.port == port)
        })
    }

    /// A configured service stays when it matches the name filter and, under a port filter,
    /// matches a kept process or declares that port.
    fn keeps_entry(&self, entry: &ConfiguredService, kept: &[Service]) -> bool {
        self.service
            .as_deref()
            .is_none_or(|name| names(entry, name))
            && self.port.is_none_or(|port| {
                entry
                    .matcher
                    .as_ref()
                    .is_some_and(|matcher| matcher.port == Some(port))
                    || kept
                        .iter()
                        .any(|service| entry.matched.contains(&service.process.identity))
            })
    }
}

/// Whether the configured service of `service` (or one of its candidates, when ambiguous) is
/// called `name`.
pub fn is_configured_as(service: &Service, profiles: &[ProjectMetadata], name: &str) -> bool {
    let ids: &[String] = match &service.profile {
        None => return false,
        Some(ProfileMatch::Matched { service: id }) => std::slice::from_ref(id),
        Some(ProfileMatch::Ambiguous { services }) => services,
    };
    project_entries(service, profiles).any(|entry| ids.contains(&entry.id) && names(entry, name))
}

/// The configured entries of `service`'s project.
pub fn project_entries<'a>(
    service: &Service,
    profiles: &'a [ProjectMetadata],
) -> impl Iterator<Item = &'a ConfiguredService> {
    profiles
        .iter()
        .filter(|metadata| service.project.as_deref() == Some(metadata.root.as_str()))
        .flat_map(|metadata| &metadata.services)
}

/// Every port the services listen on, ascending, each once.
pub fn ports<'a>(services: impl IntoIterator<Item = &'a Service>) -> Vec<u16> {
    let mut ports: Vec<u16> = services
        .into_iter()
        .flat_map(|service| service.endpoints.iter().map(|endpoint| endpoint.port))
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Fails with `notFound` (exit 1) when no profile in scope configures a service `name` (by ID or
/// name), so `url` and `wait` answer at once instead of waiting for something that can never
/// match. Lists the configured IDs in scope, or the observed ports when nothing is configured.
pub fn require_configured(
    snapshot: &Snapshot,
    root: Option<&str>,
    name: &str,
) -> Result<(), Failure> {
    let Some(observation) = snapshot.observation.as_ref() else {
        return Ok(());
    };
    let in_scope = |project: Option<&str>| root.is_none_or(|root| project == Some(root));
    let entries: Vec<&ConfiguredService> = observation
        .profiles
        .iter()
        .filter(|metadata| in_scope(Some(&metadata.root)))
        .flat_map(|metadata| &metadata.services)
        .collect();
    if entries.iter().any(|entry| names(entry, name)) {
        return Ok(());
    }
    let scope = root.map_or_else(String::new, |root| format!(" in {root}"));
    let known = if entries.is_empty() {
        let ports: Vec<String> = ports(
            observation
                .services
                .iter()
                .filter(|service| in_scope(service.project.as_deref())),
        )
        .iter()
        .map(ToString::to_string)
        .collect();
        if ports.is_empty() {
            "no services are configured and nothing listens".to_owned()
        } else {
            format!(
                "no services are configured; observed ports: {} (use the port)",
                ports.join(", ")
            )
        }
    } else {
        let mut ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        format!("configured: {}", ids.join(", "))
    };
    Err(Failure::new(
        CommandErrorCode::NotFound,
        format!(
            "no service {name:?} is configured{scope} ({known}; a repository kosse.json is read once a process of its project runs)"
        ),
    ))
}

/// Why `what` matches several services, and how to pick one: under `--here`/`--project`, the
/// ready command for each candidate port (`command`); otherwise each PID and project, and the
/// flags that narrow it.
pub fn ambiguity(
    what: &str,
    services: &[Service],
    root: Option<&str>,
    command: impl Fn(u16) -> String,
) -> String {
    if let Some(root) = root {
        let commands: Vec<String> = ports(services)
            .into_iter()
            .map(|port| format!("`{}`", command(port)))
            .collect();
        return format!(
            "{what} matches {} services in {root}; name a port: {}",
            services.len(),
            commands.join(", ")
        );
    }
    let named: Vec<String> = services
        .iter()
        .map(|service| {
            format!(
                "PID {} in {}",
                service.process.identity.pid,
                service.project.as_deref().unwrap_or("no project")
            )
        })
        .collect();
    format!(
        "{what} matches {} services ({}); narrow it with --here, --project or a port",
        services.len(),
        named.join(", ")
    )
}

fn names(entry: &ConfiguredService, name: &str) -> bool {
    entry.id == name || entry.name.as_deref() == Some(name)
}

#[cfg(test)]
mod tests {
    //! The filter is pure domain logic over a snapshot; built in memory here.
    use kosse_core::model::{
        AttributionFailure, CpuUsage, Endpoint, Exposure, HttpProbe, Observation, ProcessIdentity,
        ProcessMetrics, ProcessRecord, ScanWarning, ScannerStatus, Service, Snapshot,
        TransportProtocol,
    };

    use super::Filter;

    fn service(pid: u32, port: u16, project: Option<&str>) -> Service {
        Service {
            process: ProcessRecord {
                identity: ProcessIdentity {
                    pid,
                    started_at_secs: Some(1),
                },
                name: None,
                executable: None,
                command: None,
                cwd: None,
                parent_pid: None,
                user_id: None,
                age_secs: None,
                metrics: ProcessMetrics {
                    cpu: CpuUsage::WarmingUp,
                    memory_bytes: None,
                },
            },
            endpoints: vec![Endpoint {
                protocol: TransportProtocol::Tcp,
                port,
                bind_addresses: vec!["127.0.0.1".to_owned()],
                exposure: Exposure::Loopback,
                http: HttpProbe::Pending,
                display: Default::default(),
            }],
            project: project.map(str::to_owned),
            package: None,
            profile: None,
        }
    }

    fn snapshot(services: Vec<Service>, warnings: Vec<ScanWarning>) -> Snapshot {
        Snapshot {
            schema_version: kosse_core::model::SNAPSHOT_SCHEMA_VERSION,
            instance: "test".to_owned(),
            instance_pid: 0,
            revision: 1,
            observation: Some(Observation {
                scanned_at_ms: 0,
                scan_duration_ms: 0.0,
                services,
                projects: Vec::new(),
                warnings,
                profiles: Vec::new(),
            }),
            scanner: ScannerStatus {
                interval_ms: None,
                last_attempt_at_ms: 0,
                last_attempt_duration_ms: 0.0,
                failure: None,
                consecutive_failures: 0,
                overruns: 0,
            },
        }
    }

    #[test]
    fn a_project_and_port_filter_keeps_the_warning_of_a_process_whose_project_is_unknown() {
        let unknown = |pid| ScanWarning::AttributionUnavailable {
            pid,
            reason: AttributionFailure::GitTimedOut,
        };
        let mut snapshot = snapshot(
            vec![
                service(1, 3000, Some("/a")),
                service(2, 3001, Some("/b")),
                service(3, 3002, None),
            ],
            vec![unknown(3), ScanWarning::OtherUsersNotVisible],
        );
        let mut by_project = snapshot.clone();
        Filter {
            root: Some("/a".to_owned()),
            ..Filter::default()
        }
        .apply(&mut by_project);
        // Without a port, a project filter does not list every unknown-project process.
        assert_eq!(
            by_project.observation.expect("observation").warnings,
            [ScanWarning::OtherUsersNotVisible]
        );
        let filter = Filter {
            root: Some("/a".to_owned()),
            port: Some(3002),
            ..Filter::default()
        };
        filter.apply(&mut snapshot);
        let observation = snapshot.observation.expect("observation");
        let pids: Vec<u32> = observation
            .services
            .iter()
            .map(|service| service.process.identity.pid)
            .collect();
        assert_eq!(pids, Vec::<u32>::new(), "nothing of /a listens on 3002");
        // Process 3 on that port might be in /a: its unknown stays visible, never dropped.
        assert_eq!(
            observation.warnings,
            [unknown(3), ScanWarning::OtherUsersNotVisible]
        );
    }
}
