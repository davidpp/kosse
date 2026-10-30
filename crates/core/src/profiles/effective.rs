//! Effective metadata: central over repository, field by field with provenance, then matched
//! against the observed processes of the same project.
//!
//! Matching never crosses projects: only services attributed to a root are candidates for that
//! root's entries, so a declared port cannot claim another project's process. `match.cwd` is
//! resolved component by component without looking outside the project: a symbolic link that leads
//! out of it matches nothing, with a warning. A
//! process matching several entries is ambiguous and gets none. Profiles never change PIDs,
//! owners, binds or eligibility; they can only label, add links and name a probe path (used only
//! once `GET /` has verified the endpoint as HTTP) and set each endpoint's display ([`display`]).
//! Only the user's own central profile can make an
//! own loopback endpoint an HTTP probe candidate by declaring `http`/`https`; a repository's
//! `protocol` is a hint shown to the user, never a probe trigger.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    model::{DisplayBasis, EndpointDisplay, Observation, ScanWarning, Service},
    probe,
    profiles::{
        document::{DeclaredProtocol, EndpointShow, LinkProfile, ProjectProfile, ServiceProfile},
        validate,
        view::{
            ConfiguredEndpoint, ConfiguredService, EffectiveLink, ProfileMatch, ProfileSource,
            ProjectMetadata, ServiceSources,
        },
    },
};

/// The effective metadata of one root, before matching.
#[must_use]
pub fn merge(
    root: &str,
    central: Option<&ProjectProfile>,
    repository: Option<&ProjectProfile>,
    observed: bool,
) -> ProjectMetadata {
    let mut sources = Vec::new();
    if central.is_some() {
        sources.push(ProfileSource::Central);
    }
    if repository.is_some() {
        sources.push(ProfileSource::Repository);
    }
    let (name, name_source) = pick(
        central.and_then(|profile| profile.name.as_ref()),
        repository.and_then(|profile| profile.name.as_ref()),
    );
    let ids: BTreeSet<&String> = central
        .iter()
        .chain(repository.iter())
        .flat_map(|profile| profile.services.keys())
        .collect();
    let services = ids
        .into_iter()
        .map(|id| {
            merge_service(
                id,
                central.and_then(|profile| profile.services.get(id)),
                repository.and_then(|profile| profile.services.get(id)),
            )
        })
        .collect();
    ProjectMetadata {
        root: root.to_owned(),
        observed,
        sources,
        name,
        name_source,
        central_revision: crate::profiles::central::revision(central),
        services,
    }
}

fn merge_service(
    id: &str,
    central: Option<&ServiceProfile>,
    repository: Option<&ServiceProfile>,
) -> ConfiguredService {
    macro_rules! field {
        ($field:ident) => {
            pick(
                central.and_then(|service| service.$field.as_ref()),
                repository.and_then(|service| service.$field.as_ref()),
            )
        };
    }
    let (name, name_source) = field!(name);
    let (matcher, matcher_source) = field!(matcher);
    let (protocol, protocol_source) = field!(protocol);
    let (probe_path, probe_path_source) = field!(probe_path);
    let (endpoints, endpoints_source) = field!(endpoints);
    let (other_endpoints, other_endpoints_source) = field!(other_endpoints);
    let (links, links_source) = field!(links);
    let (disabled, disabled_source) = field!(disabled);
    // Profiles are validated when loaded, so every key is a port; one that was not would be dropped.
    let mut endpoints: Vec<ConfiguredEndpoint> = endpoints
        .iter()
        .flatten()
        .filter_map(|(key, endpoint)| {
            Some(ConfiguredEndpoint {
                port: validate::port_key(key).ok()?,
                name: endpoint.name.clone(),
                show: endpoint.show,
                probe_path: endpoint.probe_path.clone(),
            })
        })
        .collect();
    endpoints.sort_by_key(|endpoint| endpoint.port);
    ConfiguredService {
        id: id.to_owned(),
        name,
        matcher,
        protocol,
        probe_path,
        endpoints,
        other_endpoints: other_endpoints.map(|other| other.show),
        links: links.iter().flatten().filter_map(effective_link).collect(),
        disabled: disabled.unwrap_or(false),
        sources: ServiceSources {
            name: name_source,
            matcher: matcher_source,
            protocol: protocol_source,
            probe_path: probe_path_source,
            endpoints: endpoints_source,
            other_endpoints: other_endpoints_source,
            links: links_source,
            disabled: disabled_source,
        },
        matched: Vec::new(),
    }
}

/// The central value if set, else the repository's, with its source.
fn pick<T: Clone>(
    central: Option<&T>,
    repository: Option<&T>,
) -> (Option<T>, Option<ProfileSource>) {
    match (central, repository) {
        (Some(value), _) => (Some(value.clone()), Some(ProfileSource::Central)),
        (None, Some(value)) => (Some(value.clone()), Some(ProfileSource::Repository)),
        (None, None) => (None, None),
    }
}

/// Profiles are validated when loaded, so every link parses; one that did not would be dropped.
fn effective_link(link: &LinkProfile) -> Option<EffectiveLink> {
    let parsed = validate::link_url(&link.url).ok()?;
    Some(EffectiveLink {
        label: link.label.clone(),
        kind: link.kind.unwrap_or_else(|| parsed.default_kind()),
        url: parsed.url,
        host: parsed.host,
        reach: parsed.reach,
    })
}

/// Matches every observed root's enabled entries against its services, filling
/// `Service::profile` and `ConfiguredService::matched`, and returns the warnings.
pub fn assign(
    observation: &mut Observation,
    metadata: &mut [ProjectMetadata],
    own_user: Option<u32>,
) -> Vec<ScanWarning> {
    let mut warnings = Vec::new();
    let mut cwds: HashMap<String, Option<PathBuf>> = HashMap::new();
    for project in metadata.iter_mut().filter(|project| project.observed) {
        let root = PathBuf::from(&project.root);
        let targets: Vec<Option<PathBuf>> = project
            .services
            .iter()
            .map(|service| match match_dir(&root, service)? {
                Resolved::Inside(target) => Some(target),
                Resolved::Outside => {
                    warnings.push(ScanWarning::ProfileMatchOutsideProject {
                        root: project.root.clone(),
                        service: service.id.clone(),
                    });
                    None
                }
                Resolved::Missing => None,
            })
            .collect();

        for service in &mut observation.services {
            if service.project.as_deref() != Some(project.root.as_str()) {
                continue;
            }
            // A preview re-matches services an earlier match already displayed.
            for endpoint in &mut service.endpoints {
                endpoint.display = EndpointDisplay::inferred(endpoint.port);
            }
            let Some(cwd) = canonical_cwd(&mut cwds, service) else {
                continue;
            };
            let mut matching: Vec<usize> = project
                .services
                .iter()
                .zip(&targets)
                .enumerate()
                .filter(|(_, (entry, target))| {
                    target.as_ref().is_some_and(|target| {
                        cwd.starts_with(target)
                            && port_matches(entry, service)
                            && process_matches(entry, service)
                    })
                })
                .map(|(index, _)| index)
                .collect();
            prefer_process_entries(&mut matching, &project.services);
            for &index in &matching {
                project.services[index]
                    .matched
                    .push(service.process.identity);
            }
            let entry = match matching[..] {
                [only] => Some(&project.services[only]),
                _ => None,
            };
            for endpoint in &mut service.endpoints {
                endpoint.display = display(entry, endpoint.port);
            }
            service.profile = match matching[..] {
                [] => None,
                [only] => {
                    let entry = &project.services[only];
                    if matches!(
                        entry.protocol,
                        Some(DeclaredProtocol::Http | DeclaredProtocol::Https)
                    ) && entry.sources.protocol == Some(ProfileSource::Central)
                    {
                        declare_http(
                            service,
                            entry.matcher.as_ref().and_then(|m| m.port),
                            own_user,
                        );
                    }
                    Some(ProfileMatch::Matched {
                        service: entry.id.clone(),
                    })
                }
                _ => Some(ProfileMatch::Ambiguous {
                    services: matching
                        .iter()
                        .map(|&index| project.services[index].id.clone())
                        .collect(),
                }),
            };
        }
        for entry in &mut project.services {
            entry.matched.sort_by_key(|identity| identity.pid);
        }
    }
    warnings
}

/// Where an entry's `match.cwd` leads.
#[derive(Debug, PartialEq, Eq)]
enum Resolved {
    /// The real directory, inside the project.
    Inside(PathBuf),
    /// A symbolic link (or `..` inside one) leads out of the project.
    Outside,
    /// Does not exist (yet), is not a directory, or links too deeply.
    Missing,
}

/// Symbolic links followed while resolving one match directory, like the kernel's limit.
const MAX_LINKS: usize = 32;

/// The directory an enabled entry matches in; `None` for an entry without `match` or a disabled
/// one.
fn match_dir(root: &Path, service: &ConfiguredService) -> Option<Resolved> {
    if service.disabled {
        return None;
    }
    let matcher = service.matcher.as_ref()?;
    let parts = match &matcher.cwd {
        Some(cwd) => validate::relative_dir(cwd).ok()?,
        None => Vec::new(),
    };
    Some(resolve_inside(
        root,
        parts.into_iter().map(OsString::from).collect(),
    ))
}

/// Resolves `parts` under the canonical `root` one component at a time with `symlink_metadata`
/// and `read_link`, refusing as soon as a link or `..` would leave the root, so nothing outside the
/// project is ever looked at. An absolute link target counts as inside only when it starts with
/// the canonical root itself.
fn resolve_inside(root: &Path, parts: Vec<OsString>) -> Resolved {
    let mut pending: VecDeque<OsString> = parts.into();
    let mut current = root.to_path_buf();
    let mut links = 0;
    while let Some(part) = pending.pop_front() {
        match part.to_str() {
            Some("" | ".") => continue,
            // `current` is a real directory below the root (no links in it), so `..` is its parent.
            Some("..") => {
                if current == root {
                    return Resolved::Outside;
                }
                current.pop();
                continue;
            }
            _ => {}
        }
        let next = current.join(&part);
        let Ok(metadata) = fs::symlink_metadata(&next) else {
            return Resolved::Missing;
        };
        if metadata.file_type().is_symlink() {
            links += 1;
            let Ok(target) = fs::read_link(&next) else {
                return Resolved::Missing;
            };
            if links > MAX_LINKS {
                return Resolved::Missing;
            }
            let rest: Vec<OsString> = if target.is_absolute() {
                let Ok(inside) = target.strip_prefix(root) else {
                    return Resolved::Outside;
                };
                current = root.to_path_buf();
                inside.iter().map(OsString::from).collect()
            } else {
                target.iter().map(OsString::from).collect()
            };
            for component in rest.into_iter().rev() {
                pending.push_front(component);
            }
        } else if metadata.is_dir() {
            current = next;
        } else {
            return Resolved::Missing;
        }
    }
    Resolved::Inside(current)
}

fn port_matches(entry: &ConfiguredService, service: &Service) -> bool {
    match entry.matcher.as_ref().and_then(|matcher| matcher.port) {
        Some(port) => service
            .endpoints
            .iter()
            .any(|endpoint| endpoint.port == port),
        None => true,
    }
}

/// An entry naming the process is the more specific one: it takes a child (workerd) from a
/// directory-wide entry rather than making it ambiguous. Specificity never overrides precedence:
/// when a repository entry naming the process would take it from a central entry, all stay (the
/// process is ambiguous), so a `kosse.json` cannot silently displace the user's own profile.
/// `process` with `port` is no more specific than `process` alone.
fn prefer_process_entries(matching: &mut Vec<usize>, entries: &[ConfiguredService]) {
    let (specific, general): (Vec<usize>, Vec<usize>) = matching
        .iter()
        .partition(|&&index| names_process(&entries[index]));
    if specific.is_empty() || general.is_empty() {
        return;
    }
    let source = |index: &usize| entries[*index].sources.matcher;
    let displaces_central = general
        .iter()
        .any(|index| source(index) == Some(ProfileSource::Central))
        && specific
            .iter()
            .any(|index| source(index) == Some(ProfileSource::Repository));
    if !displaces_central {
        *matching = specific;
    }
}

fn names_process(entry: &ConfiguredService) -> bool {
    entry
        .matcher
        .as_ref()
        .is_some_and(|matcher| matcher.process.is_some())
}

fn process_matches(entry: &ConfiguredService, service: &Service) -> bool {
    match entry
        .matcher
        .as_ref()
        .and_then(|matcher| matcher.process.as_deref())
    {
        Some(name) => service.process.name.as_deref() == Some(name),
        None => true,
    }
}

/// How the endpoint on `port` of a process is shown: by the entry that matched it alone (`None`
/// for an unmatched or ambiguous process): its `endpoints` entry, else its `otherEndpoints`, else
/// the inference for the inspector port, else the summary.
#[must_use]
pub fn display(entry: Option<&ConfiguredService>, port: u16) -> EndpointDisplay {
    if let Some(entry) = entry {
        if let Some(listed) = entry.endpoints.iter().find(|listed| listed.port == port) {
            return EndpointDisplay {
                name: listed.name.clone(),
                show: listed.show.unwrap_or_default(),
                basis: DisplayBasis::Endpoint,
                source: entry.sources.endpoints,
            };
        }
        if let Some(show) = entry.other_endpoints {
            return EndpointDisplay {
                name: None,
                show,
                basis: DisplayBasis::OtherEndpoints,
                source: entry.sources.other_endpoints,
            };
        }
    }
    EndpointDisplay::inferred(port)
}

fn canonical_cwd<'a>(
    cache: &'a mut HashMap<String, Option<PathBuf>>,
    service: &Service,
) -> Option<&'a PathBuf> {
    let cwd = service.process.cwd.as_ref()?;
    cache
        .entry(cwd.clone())
        .or_insert_with(|| fs::canonicalize(cwd).ok())
        .as_ref()
}

/// A centrally declared HTTP service's endpoints (only the matched port, when the match names one) become
/// probe candidates despite a non-HTTP-looking port or process name. Loopback and owner rules
/// still apply.
fn declare_http(service: &mut Service, port: Option<u16>, own_user: Option<u32>) {
    let own = service.process.user_id.is_some() && service.process.user_id == own_user;
    for endpoint in &mut service.endpoints {
        if port.is_none_or(|port| port == endpoint.port) {
            endpoint.http = probe::declared_http(endpoint.http.clone(), own);
        }
    }
}

/// The probe path configured for this service's endpoint on `port`, if any: the endpoint's own
/// `probePath` on either tier, else the service's, which is the health path of its summary
/// endpoints only (and only of the matched port, when the match names one).
#[must_use]
pub fn probe_path<'a>(
    observation: &'a Observation,
    service: &Service,
    port: u16,
) -> Option<&'a str> {
    let Some(ProfileMatch::Matched { service: id }) = &service.profile else {
        return None;
    };
    let root = service.project.as_deref()?;
    let entry = observation
        .profiles
        .iter()
        .find(|project| project.root == root)?
        .services
        .iter()
        .find(|entry| &entry.id == id)?;
    entry_probe_path(entry, port)
}

fn entry_probe_path(entry: &ConfiguredService, port: u16) -> Option<&str> {
    if let Some(path) = entry
        .endpoints
        .iter()
        .find(|listed| listed.port == port)
        .and_then(|listed| listed.probe_path.as_deref())
    {
        return Some(path);
    }
    let narrowed = entry.matcher.as_ref().and_then(|matcher| matcher.port);
    if narrowed.is_some_and(|narrowed| narrowed != port)
        || display(Some(entry), port).show != EndpointShow::Summary
    {
        return None;
    }
    entry.probe_path.as_deref()
}

/// Every root that has a profile from either source.
pub(crate) fn roots<'a>(
    central: Option<&'a BTreeMap<String, ProjectProfile>>,
    repositories: impl Iterator<Item = &'a String>,
) -> BTreeSet<&'a String> {
    central
        .into_iter()
        .flat_map(BTreeMap::keys)
        .chain(repositories)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{
            CpuUsage, Endpoint, Exposure, HttpProbe, ProcessIdentity, ProcessMetrics,
            ProcessRecord, TransportProtocol,
        },
        profiles::document::{LinkKind, ServiceMatch},
    };

    fn service(name: Option<&str>, cwd: Option<&str>) -> ServiceProfile {
        ServiceProfile {
            name: name.map(str::to_owned),
            matcher: cwd.map(|cwd| ServiceMatch {
                cwd: Some(cwd.to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn match_directories_resolve_without_leaving_the_project() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().canonicalize().expect("canonical");
        let root = base.join("project");
        let outside = base.join("outside");
        for path in [
            root.join("apps/web"),
            root.join("apps/api"),
            outside.join("x"),
        ] {
            fs::create_dir_all(path).expect("dir");
        }
        symlink("../apps/api", root.join("apps/relative")).expect("link");
        symlink(root.join("apps/web"), root.join("apps/absolute")).expect("link");
        symlink("../../outside", root.join("apps/up")).expect("link");
        symlink(&outside, root.join("apps/away")).expect("link");
        symlink("loop", root.join("apps/loop")).expect("link");
        let resolve = |cwd: &str| {
            let parts = validate::relative_dir(cwd).expect("valid");
            resolve_inside(&root, parts.into_iter().map(OsString::from).collect())
        };
        assert_eq!(resolve("."), Resolved::Inside(root.clone()));
        assert_eq!(
            resolve("apps/relative"),
            Resolved::Inside(root.join("apps/api"))
        );
        assert_eq!(
            resolve("apps/absolute"),
            Resolved::Inside(root.join("apps/web"))
        );
        assert_eq!(resolve("apps/up"), Resolved::Outside);
        assert_eq!(resolve("apps/up/x"), Resolved::Outside);
        assert_eq!(resolve("apps/away"), Resolved::Outside);
        assert_eq!(resolve("apps/loop"), Resolved::Missing);
        assert_eq!(resolve("apps/none"), Resolved::Missing);
    }

    #[test]
    fn central_overrides_repository_field_by_field_with_provenance() {
        let mut repository = ProjectProfile::new();
        repository.name = Some("Repo name".into());
        let mut web = service(Some("Repo web"), Some("apps/web"));
        web.probe_path = Some("/health".into());
        web.links = Some(vec![LinkProfile {
            label: "Docs".into(),
            url: "HTTP://LOCALHOST:3001/docs".into(),
            kind: None,
            unknown: BTreeMap::new(),
        }]);
        repository.services.insert("web".into(), web);
        repository.services.insert(
            "worker".into(),
            service(Some("Worker"), Some("apps/worker")),
        );

        let mut central = ProjectProfile::new();
        let mut web = service(Some("Central web"), None);
        web.links = Some(Vec::new());
        central.services.insert("web".into(), web);
        central.services.insert(
            "worker".into(),
            ServiceProfile {
                disabled: Some(true),
                ..Default::default()
            },
        );

        let merged = merge("/r", Some(&central), Some(&repository), true);
        assert_eq!(
            merged.sources,
            [ProfileSource::Central, ProfileSource::Repository]
        );
        assert_eq!(merged.name.as_deref(), Some("Repo name"));
        assert_eq!(merged.name_source, Some(ProfileSource::Repository));
        let web = &merged.services[0];
        assert_eq!(web.name.as_deref(), Some("Central web"));
        assert_eq!(web.sources.name, Some(ProfileSource::Central));
        assert_eq!(
            web.matcher.as_ref().and_then(|m| m.cwd.as_deref()),
            Some("apps/web")
        );
        assert_eq!(web.sources.matcher, Some(ProfileSource::Repository));
        assert_eq!(web.probe_path.as_deref(), Some("/health"));
        assert!(web.links.is_empty(), "arrays replace, never concatenate");
        assert_eq!(web.sources.links, Some(ProfileSource::Central));
        let worker = &merged.services[1];
        assert!(worker.disabled);
        assert_eq!(worker.sources.disabled, Some(ProfileSource::Central));
        assert_eq!(worker.name.as_deref(), Some("Worker"));

        let repo_only = merge("/r", None, Some(&repository), true);
        let link = &repo_only.services[0].links[0];
        assert_eq!(link.url, "http://localhost:3001/docs");
        assert_eq!(link.kind, LinkKind::Local);
    }

    /// One merged entry from a central profile written as JSON.
    fn entry(json: serde_json::Value) -> ConfiguredService {
        let service: ServiceProfile = serde_json::from_value(json).expect("service profile");
        let mut central = ProjectProfile::new();
        central.services.insert("web".into(), service);
        merge("/r", Some(&central), None, true).services.remove(0)
    }

    fn shown(display: &EndpointDisplay) -> (Option<&str>, EndpointShow, DisplayBasis) {
        (display.name.as_deref(), display.show, display.basis)
    }

    #[test]
    fn display_follows_the_endpoint_entry_then_other_endpoints_then_the_inspector_port() {
        use DisplayBasis::{Default, Endpoint, InspectorPort, OtherEndpoints};
        use EndpointShow::{Details, Summary};
        let web = entry(serde_json::json!({
            "endpoints": {
                "3000": {"name": "Site"},
                "4000": {"show": "details"},
                "9229": {"name": "Debugger", "show": "summary"}
            },
            "otherEndpoints": {"show": "details"}
        }));
        let at = |port| display(Some(&web), port);
        assert_eq!(
            shown(&at(3000)),
            (Some("Site"), Summary, Endpoint),
            "listed means summary"
        );
        assert_eq!(shown(&at(4000)), (None, Details, Endpoint));
        assert_eq!(
            shown(&at(9229)),
            (Some("Debugger"), Summary, Endpoint),
            "a listing beats the inference"
        );
        assert_eq!(
            shown(&at(51234)),
            (None, Details, OtherEndpoints),
            "a changing port needs no entry"
        );
        assert_eq!(at(3000).source, Some(ProfileSource::Central));
        assert_eq!(at(51234).source, Some(ProfileSource::Central));

        let summary_rest = entry(serde_json::json!({"otherEndpoints": {"show": "summary"}}));
        assert_eq!(
            shown(&display(Some(&summary_rest), 9229)),
            (None, Summary, OtherEndpoints),
            "the profile's default beats the inference"
        );

        let plain = entry(serde_json::json!({"name": "Web"}));
        for described in [Some(&plain), None] {
            assert_eq!(
                shown(&display(described, 9229)),
                (Some("Inspector"), Details, InspectorPort)
            );
            assert_eq!(display(described, 9229).source, None);
            assert_eq!(shown(&display(described, 3000)), (None, Summary, Default));
        }
    }

    #[test]
    fn a_service_probe_path_checks_only_its_summary_endpoints() {
        let web = entry(serde_json::json!({
            "probePath": "/health",
            "endpoints": {
                "3000": {"name": "Site"},
                "4000": {"show": "details", "probePath": "/ready"},
                "5000": {"show": "details"},
                "6000": {"probePath": "/own"}
            },
            "otherEndpoints": {"show": "details"}
        }));
        assert_eq!(entry_probe_path(&web, 3000), Some("/health"));
        assert_eq!(
            entry_probe_path(&web, 4000),
            Some("/ready"),
            "an endpoint's own path, on any tier"
        );
        assert_eq!(
            entry_probe_path(&web, 5000),
            None,
            "a details endpoint is not the service's health"
        );
        assert_eq!(entry_probe_path(&web, 6000), Some("/own"));
        assert_eq!(
            entry_probe_path(&web, 7000),
            None,
            "otherEndpoints puts it in details"
        );

        let narrowed = entry(serde_json::json!({
            "match": {"cwd": ".", "port": 3000},
            "probePath": "/health",
            "endpoints": {"3001": {"name": "Admin"}, "3002": {"probePath": "/admin"}}
        }));
        assert_eq!(entry_probe_path(&narrowed, 3000), Some("/health"));
        assert_eq!(
            entry_probe_path(&narrowed, 3001),
            None,
            "match.port still narrows the service path"
        );
        assert_eq!(entry_probe_path(&narrowed, 3002), Some("/admin"));
    }

    fn listener(pid: u32, name: &str, cwd: &Path, ports: &[u16]) -> Service {
        Service {
            process: ProcessRecord {
                identity: ProcessIdentity {
                    pid,
                    started_at_secs: Some(1),
                },
                name: Some(name.to_owned()),
                executable: None,
                command: None,
                cwd: Some(cwd.to_string_lossy().into_owned()),
                parent_pid: None,
                user_id: Some(501),
                age_secs: None,
                metrics: ProcessMetrics {
                    cpu: CpuUsage::WarmingUp,
                    memory_bytes: None,
                },
            },
            endpoints: ports
                .iter()
                .map(|&port| Endpoint {
                    protocol: TransportProtocol::Tcp,
                    port,
                    bind_addresses: vec!["127.0.0.1".to_owned()],
                    exposure: Exposure::Loopback,
                    http: HttpProbe::Pending,
                    display: EndpointDisplay::inferred(port),
                })
                .collect(),
            project: Some(cwd.to_string_lossy().into_owned()),
            package: None,
            profile: None,
        }
    }

    /// A Vite-like `node` (site, inspector, internal port) and its `workerd` child in `root`.
    fn stack(root: &Path) -> Observation {
        Observation {
            scanned_at_ms: 0,
            scan_duration_ms: 0.0,
            services: vec![
                listener(10, "node", root, &[3000, 9229, 40123]),
                listener(11, "workerd", root, &[51001, 51002]),
            ],
            projects: Vec::new(),
            warnings: Vec::new(),
            profiles: Vec::new(),
        }
    }

    fn assign_profile(observation: &mut Observation, root: &Path, entries: &serde_json::Value) {
        let mut profile = ProjectProfile::new();
        profile.services = serde_json::from_value(entries.clone()).expect("entries");
        let mut metadata = [merge(&root.to_string_lossy(), Some(&profile), None, true)];
        assert_eq!(assign(observation, &mut metadata, Some(501)), []);
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "compared with `Service::profile` as is"
    )]
    fn matched(id: &str) -> Option<ProfileMatch> {
        Some(ProfileMatch::Matched { service: id.into() })
    }

    #[test]
    fn a_repository_process_entry_never_displaces_a_central_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        let mut observation = stack(&root);
        let mut central = ProjectProfile::new();
        central.services = serde_json::from_value(serde_json::json!({
            "web": {"match": {"cwd": "."}, "protocol": "http"}
        }))
        .expect("central");
        let mut repository = ProjectProfile::new();
        repository.services = serde_json::from_value(serde_json::json!({
            "node": {"match": {"process": "node"}}
        }))
        .expect("repository");
        let root_text = root.to_string_lossy().into_owned();
        let mut metadata = [merge(&root_text, Some(&central), Some(&repository), true)];
        assign(&mut observation, &mut metadata, Some(501));
        assert_eq!(
            observation.services[0].profile,
            Some(ProfileMatch::Ambiguous {
                services: vec!["node".into(), "web".into()]
            }),
            "a visible ambiguity, as before S21, not a silent takeover"
        );
        assert_eq!(
            observation.services[1].profile,
            matched("web"),
            "workerd is not `node`"
        );

        // Both from the central profile: the process entry is more specific.
        let mut both = central.clone();
        both.services.extend(repository.services.clone());
        let mut metadata = [merge(&root_text, Some(&both), None, true)];
        assign(&mut observation, &mut metadata, Some(501));
        assert_eq!(observation.services[0].profile, matched("node"));

        // A central process entry may take a process from a repository directory entry.
        let mut metadata = [merge(&root_text, Some(&repository), Some(&central), true)];
        assign(&mut observation, &mut metadata, Some(501));
        assert_eq!(observation.services[0].profile, matched("node"));
    }

    #[test]
    fn launch_targets_are_the_summary_endpoints() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        let mut observation = stack(&root);
        assign_profile(
            &mut observation,
            &root,
            &serde_json::json!({
                "site": {
                    "match": {"cwd": ".", "port": 3000},
                    "endpoints": {"3000": {"name": "Site"}},
                    "otherEndpoints": {"show": "details"}
                },
                "worker": {
                    "match": {"cwd": ".", "process": "workerd"},
                    "otherEndpoints": {"show": "details"}
                },
                "anything": {"match": {"process": "other"}}
            }),
        );
        let [vite, workerd] = &observation.services[..] else {
            panic!("two services");
        };
        assert_eq!(vite.profile, matched("site"));
        assert_eq!(workerd.profile, matched("worker"));
        let launch = |service: &Service| -> Vec<u16> {
            service
                .launch_endpoints()
                .map(|endpoint| endpoint.port)
                .collect()
        };
        assert_eq!(
            launch(vite),
            [3000],
            "the inspector and the internal port are details"
        );
        assert_eq!(
            launch(workerd),
            [0_u16; 0],
            "a service of details endpoints has no launch target"
        );
    }

    #[test]
    fn a_process_entry_is_more_specific_than_a_directory_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        let mut observation = stack(&root);
        assign_profile(
            &mut observation,
            &root,
            &serde_json::json!({"worker": {"match": {"process": "workerd"}, "otherEndpoints": {"show": "details"}}}),
        );
        let loose = serde_json::json!({
            "all": {"match": {"cwd": "."}},
            "worker": {"match": {"process": "workerd"}},
            "other": {"match": {"process": "workerd", "port": 1}}
        });
        assign_profile(&mut observation, &root, &loose);
        assert_eq!(observation.services[0].profile, matched("all"));
        assert_eq!(observation.services[1].profile, matched("worker"));
        assert_eq!(
            shown(&observation.services[1].endpoints[0].display),
            (None, EndpointShow::Summary, DisplayBasis::Default),
            "a re-match starts again from the inference"
        );

        let mut crowded = loose;
        crowded["worker-too"] = serde_json::json!({"match": {"process": "workerd"}});
        crowded["site"] = serde_json::json!({"match": {"cwd": ".", "port": 3000}, "endpoints": {"3000": {"name": "Site"}}});
        assign_profile(&mut observation, &root, &crowded);
        assert_eq!(
            observation.services[1].profile,
            Some(ProfileMatch::Ambiguous {
                services: vec!["worker".into(), "worker-too".into()]
            }),
            "two equally specific entries stay ambiguous"
        );
        assert!(
            matches!(
                observation.services[0].profile,
                Some(ProfileMatch::Ambiguous { .. })
            ),
            "two directory entries stay ambiguous"
        );
        assert_eq!(
            shown(&observation.services[0].endpoints[0].display),
            (None, EndpointShow::Summary, DisplayBasis::Default),
            "an ambiguous process gets only the inference"
        );
        assert_eq!(
            observation.services[0].endpoints[1].display.basis,
            DisplayBasis::InspectorPort
        );
    }
}
