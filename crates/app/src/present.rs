//! How a service is named and what it offers, shared by the tray menu and the dashboard window
//! so both views show the same names, branches and actions. Text returned here is printable:
//! process-, probe- and repository-derived strings pass [`printable`].

use kosse_core::{
    model::{GitHead, HttpProbe, Observation, ProbeResult, ProbeSkip, Project, Service},
    profiles::view::{ConfiguredService, ProfileMatch, ProjectMetadata},
};

use crate::text::printable;

/// The profile entry a service matched, if any.
pub fn configured<'a>(
    service: &Service,
    observation: &'a Observation,
) -> Option<&'a ConfiguredService> {
    let Some(ProfileMatch::Matched { service: id }) = &service.profile else {
        return None;
    };
    observation
        .profiles
        .iter()
        .filter(|metadata| service.project.as_deref() == Some(metadata.root.as_str()))
        .flat_map(|metadata| &metadata.services)
        .find(|entry| &entry.id == id)
}

/// The configured service name (or its ID), else the package name, else the process name, else
/// the PID.
pub fn service_name(service: &Service, observation: &Observation) -> String {
    let name = configured(service, observation)
        .map(|entry| entry.name.clone().unwrap_or_else(|| entry.id.clone()))
        .or_else(|| service.package.as_ref().map(|package| package.name.clone()))
        .or_else(|| service.process.name.clone())
        .unwrap_or_else(|| format!("PID {}", service.process.identity.pid));
    printable(&name)
}

/// The profile's project name, else the detected one.
pub fn project_name(project: &Project, metadata: Option<&ProjectMetadata>) -> String {
    printable(
        metadata
            .and_then(|metadata| metadata.name.as_deref())
            .unwrap_or(&project.name),
    )
}

/// `main`, `detached a1b2c3d` or `unknown branch`; `None` outside Git.
pub fn branch(project: &Project) -> Option<String> {
    let git = project.git.as_ref()?;
    Some(printable(&match &git.head {
        GitHead::Branch { name } => name.clone(),
        GitHead::Detached { commit } => {
            format!("detached {}", commit.chars().take(7).collect::<String>())
        }
        GitHead::Unknown => "unknown branch".to_owned(),
    }))
}

/// Another user's process: Kossé neither probes nor stops it.
pub fn other_user(service: &Service) -> bool {
    service.endpoints.iter().any(|endpoint| {
        matches!(
            endpoint.http,
            HttpProbe::Skipped {
                reason: ProbeSkip::OtherUser
            }
        )
    })
}

/// Whether Stop… is offered: a known start time (the identity a confirmation names), never PID 0
/// or 1, never Kossé itself (`kosse` is the snapshot's `instancePid`), never another user's
/// process. The owner revalidates all of it before any signal.
pub fn stop_offered(service: &Service, kosse: u32) -> bool {
    let identity = service.process.identity;
    identity.started_at_secs.is_some()
        && identity.pid > 1
        && identity.pid != kosse
        && !other_user(service)
}

/// The URL to open when the endpoint answered HTTP (any status counts as reachable). Only the
/// prober's own `http(s)://localhost:<port>/` form is ever opened.
pub fn open_url(http: &HttpProbe) -> Option<&str> {
    match http {
        HttpProbe::Probed {
            result: ProbeResult::Response { url, .. },
            ..
        } if url.starts_with("http://localhost:") || url.starts_with("https://localhost:") => {
            Some(url)
        }
        _ => None,
    }
}
