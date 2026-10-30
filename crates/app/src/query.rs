//! `kosse url` and `kosse port`: one answer each, for scripts and agents, from the same snapshot
//! as `kosse list` (see [`crate::scope::observe`]).
//!
//! - `url <service|port>` prints only `prober::open_url`'s `http(s)://localhost:<port>/` of the one
//!   endpoint that answered HTTP: for a service, among its summary endpoints (its launch targets);
//!   a port answers for any endpoint. A name matching several services (two worktrees, say),
//!   several answering ports, a service whose endpoints are all details, or no answer at all is a
//!   negative answer (exit 1) with the reason, and the endpoints' probe states, on stderr.
//! - `port <n>` prints `free`, qualified when other users' listeners are hidden, or the service
//!   table's rows for that port.

use std::{io, process::ExitCode};

use kosse_core::{
    agent::{AGENT_SCHEMA_VERSION, CommandErrorCode, PortAnswer, PortState, UrlAnswer},
    model::{Observation, ScanWarning, Service, Snapshot},
    profiles::view::ProfileMatch,
};

use crate::{
    cli::{PortArgs, UrlArgs},
    failure::{self, Failure},
    list,
    scope::{self, Filter},
};

pub fn url(args: &UrlArgs) -> ExitCode {
    failure::exit(answer_url(args), args.json)
}

pub fn port(args: &PortArgs) -> ExitCode {
    failure::exit(answer_port(args), args.json)
}

fn answer_url(args: &UrlArgs) -> Result<(), Failure> {
    let mut snapshot = scope::observe(args.instance.instance_port, args.json)?;
    // A number is a port; configured service IDs and names are matched as given otherwise.
    let port = args.target.parse::<u16>().ok();
    let filter = Filter {
        root: scope::root(&args.scope, &snapshot)?,
        port,
        service: port.is_none().then(|| args.target.clone()),
    };
    if port.is_none() {
        scope::require_configured(&snapshot, filter.root.as_deref(), &args.target)?;
    }
    filter.apply(&mut snapshot);
    let probes_run = list::probes_run(&snapshot);
    let observation = observation(&snapshot)?;
    let what = port.map_or_else(
        || format!("service {:?}", args.target),
        |port| format!("port {port}"),
    );
    let service = match observation.services.as_slice() {
        [] => {
            let scope = filter
                .root
                .as_ref()
                .map_or_else(String::new, |root| format!(" in {root}"));
            return Err(Failure::new(
                CommandErrorCode::NotFound,
                format!("no observed {what}{scope}"),
            ));
        }
        [service] => service,
        several => {
            return Err(Failure::new(
                CommandErrorCode::Ambiguous,
                scope::ambiguity(&what, several, filter.root.as_deref(), |port| {
                    format!("kosse url {port}")
                }),
            ));
        }
    };
    if port.is_none()
        && let Some(ProfileMatch::Ambiguous { services }) = &service.profile
    {
        return Err(Failure::new(
            CommandErrorCode::Ambiguous,
            format!(
                "process {} matches several configured services ({}); fix the profile or name a port",
                service.process.identity.pid,
                services.join(", ")
            ),
        ));
    }
    let (port, url) = reachable(service, port, &what, probes_run)?;
    let mut stdout = io::stdout().lock();
    if args.json {
        let answer = UrlAnswer {
            schema_version: AGENT_SCHEMA_VERSION,
            url,
            port,
            identity: service.process.identity,
            project: service.project.clone(),
            service: match &service.profile {
                Some(ProfileMatch::Matched { service }) => Some(service.clone()),
                _ => None,
            },
        };
        return failure::written(list::write_json(&mut stdout, &answer));
    }
    failure::written(io::Write::write_all(
        &mut stdout,
        format!("{url}\n").as_bytes(),
    ))
}

/// The one endpoint of `service` (on `port`, when given) that answered HTTP, with its URL.
fn reachable(
    service: &Service,
    port: Option<u16>,
    what: &str,
    probes_run: bool,
) -> Result<(u16, String), Failure> {
    let endpoints: Vec<_> = match port {
        Some(port) => service
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.port == port)
            .collect(),
        None => service.launch_endpoints().collect(),
    };
    if endpoints.is_empty() && port.is_none() {
        return Err(Failure::new(
            CommandErrorCode::NoHttp,
            details_only(what, service, "kosse url"),
        ));
    }
    let reachable: Vec<_> = endpoints
        .iter()
        .filter_map(|endpoint| list::reachable_url(endpoint).map(|url| (endpoint.port, url)))
        .collect();
    match reachable.as_slice() {
        [(port, url)] => Ok((*port, url.clone())),
        [] => {
            let states: Vec<String> = endpoints
                .iter()
                .map(|endpoint| {
                    format!(
                        ":{} {}",
                        endpoint.port,
                        list::http_state(endpoint, probes_run)
                    )
                })
                .collect();
            let hint = if probes_run {
                ""
            } else {
                " (no Kossé instance runs, and a one-shot scan never probes: start `kosse serve` or the menu-bar app)"
            };
            Err(Failure::new(
                CommandErrorCode::NoHttp,
                format!("{what} has not answered HTTP: {}{hint}", states.join("; ")),
            ))
        }
        several => {
            let commands: Vec<String> = several
                .iter()
                .map(|(port, _)| format!("`kosse url {port}`"))
                .collect();
            Err(Failure::new(
                CommandErrorCode::Ambiguous,
                format!(
                    "{what} answers HTTP on several ports; name one: {}",
                    commands.join(", ")
                ),
            ))
        }
    }
}

/// Why a service without summary endpoints has no launch target, naming its ports so a caller can
/// ask for one, with `command` and the first port as the suggestion.
pub fn details_only(what: &str, service: &Service, command: &str) -> String {
    let ports: Vec<String> = service
        .endpoints
        .iter()
        .map(|endpoint| format!(":{}", endpoint.port))
        .collect();
    let first = service
        .endpoints
        .first()
        .map_or_else(String::new, |endpoint| {
            format!("; name a port: `{command} {}`", endpoint.port)
        });
    format!(
        "{what} has no launch target: every endpoint is in details ({}){first}",
        ports.join(", ")
    )
}

fn answer_port(args: &PortArgs) -> Result<(), Failure> {
    let mut snapshot = scope::observe(args.instance.instance_port, args.json)?;
    Filter {
        port: Some(args.port),
        ..Filter::default()
    }
    .apply(&mut snapshot);
    let observation = observation(&snapshot)?;
    let other_users_visible = !observation
        .warnings
        .contains(&ScanWarning::OtherUsersNotVisible);
    let free = observation.services.is_empty();
    let mut stdout = io::stdout().lock();
    if args.json {
        let answer = PortAnswer {
            schema_version: AGENT_SCHEMA_VERSION,
            port: args.port,
            state: if free {
                PortState::Free
            } else {
                PortState::Taken
            },
            other_users_visible,
            scanned_at_ms: observation.scanned_at_ms,
            services: observation.services.clone(),
            projects: observation.projects.clone(),
        };
        return failure::written(list::write_json(&mut stdout, &answer));
    }
    if free {
        let line = if other_users_visible {
            "free\n"
        } else {
            "free as far as this user can see (other users' listeners, system services included, are hidden without administrator privileges)\n"
        };
        return failure::written(io::Write::write_all(&mut stdout, line.as_bytes()));
    }
    failure::written(
        list::services_table(observation, Some(args.port), list::probes_run(&snapshot))
            .write(&mut stdout),
    )
}

fn observation(snapshot: &Snapshot) -> Result<&Observation, Failure> {
    // `scope::observe` never returns a snapshot without one; say so rather than panic.
    snapshot.observation.as_ref().ok_or_else(|| {
        Failure::new(
            CommandErrorCode::ScanFailed,
            "no scan has succeeded yet".to_owned(),
        )
    })
}
