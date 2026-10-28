//! Effective metadata: central over repository, field by field with provenance, then matched
//! against the observed processes of the same project.
//!
//! Matching never crosses projects: only services attributed to a root are candidates for that
//! root's entries, so a declared port cannot claim another project's process. `match.cwd`
//! resolves (symbolic links included) inside the project or matches nothing, with a warning. A
//! process matching several entries is ambiguous and gets none. Profiles never change PIDs,
//! owners, binds or eligibility; they can only label, add links, pick a probe path, and make an
//! own loopback endpoint an HTTP probe candidate when it declares `http`/`https`.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    model::{Observation, ScanWarning, Service},
    probe,
    profiles::{
        document::{DeclaredProtocol, LinkProfile, ProjectProfile, ServiceProfile},
        validate,
        view::{
            ConfiguredService, EffectiveLink, ProfileMatch, ProfileSource, ProjectMetadata,
            ServiceSources,
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
    let (links, links_source) = field!(links);
    let (disabled, disabled_source) = field!(disabled);
    ConfiguredService {
        id: id.to_owned(),
        name,
        matcher,
        protocol,
        probe_path,
        links: links.iter().flatten().filter_map(effective_link).collect(),
        disabled: disabled.unwrap_or(false),
        sources: ServiceSources {
            name: name_source,
            matcher: matcher_source,
            protocol: protocol_source,
            probe_path: probe_path_source,
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
            .map(|service| {
                let target = match_dir(&root, service)?;
                if !target.starts_with(&root) {
                    warnings.push(ScanWarning::ProfileMatchOutsideProject {
                        root: project.root.clone(),
                        service: service.id.clone(),
                    });
                    return None;
                }
                Some(target)
            })
            .collect();

        for service in &mut observation.services {
            if service.project.as_deref() != Some(project.root.as_str()) {
                continue;
            }
            let Some(cwd) = canonical_cwd(&mut cwds, service) else {
                continue;
            };
            let matching: Vec<usize> = project
                .services
                .iter()
                .zip(&targets)
                .enumerate()
                .filter(|(_, (entry, target))| {
                    target.as_ref().is_some_and(|target| {
                        cwd.starts_with(target) && port_matches(entry, service)
                    })
                })
                .map(|(index, _)| index)
                .collect();
            for &index in &matching {
                project.services[index]
                    .matched
                    .push(service.process.identity);
            }
            service.profile = match matching[..] {
                [] => None,
                [only] => {
                    let entry = &project.services[only];
                    if matches!(
                        entry.protocol,
                        Some(DeclaredProtocol::Http | DeclaredProtocol::Https)
                    ) {
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

/// The canonical directory an enabled entry matches in; `None` for an entry without `match`, a
/// disabled one, or a directory that does not exist (yet).
fn match_dir(root: &Path, service: &ConfiguredService) -> Option<PathBuf> {
    if service.disabled {
        return None;
    }
    let matcher = service.matcher.as_ref()?;
    let mut dir = root.to_path_buf();
    if let Some(cwd) = &matcher.cwd {
        dir.extend(validate::relative_dir(cwd).ok()?);
    }
    fs::canonicalize(dir).ok()
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

/// A declared HTTP service's endpoints (only the matched port, when the match names one) become
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

/// The probe path configured for this service's endpoint on `port`, if any.
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
    let narrowed = entry.matcher.as_ref().and_then(|matcher| matcher.port);
    if narrowed.is_some_and(|narrowed| narrowed != port) {
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
    use crate::profiles::document::{LinkKind, ServiceMatch};

    fn service(name: Option<&str>, cwd: Option<&str>) -> ServiceProfile {
        ServiceProfile {
            name: name.map(str::to_owned),
            matcher: cwd.map(|cwd| ServiceMatch {
                cwd: Some(cwd.to_owned()),
                port: None,
            }),
            ..Default::default()
        }
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
}
