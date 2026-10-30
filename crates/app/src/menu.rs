//! The tray menu as data: a pure function of the shared snapshot, so it is cheap to rebuild on
//! every publication, compared before touching `AppKit`, and tested without a menu bar.
//!
//! Layout (blueprint/product.md, Tray surface): a status line when the tray needs attention,
//! one section per project (name and branch) listing its services as `name :port[, :port]`, each
//! with Open (only for summary endpoints, the launch targets, that answered HTTP), Inspect and
//! Stop…; then the listeners
//! outside any project; then Open Dashboard and Quit Kossé. Inspect and Stop… open dashboard
//! routes; Stop… opens the dashboard's own confirmation, so the menu never signals anything.

use std::collections::HashMap;

use kosse_core::{
    model::{Endpoint, GitHead, HttpProbe, Observation, ProbeResult, Project, Service, Snapshot},
    profiles::view::{ProfileMatch, ProjectMetadata},
};

use crate::{
    launcher,
    route::{Route, Target},
    text::printable,
};

/// The scanner counts as stalled after this many intervals without a finished attempt.
const STALL_INTERVALS: u64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Why the tray shows its attention icon: discovery failed or the snapshot is stale. A failed
    /// HTTP probe never sets it.
    pub attention: Option<String>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// Not clickable: a status line or a project heading.
    Text(String),
    Heading(String),
    Separator,
    Action {
        label: String,
        action: Action,
    },
    Submenu {
        label: String,
        items: Vec<Item>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// A service's page in the default browser (`http(s)://localhost:<port>/` from the prober).
    Open(String),
    /// A dashboard route in the dashboard window.
    Show {
        route: Route,
        title: String,
    },
    Quit,
}

impl Action {
    /// A stable menu item ID; unique within one menu.
    pub fn id(&self) -> String {
        match self {
            Self::Open(url) => format!("open:{url}"),
            Self::Show { route, .. } => match route {
                Route::Dashboard => "dashboard".to_owned(),
                route => route.path(),
            },
            Self::Quit => "quit".to_owned(),
        }
    }
}

impl Model {
    /// The menu for the latest publication (`None` before the first scan finishes). `now_ms` only
    /// decides whether the scanner has stalled.
    pub fn of(latest: Option<&Snapshot>, now_ms: u64) -> Self {
        let mut items = Vec::new();
        let attention = latest.and_then(|snapshot| attention(snapshot, now_ms));
        if let Some(reason) = &attention {
            items.push(Item::Text(reason.clone()));
            items.push(Item::Separator);
        }
        match latest
            .and_then(|snapshot| Some((snapshot.observation.as_ref()?, snapshot.instance_pid)))
        {
            None if latest.is_none() => items.push(Item::Text("Scanning…".to_owned())),
            None => {}
            Some((observation, kosse)) => services(observation, kosse, &mut items),
        }
        if !matches!(items.last(), None | Some(Item::Separator)) {
            items.push(Item::Separator);
        }
        items.push(Item::Action {
            label: "Open Dashboard".to_owned(),
            action: Action::Show {
                route: Route::Dashboard,
                title: launcher::TITLE.to_owned(),
            },
        });
        items.push(Item::Action {
            label: "Quit Kossé".to_owned(),
            action: Action::Quit,
        });
        Self { attention, items }
    }

    /// The window title the menu would use for `route` (`Kossé — <service>`), if it lists it.
    pub fn title_for(&self, route: Route) -> Option<String> {
        self.actions().into_iter().find_map(|action| match action {
            Action::Show {
                route: shown,
                title,
            } if *shown == route => Some(title.clone()),
            _ => None,
        })
    }

    /// Every clickable action, depth first.
    pub fn actions(&self) -> Vec<&Action> {
        fn walk<'a>(items: &'a [Item], out: &mut Vec<&'a Action>) {
            for item in items {
                match item {
                    Item::Action { action, .. } => out.push(action),
                    Item::Submenu { items, .. } => walk(items, out),
                    Item::Text(_) | Item::Heading(_) | Item::Separator => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, &mut out);
        out
    }
}

fn attention(snapshot: &Snapshot, now_ms: u64) -> Option<String> {
    let scanner = &snapshot.scanner;
    if let Some(failure) = &scanner.failure {
        let failure = printable(failure);
        return Some(if snapshot.observation.is_some() {
            format!("Showing the last good scan: {failure}")
        } else {
            format!("Discovery failed: {failure}")
        });
    }
    let interval = scanner.interval_ms?;
    let stalled_after = interval.saturating_mul(STALL_INTERVALS);
    (now_ms.saturating_sub(scanner.last_attempt_at_ms) > stalled_after).then(|| {
        format!(
            "Out of date: no scan has finished for over {} s",
            stalled_after / 1000
        )
    })
}

/// Project sections in the observation's order, then the listeners outside any project.
/// `kosse` is this instance's PID: its own row has no Stop… (Kossé never signals itself).
fn services(observation: &Observation, kosse: u32, items: &mut Vec<Item>) {
    let names: HashMap<&str, &ProjectMetadata> = observation
        .profiles
        .iter()
        .map(|metadata| (metadata.root.as_str(), metadata))
        .collect();
    for project in &observation.projects {
        let mut members: Vec<Item> = observation
            .services
            .iter()
            .filter(|service| service.project.as_deref() == Some(project.root.as_str()))
            .map(|service| service_item(service, observation, kosse))
            .collect();
        if members.is_empty() {
            continue;
        }
        members.sort_by(|a, b| label(a).cmp(label(b)));
        items.push(Item::Heading(heading(
            project,
            names.get(project.root.as_str()).copied(),
        )));
        items.append(&mut members);
        items.push(Item::Separator);
    }
    let mut others: Vec<Item> = observation
        .services
        .iter()
        .filter(|service| service.project.is_none())
        .map(|service| service_item(service, observation, kosse))
        .collect();
    if !others.is_empty() {
        others.sort_by(|a, b| label(a).cmp(label(b)));
        items.push(Item::Submenu {
            label: "Other Listeners".to_owned(),
            items: others,
        });
    }
    if observation.services.is_empty() {
        items.push(Item::Text("No listening services".to_owned()));
    }
}

fn label(item: &Item) -> &str {
    match item {
        Item::Text(label)
        | Item::Heading(label)
        | Item::Action { label, .. }
        | Item::Submenu { label, .. } => label,
        Item::Separator => "",
    }
}

/// `name — branch`, `name — detached a1b2c3d`, or the name alone outside Git.
fn heading(project: &Project, metadata: Option<&ProjectMetadata>) -> String {
    let name = metadata
        .and_then(|metadata| metadata.name.as_deref())
        .unwrap_or(&project.name);
    let head = project.git.as_ref().map(|git| match &git.head {
        GitHead::Branch { name } => name.clone(),
        GitHead::Detached { commit } => {
            format!("detached {}", commit.chars().take(7).collect::<String>())
        }
        GitHead::Unknown => "unknown branch".to_owned(),
    });
    printable(&match head {
        Some(head) => format!("{name} — {head}"),
        None => name.to_owned(),
    })
}

/// `name :port[, :port]` with Open per summary endpoint that answered HTTP, Inspect and Stop….
fn service_item(service: &Service, observation: &Observation, kosse: u32) -> Item {
    let name = service_name(service, observation);
    let ports: Vec<String> = service
        .endpoints
        .iter()
        .map(|endpoint| format!(":{}", endpoint.port))
        .collect();
    let mut items = Vec::new();
    let reachable: Vec<(&Endpoint, &str)> = service
        .launch_endpoints()
        .filter_map(|endpoint| open_url(&endpoint.http).map(|url| (endpoint, url)))
        .collect();
    for (endpoint, url) in &reachable {
        let name = endpoint.display.name.as_deref();
        // Two endpoints of one name (an unlikely profile) still get distinct items.
        let shared = reachable
            .iter()
            .filter(|(other, _)| other.display.name.as_deref() == name)
            .count()
            > 1;
        let label = match name {
            _ if reachable.len() == 1 => "Open".to_owned(),
            Some(name) if shared => printable(&format!("Open {name} :{}", endpoint.port)),
            Some(name) => printable(&format!("Open {name}")),
            None => format!("Open :{}", endpoint.port),
        };
        items.push(Item::Action {
            label,
            action: Action::Open((*url).to_owned()),
        });
    }
    let title = launcher::title_for(&name);
    match Target::of(service.process.identity) {
        Some(target) => {
            items.push(Item::Action {
                label: "Inspect".to_owned(),
                action: Action::Show {
                    route: Route::Inspect(target),
                    title: title.clone(),
                },
            });
            if service.process.identity.pid != kosse {
                items.push(Item::Action {
                    label: "Stop…".to_owned(),
                    action: Action::Show {
                        route: Route::Stop(target),
                        title,
                    },
                });
            }
        }
        // Without a start time the process cannot be named safely, so neither route exists.
        None => items.push(Item::Text(
            "Start time unknown: see the dashboard".to_owned(),
        )),
    }
    Item::Submenu {
        label: format!("{name} {}", ports.join(", ")),
        items,
    }
}

/// The configured service name, else the package name, else the process name, else the PID.
fn service_name(service: &Service, observation: &Observation) -> String {
    let configured = match &service.profile {
        Some(ProfileMatch::Matched { service: id }) => observation
            .profiles
            .iter()
            .filter(|metadata| service.project.as_deref() == Some(metadata.root.as_str()))
            .flat_map(|metadata| &metadata.services)
            .find(|entry| &entry.id == id)
            .map(|entry| entry.name.clone().unwrap_or_else(|| entry.id.clone())),
        _ => None,
    };
    let name = configured
        .or_else(|| service.package.as_ref().map(|package| package.name.clone()))
        .or_else(|| service.process.name.clone())
        .unwrap_or_else(|| format!("PID {}", service.process.identity.pid));
    printable(&name)
}

/// The URL to open when the endpoint answered HTTP (any status counts as reachable).
fn open_url(http: &HttpProbe) -> Option<&str> {
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

#[cfg(test)]
mod tests {
    //! Snapshots are built in memory here: the menu is a pure function of one.

    use kosse_core::model::{
        CpuUsage, Endpoint, Exposure, GitWorktree, PackageLabel, ProbeSkip, ProcessIdentity,
        ProcessMetrics, ProcessRecord, ScannerStatus, TransportProtocol,
    };

    use super::*;

    const NOW: u64 = 1_761_000_000_000;

    fn endpoint(port: u16, http: HttpProbe) -> Endpoint {
        Endpoint {
            protocol: TransportProtocol::Tcp,
            port,
            bind_addresses: vec!["127.0.0.1".to_owned()],
            exposure: Exposure::Loopback,
            http,
            display: kosse_core::model::EndpointDisplay::default(),
        }
    }

    fn answered(port: u16) -> HttpProbe {
        HttpProbe::Probed {
            probed_at_ms: NOW,
            requested: false,
            result: ProbeResult::Response {
                url: format!("http://localhost:{port}/"),
                status: 404,
                title: None,
                location: None,
                tls: false,
                body_truncated: false,
            },
        }
    }

    fn service(pid: u32, name: &str, project: Option<&str>, endpoints: Vec<Endpoint>) -> Service {
        Service {
            process: ProcessRecord {
                identity: ProcessIdentity {
                    pid,
                    started_at_secs: Some(1_760_000_000 + u64::from(pid)),
                },
                name: Some(name.to_owned()),
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
            endpoints,
            project: project.map(str::to_owned),
            package: None,
            profile: None,
        }
    }

    fn project(root: &str, name: &str, head: Option<GitHead>) -> Project {
        Project {
            root: root.to_owned(),
            name: name.to_owned(),
            git: head.map(|head| GitWorktree {
                common_dir: format!("{root}/.git"),
                repository_name: name.to_owned(),
                linked: false,
                head,
            }),
        }
    }

    fn snapshot(services: Vec<Service>, projects: Vec<Project>) -> Snapshot {
        Snapshot {
            schema_version: kosse_core::model::SNAPSHOT_SCHEMA_VERSION,
            instance: "test".to_owned(),
            instance_pid: 0,
            revision: 3,
            observation: Some(Observation {
                scanned_at_ms: NOW,
                scan_duration_ms: 1.0,
                services,
                projects,
                warnings: Vec::new(),
                profiles: Vec::new(),
            }),
            scanner: ScannerStatus {
                interval_ms: Some(3000),
                last_attempt_at_ms: NOW,
                last_attempt_duration_ms: 1.0,
                failure: None,
                consecutive_failures: 0,
                overruns: 0,
            },
        }
    }

    fn submenu<'a>(model: &'a Model, label: &str) -> &'a [Item] {
        model
            .items
            .iter()
            .find_map(|item| match item {
                Item::Submenu { label: l, items } if l == label => Some(items.as_slice()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no submenu {label:?} in {model:#?}"))
    }

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(label).collect()
    }

    #[test]
    fn services_are_grouped_by_project_with_branch_and_ports() {
        let snapshot = snapshot(
            vec![
                service(
                    20,
                    "node",
                    Some("/src/shop"),
                    vec![endpoint(5173, answered(5173))],
                ),
                service(
                    10,
                    "api",
                    Some("/src/shop"),
                    vec![
                        endpoint(3000, HttpProbe::Pending),
                        endpoint(3001, HttpProbe::Pending),
                    ],
                ),
                service(
                    30,
                    "postgres",
                    None,
                    vec![endpoint(
                        5432,
                        HttpProbe::Skipped {
                            reason: ProbeSkip::KnownNonHttpPort,
                        },
                    )],
                ),
            ],
            vec![project(
                "/src/shop",
                "shop",
                Some(GitHead::Branch {
                    name: "main".to_owned(),
                }),
            )],
        );
        let model = Model::of(Some(&snapshot), NOW);

        assert_eq!(model.attention, None);
        assert_eq!(
            labels(&model.items),
            [
                "shop — main",
                "api :3000, :3001",
                "node :5173",
                "",
                "Other Listeners",
                "",
                "Open Dashboard",
                "Quit Kossé"
            ]
        );
        assert!(matches!(model.items[0], Item::Heading(_)));
        // Open only where an HTTP response was received (a 404 counts).
        assert_eq!(
            labels(submenu(&model, "node :5173")),
            ["Open", "Inspect", "Stop…"]
        );
        assert_eq!(
            labels(submenu(&model, "api :3000, :3001")),
            ["Inspect", "Stop…"]
        );
        let others = submenu(&model, "Other Listeners");
        assert_eq!(labels(others), ["postgres :5432"]);
    }

    #[test]
    fn kosse_itself_has_no_stop() {
        let mut snapshot = snapshot(
            vec![service(
                77,
                "kosse",
                Some("/src/shop"),
                vec![endpoint(47831, HttpProbe::Pending)],
            )],
            vec![project("/src/shop", "shop", None)],
        );
        snapshot.instance_pid = 77;
        let model = Model::of(Some(&snapshot), NOW);
        assert_eq!(labels(submenu(&model, "kosse :47831")), ["Inspect"]);
    }

    #[test]
    fn actions_route_to_the_dashboard_and_never_signal() {
        let snapshot = snapshot(
            vec![service(
                20,
                "node",
                Some("/src/shop"),
                vec![
                    endpoint(5173, answered(5173)),
                    endpoint(5174, answered(5174)),
                ],
            )],
            vec![project("/src/shop", "shop", None)],
        );
        let model = Model::of(Some(&snapshot), NOW);
        let ids: Vec<String> = model.actions().iter().map(|action| action.id()).collect();
        assert_eq!(
            ids,
            [
                "open:http://localhost:5173/",
                "open:http://localhost:5174/",
                "/?inspect=20.1760000020",
                "/?stop=20.1760000020",
                "dashboard",
                "quit"
            ]
        );
        assert_eq!(
            labels(submenu(&model, "node :5173, :5174")),
            ["Open :5173", "Open :5174", "Inspect", "Stop…"]
        );
        let stop = model
            .actions()
            .into_iter()
            .find(|action| action.id().starts_with("/?stop="))
            .cloned();
        assert_eq!(
            stop,
            Some(Action::Show {
                route: Route::Stop(
                    Target::of(ProcessIdentity {
                        pid: 20,
                        started_at_secs: Some(1_760_000_020)
                    })
                    .expect("start")
                ),
                title: launcher::title_for("node"),
            })
        );
    }

    #[test]
    fn open_is_offered_only_for_summary_endpoints() {
        use kosse_core::{
            model::{DisplayBasis, EndpointDisplay},
            profiles::document::EndpointShow,
        };
        let shown = |port: u16, name: Option<&str>, show: EndpointShow| {
            let mut endpoint = endpoint(port, answered(port));
            endpoint.display = EndpointDisplay {
                name: name.map(str::to_owned),
                show,
                basis: DisplayBasis::Endpoint,
                source: None,
            };
            endpoint
        };
        let snapshot = snapshot(
            vec![
                service(
                    20,
                    "node",
                    Some("/src/shop"),
                    vec![
                        shown(3000, Some("Site"), EndpointShow::Summary),
                        shown(3001, None, EndpointShow::Summary),
                        shown(9229, Some("Inspector"), EndpointShow::Details),
                    ],
                ),
                service(
                    21,
                    "workerd",
                    Some("/src/shop"),
                    vec![
                        shown(51001, None, EndpointShow::Details),
                        shown(51002, None, EndpointShow::Details),
                    ],
                ),
            ],
            vec![project("/src/shop", "shop", None)],
        );
        let model = Model::of(Some(&snapshot), NOW);
        assert_eq!(
            labels(submenu(&model, "node :3000, :3001, :9229")),
            ["Open Site", "Open :3001", "Inspect", "Stop…"],
            "a details endpoint that answered HTTP is no launch target"
        );

        assert_eq!(
            labels(submenu(&model, "workerd :51001, :51002")),
            ["Inspect", "Stop…"]
        );
    }

    #[test]
    fn open_items_of_one_name_name_their_ports() {
        use kosse_core::{model::DisplayBasis, profiles::document::EndpointShow};
        let named = |port: u16| {
            let mut endpoint = endpoint(port, answered(port));
            endpoint.display = kosse_core::model::EndpointDisplay {
                name: Some("Site".into()),
                show: EndpointShow::Summary,
                basis: DisplayBasis::Endpoint,
                source: None,
            };
            endpoint
        };
        let snapshot = snapshot(
            vec![service(
                20,
                "node",
                Some("/src/shop"),
                vec![named(3000), named(3001)],
            )],
            vec![project("/src/shop", "shop", None)],
        );
        let model = Model::of(Some(&snapshot), NOW);
        assert_eq!(
            labels(submenu(&model, "node :3000, :3001")),
            ["Open Site :3000", "Open Site :3001", "Inspect", "Stop…"]
        );
    }

    #[test]
    fn a_process_without_start_time_offers_no_routes() {
        let mut unknown = service(20, "node", None, vec![endpoint(8080, answered(8080))]);
        unknown.process.identity.started_at_secs = None;
        let model = Model::of(Some(&snapshot(vec![unknown], Vec::new())), NOW);
        let others = submenu(&model, "Other Listeners");
        assert_eq!(
            labels(match &others[0] {
                Item::Submenu { items, .. } => items,
                other => panic!("{other:?}"),
            }),
            ["Open", "Start time unknown: see the dashboard"]
        );
    }

    #[test]
    fn untrusted_names_are_escaped_and_package_names_preferred() {
        let mut named = service(20, "node\u{202E}x", Some("/src/a"), Vec::new());
        named.endpoints.push(endpoint(3000, HttpProbe::Pending));
        let bidi = service(
            21,
            "evil\u{202E}gnp.exe",
            Some("/src/a"),
            vec![endpoint(3001, HttpProbe::Pending)],
        );
        named.package = Some(PackageLabel {
            path: "apps/web".to_owned(),
            name: "@shop/web".to_owned(),
            manifests: Vec::new(),
        });
        let model = Model::of(
            Some(&snapshot(
                vec![named, bidi],
                vec![project("/src/a", "a\nb", None)],
            )),
            NOW,
        );
        assert_eq!(
            labels(&model.items)[..3],
            ["a\\u{a}b", "@shop/web :3000", "evil\\u{202e}gnp.exe :3001"]
        );
    }

    #[test]
    fn attention_means_failed_discovery_or_a_stale_snapshot_not_a_failed_probe() {
        let mut probe_failed = snapshot(
            vec![service(
                20,
                "node",
                None,
                vec![endpoint(
                    8080,
                    HttpProbe::Probed {
                        probed_at_ms: NOW,
                        requested: false,
                        result: ProbeResult::TimedOut,
                    },
                )],
            )],
            Vec::new(),
        );
        assert_eq!(Model::of(Some(&probe_failed), NOW).attention, None);

        probe_failed.scanner.failure = Some("enumeration failed".to_owned());
        probe_failed.scanner.consecutive_failures = 1;
        let stale = Model::of(Some(&probe_failed), NOW);
        assert_eq!(
            stale.attention.as_deref(),
            Some("Showing the last good scan: enumeration failed")
        );
        assert_eq!(
            stale.items[0],
            Item::Text("Showing the last good scan: enumeration failed".to_owned())
        );

        probe_failed.observation = None;
        let failed = Model::of(Some(&probe_failed), NOW);
        assert_eq!(
            failed.attention.as_deref(),
            Some("Discovery failed: enumeration failed")
        );
        assert_eq!(
            labels(&failed.items),
            [
                "Discovery failed: enumeration failed",
                "",
                "Open Dashboard",
                "Quit Kossé"
            ]
        );

        let mut stalled = snapshot(Vec::new(), Vec::new());
        assert_eq!(Model::of(Some(&stalled), NOW + 9_000).attention, None);
        stalled.scanner.last_attempt_at_ms = NOW - 9_001;
        assert_eq!(
            Model::of(Some(&stalled), NOW).attention.as_deref(),
            Some("Out of date: no scan has finished for over 9 s")
        );
    }

    #[test]
    fn before_the_first_scan_the_menu_says_so() {
        let model = Model::of(None, NOW);
        assert_eq!(model.attention, None);
        assert_eq!(
            labels(&model.items),
            ["Scanning…", "", "Open Dashboard", "Quit Kossé"]
        );
    }
}
