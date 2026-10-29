//! Project groups and their rows, filtered by scope and search.
use super::{
    State, display_name, observation, port_list,
    row::{self, Layout},
    theme::{QUIET, age, button, label, mono, muted},
};
use crate::{present, text::printable};
use kosse_core::{
    model::{Observation, Project, Service},
    profiles::view::ProjectMetadata,
};
use rune::{El, TextStyle, col, el, row, text};
use rune_kit::Kit;

/// Every search term must appear somewhere in `haystack` (case-insensitive).
fn query_matches(haystack: &str, query: &str) -> bool {
    let haystack = haystack.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|term| haystack.contains(term))
}

/// A project's name and branch, as its heading shows them.
fn project_words(observation: &Observation, root: Option<&str>) -> String {
    let project = observation
        .projects
        .iter()
        .find(|p| Some(p.root.as_str()) == root);
    let metadata = metadata(observation, root);
    format!(
        "{} {} {}",
        root.unwrap_or(""),
        metadata.and_then(|m| m.name.as_deref()).unwrap_or(""),
        project
            .map(|p| format!("{} {}", p.name, present::branch(p).unwrap_or_default()))
            .unwrap_or_default(),
    )
}

/// Search covers the project (name, branch, root), the service's names, package, redacted
/// command and ports.
fn service_matches(service: &Service, s: &State, observation: &Observation) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {}",
        project_words(observation, service.project.as_deref()),
        display_name(service, s),
        service.process.name.as_deref().unwrap_or(""),
        service
            .package
            .as_ref()
            .map_or(String::new(), |p| format!("{} {}", p.name, p.path)),
        service
            .process
            .command
            .as_ref()
            .map_or_else(String::new, |c| c.args.join(" ")),
        port_list(service.endpoints.iter().map(|e| e.port)),
    );
    query_matches(&haystack, &s.search.text)
}

/// Hidden unless All listeners is on: outside any project, or another user's.
fn noise(service: &Service) -> bool {
    service.project.is_none() || present::other_user(service)
}

fn visible(service: &Service, s: &State, observation: &Observation) -> bool {
    (s.all || !noise(service)) && service_matches(service, s, observation)
}

fn metadata<'a>(observation: &'a Observation, root: Option<&str>) -> Option<&'a ProjectMetadata> {
    observation
        .profiles
        .iter()
        .find(|p| Some(p.root.as_str()) == root)
}

pub fn projects(s: &State, k: &Kit, compact: bool) -> El {
    let mut list = col().key("projects").gap(24.0).padding(20.0).w_full();
    if let Some(error) = &s.connection_error {
        list = list.child(label(printable(error), k).wrap());
    }
    if let Some(error) = &s.error {
        list = list.child(
            row()
                .gap(8.0)
                .center_y()
                .child(label(printable(error), k).wrap().grow(1.0))
                .child(button("Dismiss", "dismiss-error", QUIET, k, |s| {
                    s.error = None;
                })),
        );
    }
    let Some(observation) = observation(s) else {
        return list.child(label("Discovering local services…", k));
    };
    if let Some(failure) = s
        .snapshot
        .as_ref()
        .and_then(|snap| snap.scanner.failure.as_ref())
    {
        list = list.child(label(
            format!("Showing the last good observation: {}", printable(failure)),
            k,
        ));
    }
    if s.notes {
        let mut notes = col()
            .key("scan-warnings")
            .gap(6.0)
            .padding(12.0)
            .radius(8.0)
            .border(1.0, k.c.border_weak);
        for warning in &observation.warnings {
            notes = notes.child(muted(printable(&warning.to_string()), k).wrap());
        }
        list = list.child(notes);
    }
    let layout = if compact {
        Layout::Stacked
    } else {
        Layout::Columns
    };
    if layout == Layout::Columns {
        list = list.child(row::column_captions(k));
    }
    let mut roots: Vec<Option<&str>> = observation
        .projects
        .iter()
        .map(|p| Some(p.root.as_str()))
        .collect();
    for profile in &observation.profiles {
        if !roots.contains(&Some(profile.root.as_str())) {
            roots.push(Some(&profile.root));
        }
    }
    roots.push(None);
    let mut count = 0;
    for root in roots {
        if let Some((group, total)) = group(s, observation, root, k, layout) {
            list = list.child(group);
            count += total;
        }
    }
    for (service, seen) in &s.ended {
        if visible(service, s, observation) {
            list = list.child(
                muted(
                    format!(
                        "{} · ended · last observed {} ago · ports {}",
                        display_name(service, s),
                        age(observation.scanned_at_ms.saturating_sub(*seen) / 1000),
                        port_list(service.endpoints.iter().map(|e| e.port)),
                    ),
                    k,
                )
                .wrap(),
            );
        }
    }
    if count == 0 {
        list = list.child(label(
            if s.search.text.is_empty() {
                "No project services are listening. Start a development server, or show all \
                 listeners."
            } else {
                "No matching services. Try another project, process or port."
            },
            k,
        ));
    }
    list
}

fn group(
    s: &State,
    observation: &Observation,
    root: Option<&str>,
    k: &Kit,
    layout: Layout,
) -> Option<(El, usize)> {
    let services: Vec<_> = observation
        .services
        .iter()
        .filter(|service| service.project.as_deref() == root && visible(service, s, observation))
        .collect();
    let metadata = metadata(observation, root);
    let configured_only: Vec<_> = metadata
        .into_iter()
        .flat_map(|m| &m.services)
        .filter(|e| e.matched.is_empty())
        .collect();
    // Configured-only entries and unobserved profiles show when the project itself matches.
    let project_matches =
        metadata.is_some() && query_matches(&project_words(observation, root), &s.search.text);
    let declared = metadata.is_some_and(|m| !m.observed) || !configured_only.is_empty();
    if services.is_empty() && !(project_matches && declared) {
        return None;
    }
    let count = services.len();
    let mut group = col()
        .key(root.unwrap_or("other-listeners"))
        .gap(4.0)
        .child(heading(observation, root, count, k));
    if let Some(metadata) = metadata {
        group = group.child(
            mono(
                printable(&format!(
                    "{} · {}",
                    metadata.root,
                    metadata
                        .sources
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" + ")
                )),
                k,
            )
            .truncate()
            .pb(4.0),
        );
    }
    if project_matches {
        for entry in configured_only {
            group = group.child(muted(
                format!(
                    "{} · {}",
                    printable(entry.name.as_deref().unwrap_or(&entry.id)),
                    if entry.disabled {
                        "Disabled in profile"
                    } else {
                        "Configured, not observed"
                    }
                ),
                k,
            ));
        }
    }
    for service in services {
        group = group.child(row::view(service, s, k, layout));
    }
    Some((group, count))
}

fn heading(observation: &Observation, root: Option<&str>, count: usize, k: &Kit) -> El {
    let project: Option<&Project> = observation
        .projects
        .iter()
        .find(|p| Some(p.root.as_str()) == root);
    let metadata = metadata(observation, root);
    let title = match project {
        Some(project) => present::project_name(project, metadata),
        None => metadata
            .and_then(|m| m.name.as_deref())
            .or(root)
            .map_or_else(|| "Other listeners".to_owned(), printable),
    };
    let mut heading = row().center_y().gap(10.0).h(28.0).child(
        text(title)
            .truncate()
            .font(TextStyle {
                size: 15.0,
                ..k.ty.ui_medium
            })
            .color(k.c.text_base),
    );
    if let Some(branch) = project.and_then(present::branch) {
        heading = heading.child(
            mono(branch, k)
                .truncate()
                .px(6.0)
                .radius(4.0)
                .fill(k.c.layer_02),
        );
    }
    heading = heading
        .child(el().grow(1.0))
        .child(muted(format!("{count} listening"), k));
    if let Some(root) = root {
        let root = root.to_owned();
        heading = heading.child(button(
            "Customize…",
            format!("customize-{root}"),
            QUIET,
            k,
            move |s| {
                if let Some(observation) = s
                    .snapshot
                    .as_ref()
                    .and_then(|snap| snap.observation.as_ref())
                {
                    s.editor = Some(crate::native::customize::Editor::new(
                        root.clone(),
                        observation,
                    ));
                }
            },
        ));
    }
    heading
}
