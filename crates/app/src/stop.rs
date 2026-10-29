//! `kosse stop` and `kosse force`: a same-origin client of a verified instance's `POST /api/stop`
//! and `/api/force` (see [`crate::instance::post`]), so the CLI has no signal path of its own and
//! the instance's policy (`action::revalidate`, the exact port set, force offers, one action at a
//! time, one per second) decides everything.
//!
//! The arguments are the confirmation: `<pid>.<start>` and `--ports` with every port the process
//! listens on. Anything less (a service name, a port, a PID, an identity without `--ports`) and
//! any refusal print the exact commands that would confirm what runs now, from a fresh snapshot
//! of the same instance, and signal nothing. Without a verified instance nothing is signalled
//! (blueprint/agent-surface.md, D1): force offers live in the instance.
//!
//! Exit codes: 0 `exited`; 1 `stillRunning`, `respawned`, `endpointTaken` (signalled, but the
//! ports are not free); 3 refused, nothing signalled; 4 no instance, `observationFailed`, or the
//! instance failed or did not answer.

use std::{
    fmt::Write as _,
    io::{self, Write},
    process::ExitCode,
    time::{Duration, SystemTime},
};

use kosse_core::{
    action::{ActionRequest, ActionResult, Ineligible, PortOwner, StaleReason},
    agent::{AGENT_SCHEMA_VERSION, ActionAnswer, CommandErrorCode, Confirmation},
    contract::ApiError,
    inspect::{self, Presence},
    model::{Observation, ProcessIdentity, Service, Snapshot},
    scan::unix_millis,
};

use crate::{
    actions::FORCE_OFFER_TTL,
    cli::{ActionArgs, DEFAULT_PORT},
    failure::{self, Failure},
    instance::{self, Found, Instance, InstanceError, PostError},
    list,
    markdown::{Table, UNKNOWN},
    scope,
    text::printable,
};

/// Bounds a whole action: the instance observes a `SIGTERM`ed process for 5 s and its ports for
/// a respawn for 1 s more (see `actions.rs`), plus scans of the sockets around them.
const ACTION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Stop,
    Force,
}

impl Verb {
    fn name(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Force => "force",
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::Stop => "/api/stop",
            Self::Force => "/api/force",
        }
    }
}

pub fn run(args: &ActionArgs, verb: Verb) -> ExitCode {
    match act(args, verb) {
        Ok(code) => ExitCode::from(code),
        Err(failure) => failure::failed(&failure, args.json),
    }
}

/// What the request names.
enum Target {
    Identity(ProcessIdentity),
    /// A number: a port, or a PID.
    Number(u32),
    /// A configured service ID or name.
    Service(String),
}

fn target(text: &str) -> Target {
    if let Some((pid, start)) = text.split_once('.')
        && let (Ok(pid), Ok(start)) = (pid.parse(), start.parse())
    {
        return Target::Identity(ProcessIdentity {
            pid,
            started_at_secs: Some(start),
        });
    }
    match text.parse() {
        Ok(number) => Target::Number(number),
        Err(_) => Target::Service(text.to_owned()),
    }
}

/// The instance to act through and what it observes.
struct Client {
    runtime: tokio::runtime::Runtime,
    instance: Instance,
    snapshot: Snapshot,
    instance_port: u16,
}

fn act(args: &ActionArgs, verb: Verb) -> Result<u8, Failure> {
    let client = Client::connect(args.instance_port)?;
    let identity = match target(&args.target) {
        Target::Identity(identity) => identity,
        Target::Number(number) => {
            let root = scope::root(&args.scope, &client.snapshot)?;
            return Err(
                client.unconfirmed(verb, &format!("port or PID {number}"), |service| {
                    in_root(service, root.as_deref())
                        && (service.process.identity.pid == number
                            || u16::try_from(number).is_ok_and(|port| listens_on(service, &[port])))
                }),
            );
        }
        Target::Service(name) => {
            let root = scope::root(&args.scope, &client.snapshot)?;
            let profiles = client
                .snapshot
                .observation
                .as_ref()
                .map(|observation| observation.profiles.clone())
                .unwrap_or_default();
            return Err(
                client.unconfirmed(verb, &format!("service {name:?}"), |service| {
                    in_root(service, root.as_deref())
                        && scope::is_configured_as(service, &profiles, &name)
                }),
            );
        }
    };
    let Some(ports) = args.ports.clone() else {
        let observation = client.fresh_observation();
        let confirm = client.confirmations(&observation, verb, |service| {
            service.process.identity.pid == identity.pid
        });
        if confirm.is_empty() {
            return Err(client.not_observed(identity));
        }
        return Err(Failure {
            confirm,
            ..Failure::new(
                CommandErrorCode::ConfirmationRequired,
                format!(
                    "{} needs --ports with every port {} listens on; nothing was signalled. To confirm what runs now:",
                    verb.name(),
                    shown(identity)
                ),
            )
        });
    };
    let request = ActionRequest {
        instance: client.snapshot.instance.clone(),
        identity,
        ports,
    };
    let result = client.send(verb, &request)?;
    answer(args, verb, &client, &request, result)
}

impl Client {
    fn connect(instance_port: u16) -> Result<Self, Failure> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                Failure::new(
                    CommandErrorCode::ActionFailed,
                    format!("could not start the async runtime: {error}"),
                )
            })?;
        let found = if instance_port == 0 {
            Found::Nothing
        } else {
            runtime.block_on(instance::find(instance_port))
        };
        let start = start_command(instance_port);
        let mut instance = match found {
            Found::Kosse(instance) => *instance,
            Found::Nothing => {
                return Err(Failure::new(
                    CommandErrorCode::NoInstance,
                    format!(
                        "no Kossé instance runs on 127.0.0.1:{instance_port}; stop and force act only through one, which keeps the force offer: start `{start}` (or the menu-bar app) and retry"
                    ),
                ));
            }
            Found::Refused(error) => {
                // `InstanceError`'s own text advises `--port`, for starting a server.
                let reason = match error {
                    InstanceError::NotKosse(port) => {
                        format!("127.0.0.1:{port} is in use by a process that is not Kossé")
                    }
                    InstanceError::OtherUser(port) => format!(
                        "127.0.0.1:{port} is held by another user's process (or one whose owner cannot be read), and Kossé never uses another user's instance"
                    ),
                    other => other.to_string(),
                };
                return Err(Failure::new(
                    CommandErrorCode::NoInstance,
                    format!(
                        "{reason}; nothing was signalled: stop and force act only through a verified Kossé instance, so point --instance-port at yours (`kosse serve --no-open --port <free port>` starts one)"
                    ),
                ));
            }
        };
        let Some(snapshot) = instance
            .compatible_snapshot()
            .filter(|snapshot| snapshot.observation.is_some())
        else {
            return Err(Failure::new(
                CommandErrorCode::NoInstance,
                format!(
                    "the Kossé at {} (version {}) has no snapshot this kosse can read; nothing was signalled",
                    instance.url, instance.version
                ),
            ));
        };
        Ok(Self {
            runtime,
            instance,
            snapshot,
            instance_port,
        })
    }

    /// Posts the request and reads the instance's answer. Only answers that signalled nothing
    /// are failures here; the rest are for [`answer`].
    fn send(&self, verb: Verb, request: &ActionRequest) -> Result<ActionResult, Failure> {
        let body = serde_json::to_vec(request)
            .map_err(|error| Failure::new(CommandErrorCode::ActionFailed, error.to_string()))?;
        let posted = self.runtime.block_on(instance::post(
            self.instance.addr,
            verb.path(),
            body,
            ACTION_TIMEOUT,
        ));
        let (status, body) = match posted {
            Ok(answer) => answer,
            Err(error @ PostError::Session(_)) => {
                return Err(Failure::new(
                    CommandErrorCode::ActionFailed,
                    format!(
                        "the Kossé at {} gave no {error}; nothing was signalled",
                        self.instance.url
                    ),
                ));
            }
            Err(PostError::NoAnswer) => {
                return Err(Failure::new(
                    CommandErrorCode::ActionFailed,
                    format!(
                        "no answer from the Kossé at {} within {} s; a signal may have been sent: check with `kosse list`",
                        self.instance.url,
                        ACTION_TIMEOUT.as_secs()
                    ),
                ));
            }
        };
        if let Ok(result) = serde_json::from_slice::<ActionResult>(&body) {
            return Ok(result);
        }
        // The guard's and the handler's own refusals (rate limit, payload) signal nothing.
        let (code, message) = match serde_json::from_slice::<ApiError>(&body) {
            Ok(error) if status.as_u16() == 429 => (
                CommandErrorCode::RateLimited,
                format!(
                    "the instance is busy: {}; nothing was signalled, retry in a second",
                    error.message
                ),
            ),
            Ok(error) => (
                CommandErrorCode::ActionFailed,
                format!(
                    "the instance refused the request ({status}: {}); nothing was signalled",
                    error.message
                ),
            ),
            Err(_) => (
                CommandErrorCode::ActionFailed,
                format!(
                    "the instance answered {status} without an action result; a signal may have been sent: check with `kosse list`"
                ),
            ),
        };
        Err(Failure::new(code, message))
    }

    /// The latest observation for confirmation commands: a fresh scan of the instance when its
    /// refresh limit allows one, else the snapshot read when connecting.
    fn fresh_observation(&self) -> Observation {
        let posted = self.runtime.block_on(instance::post(
            self.instance.addr,
            "/api/refresh",
            b"{}".to_vec(),
            ACTION_TIMEOUT,
        ));
        let fresh = match posted {
            Ok((status, body)) if status.is_success() => {
                serde_json::from_slice::<Snapshot>(&body).ok()
            }
            _ => None,
        };
        fresh
            .filter(|snapshot| snapshot.instance == self.snapshot.instance)
            .and_then(|snapshot| snapshot.observation)
            .or_else(|| self.snapshot.observation.clone())
            // `connect` keeps only a snapshot with an observation.
            .unwrap_or_else(|| Observation {
                scanned_at_ms: 0,
                scan_duration_ms: 0.0,
                services: Vec::new(),
                projects: Vec::new(),
                warnings: Vec::new(),
                profiles: Vec::new(),
            })
    }

    /// The confirmation commands for the processes of `observation` that `matches` keeps;
    /// processes the instance would never signal (Kossé itself, no start time, another user's)
    /// are left out.
    fn confirmations(
        &self,
        observation: &Observation,
        verb: Verb,
        matches: impl Fn(&Service) -> bool,
    ) -> Vec<Confirmation> {
        let own_user = match inspect::vitals(std::process::id()) {
            Presence::Running(vitals) => vitals.effective_user_id,
            Presence::Exited | Presence::Unknown => None,
        };
        observation
            .services
            .iter()
            .filter(|service| matches(service))
            .filter(|service| {
                service.process.identity.pid != self.snapshot.instance_pid
                    && service.process.identity.started_at_secs.is_some()
                    && own_user.is_some()
                    && service.process.user_id == own_user
            })
            .map(|service| {
                let ports: Vec<u16> = {
                    let mut ports: Vec<u16> = service
                        .endpoints
                        .iter()
                        .map(|endpoint| endpoint.port)
                        .collect();
                    ports.sort_unstable();
                    ports.dedup();
                    ports
                };
                Confirmation {
                    command: self.command(verb, service.process.identity, &ports),
                    identity: service.process.identity,
                    ports,
                    project: service.project.clone(),
                    process: service.process.name.clone(),
                }
            })
            .collect()
    }

    fn command(&self, verb: Verb, identity: ProcessIdentity, ports: &[u16]) -> String {
        let ports: Vec<String> = ports.iter().map(ToString::to_string).collect();
        let mut command = format!(
            "kosse {} {} --ports {}",
            verb.name(),
            shown(identity),
            ports.join(",")
        );
        if self.instance_port != DEFAULT_PORT {
            let _ = write!(command, " --instance-port {}", self.instance_port);
        }
        command
    }

    /// A refusal of a request that named `what` instead of an identity: the commands that would
    /// confirm the processes it matches, or why there are none.
    fn unconfirmed(&self, verb: Verb, what: &str, matches: impl Fn(&Service) -> bool) -> Failure {
        let observation = self.fresh_observation();
        let confirm = self.confirmations(&observation, verb, &matches);
        if confirm.is_empty() {
            return if observation.services.iter().any(&matches) {
                Failure::new(
                    CommandErrorCode::ActionRefused,
                    format!(
                        "{what} matches only processes Kossé never signals (Kossé itself, another user's, or one without a start time); nothing was signalled"
                    ),
                )
            } else {
                Failure::new(
                    CommandErrorCode::NotFound,
                    format!("no observed process matches {what}; nothing was signalled"),
                )
            };
        }
        let count = match confirm.len() {
            1 => "1 process".to_owned(),
            several => format!("{several} processes"),
        };
        Failure {
            confirm,
            ..Failure::new(
                CommandErrorCode::ConfirmationRequired,
                format!(
                    "{what} matches {count}; {} takes the identity <pid>.<start> and --ports with every port it listens on, so nothing was signalled. To confirm one:",
                    verb.name()
                ),
            )
        }
    }

    fn not_observed(&self, identity: ProcessIdentity) -> Failure {
        Failure::new(
            CommandErrorCode::Stale,
            format!(
                "process {} is not observed by the Kossé at {} (it exited, listens on no TCP port, or is never signalled); nothing was signalled",
                shown(identity),
                self.instance.url
            ),
        )
    }
}

fn in_root(service: &Service, root: Option<&str>) -> bool {
    root.is_none_or(|root| service.project.as_deref() == Some(root))
}

fn listens_on(service: &Service, ports: &[u16]) -> bool {
    service
        .endpoints
        .iter()
        .any(|endpoint| ports.contains(&endpoint.port))
}

fn shown(identity: ProcessIdentity) -> String {
    format!(
        "{}.{}",
        identity.pid,
        identity
            .started_at_secs
            .map_or_else(|| UNKNOWN.to_owned(), |start| start.to_string())
    )
}

fn start_command(instance_port: u16) -> String {
    if instance_port == DEFAULT_PORT {
        "kosse serve --no-open".to_owned()
    } else {
        format!("kosse serve --no-open --port {instance_port}")
    }
}

/// Prints a result the instance signalled for, or turns one that signalled nothing into a
/// refusal (see [`refusal`]).
fn answer(
    args: &ActionArgs,
    verb: Verb,
    client: &Client,
    request: &ActionRequest,
    result: ActionResult,
) -> Result<u8, Failure> {
    let code = match &result {
        ActionResult::Exited { .. } => 0,
        ActionResult::StillRunning { .. }
        | ActionResult::Respawned { .. }
        | ActionResult::EndpointTaken { .. } => 1,
        ActionResult::ObservationFailed { .. } => 4,
        ActionResult::Stale { .. }
        | ActionResult::ForceNotOffered
        | ActionResult::Refused { .. }
        | ActionResult::PermissionDenied { .. }
        | ActionResult::SignalFailed { .. }
        | ActionResult::Internal { .. } => return Err(refusal(verb, client, request, result)),
    };
    let force_command = match (&result, verb) {
        (
            ActionResult::StillRunning {
                target,
                ports,
                force_until_ms: Some(_),
            },
            Verb::Stop,
        ) => Some(client.command(Verb::Force, *target, ports)),
        _ => None,
    };
    let mut stdout = io::stdout().lock();
    let written = if args.json {
        list::write_json(
            &mut stdout,
            &ActionAnswer {
                schema_version: AGENT_SCHEMA_VERSION,
                result,
                force_command,
            },
        )
    } else {
        write_text(&mut stdout, &result, force_command.as_deref())
    };
    failure::written(written)?;
    Ok(code)
}

/// A result that signalled nothing (or, for `internal`, maybe something), with the commands that
/// would confirm what runs now where one exists.
fn refusal(verb: Verb, client: &Client, request: &ActionRequest, result: ActionResult) -> Failure {
    let identity = request.identity;
    let (code, message, confirm) = match &result {
        ActionResult::Stale { reason } => {
            // The processes the request could have meant: that PID, or what holds its ports now.
            let confirm = client.confirmations(&client.fresh_observation(), verb, |service| {
                service.process.identity.pid == identity.pid || listens_on(service, &request.ports)
            });
            let to_confirm = if confirm.is_empty() {
                ""
            } else {
                " To confirm what runs now:"
            };
            (
                CommandErrorCode::Stale,
                format!(
                    "{}; nothing was signalled.{to_confirm}",
                    stale(*reason, identity, &request.ports)
                ),
                confirm,
            )
        }
        ActionResult::ForceNotOffered => (
            CommandErrorCode::ForceNotOffered,
            format!(
                "force is offered only after a `kosse stop` of the same identity and ports ended stillRunning on this instance, for {} s and once; nothing was signalled. Stop it first:",
                FORCE_OFFER_TTL.as_secs()
            ),
            client.confirmations(&client.fresh_observation(), Verb::Stop, |service| {
                service.process.identity == identity
            }),
        ),
        ActionResult::Refused { reason } => (
            CommandErrorCode::ActionRefused,
            format!(
                "Kossé never signals process {}: {}; nothing was signalled",
                shown(identity),
                ineligible(*reason)
            ),
            Vec::new(),
        ),
        ActionResult::PermissionDenied { target } => (
            CommandErrorCode::ActionRefused,
            format!(
                "the OS refused to signal process {}; nothing was signalled",
                shown(*target)
            ),
            Vec::new(),
        ),
        ActionResult::SignalFailed { reason, .. } => (
            CommandErrorCode::ActionFailed,
            format!("the checks before the signal failed ({reason}); nothing was signalled"),
            Vec::new(),
        ),
        ActionResult::Internal { reason } => (
            CommandErrorCode::ActionFailed,
            format!(
                "the instance failed while handling the request ({reason}); whether a signal was sent is unknown: check with `kosse list`"
            ),
            Vec::new(),
        ),
        // Signalled: printed by `answer`, never a refusal.
        ActionResult::Exited { .. }
        | ActionResult::StillRunning { .. }
        | ActionResult::Respawned { .. }
        | ActionResult::EndpointTaken { .. }
        | ActionResult::ObservationFailed { .. } => (
            CommandErrorCode::ActionFailed,
            "unexpected action result".to_owned(),
            Vec::new(),
        ),
    };
    Failure {
        confirm,
        result: Some(Box::new(result)),
        ..Failure::new(code, message)
    }
}

fn stale(reason: StaleReason, identity: ProcessIdentity, ports: &[u16]) -> String {
    let id = shown(identity);
    match reason {
        StaleReason::InstanceChanged => "the Kossé instance changed during the request".to_owned(),
        StaleReason::Exited => format!("process {id} has already exited"),
        StaleReason::IdentityChanged => format!(
            "PID {} now names another process (another start time)",
            identity.pid
        ),
        StaleReason::NotInSnapshot => {
            format!("process {id} is not in the instance's current snapshot")
        }
        StaleReason::PortsChanged => format!(
            "process {id} does not listen on exactly the ports {}",
            port_list(ports)
        ),
    }
}

fn ineligible(reason: Ineligible) -> &'static str {
    match reason {
        Ineligible::SystemProcess => "PID 0 and 1 are the kernel and launchd",
        Ineligible::KosseItself => "it is Kossé itself (quit it instead)",
        Ineligible::NoStartTime => "without a start time a reused PID cannot be told apart",
        Ineligible::OtherUser => "it runs as another user",
        Ineligible::OwnerUnknown => "the OS does not disclose its owner",
        Ineligible::Unreadable => "the OS returned nothing about it",
        Ineligible::OutsideTestScope => "it is outside this test instance's --act-only scope",
    }
}

fn port_list(ports: &[u16]) -> String {
    let ports: Vec<String> = ports.iter().map(ToString::to_string).collect();
    ports.join(", ")
}

/// `key: value` lines, then a table of who listens on the ports when anyone does.
fn write_text(
    out: &mut impl Write,
    result: &ActionResult,
    force_command: Option<&str>,
) -> io::Result<()> {
    let mut owners: Vec<(&PortOwner, &str)> = Vec::new();
    match result {
        ActionResult::Exited {
            target,
            ports,
            shared,
        } => {
            writeln!(out, "outcome: exited")?;
            writeln!(out, "process: {} (exited)", shown(*target))?;
            let free = if shared.is_empty() {
                "free"
            } else {
                "no new listener; processes that already shared these port numbers still listen"
            };
            writeln!(out, "ports: {} ({free})", port_list(ports))?;
            owners.extend(shared.iter().map(|owner| (owner, "shared")));
        }
        ActionResult::StillRunning {
            target,
            ports,
            force_until_ms,
        } => {
            writeln!(out, "outcome: stillRunning")?;
            writeln!(
                out,
                "process: {} (still running; it may ignore SIGTERM)",
                shown(*target)
            )?;
            writeln!(out, "ports: {}", port_list(ports))?;
            match (force_command, force_until_ms) {
                (Some(command), Some(until)) => {
                    let left = until.saturating_sub(unix_millis(SystemTime::now())) / 1000;
                    writeln!(
                        out,
                        "force: {command}  (offered for {left} s, once; never sent automatically)"
                    )?;
                }
                _ => writeln!(
                    out,
                    "force: not offered (the process could not be confirmed as the same one)"
                )?,
            }
        }
        ActionResult::Respawned {
            target,
            parent_pid,
            owners: new,
            shared,
        } => {
            writeln!(out, "outcome: respawned")?;
            writeln!(out, "process: {} (exited)", shown(*target))?;
            writeln!(
                out,
                "parent: {parent_pid} (likely a supervisor that restarted it; not signalled)"
            )?;
            owners.extend(new.iter().map(|owner| (owner, "new")));
            owners.extend(shared.iter().map(|owner| (owner, "shared")));
        }
        ActionResult::EndpointTaken {
            target,
            owners: new,
            shared,
        } => {
            writeln!(out, "outcome: endpointTaken")?;
            writeln!(out, "process: {} (exited)", shown(*target))?;
            owners.extend(new.iter().map(|owner| (owner, "new")));
            owners.extend(shared.iter().map(|owner| (owner, "shared")));
        }
        ActionResult::ObservationFailed {
            target,
            process_exited,
            reason,
        } => {
            writeln!(out, "outcome: observationFailed")?;
            let state = if *process_exited {
                "exited; its ports could not be checked"
            } else {
                "unknown"
            };
            writeln!(out, "process: {} ({state})", shown(*target))?;
            writeln!(out, "reason: {}", printable(reason))?;
        }
        // Signalled nothing: reported as failures by `answer`.
        ActionResult::Stale { .. }
        | ActionResult::ForceNotOffered
        | ActionResult::Refused { .. }
        | ActionResult::PermissionDenied { .. }
        | ActionResult::SignalFailed { .. }
        | ActionResult::Internal { .. } => {}
    }
    write_owners(out, &owners)
}

/// Who listens on the ports after the exit: `new` processes, or `shared` ones that already did.
fn write_owners(out: &mut impl Write, owners: &[(&PortOwner, &str)]) -> io::Result<()> {
    if owners.is_empty() {
        return Ok(());
    }
    let mut table = Table::new(["PORT", "PID.START", "PROCESS", "PARENT", "LISTENER"]);
    for &(owner, role) in owners {
        table.row([
            &owner.port.to_string(),
            &shown(owner.identity),
            owner.name.as_deref().unwrap_or(UNKNOWN),
            &owner
                .parent_pid
                .map_or_else(|| UNKNOWN.to_owned(), |pid| pid.to_string()),
            role,
        ]);
    }
    writeln!(out)?;
    table.write(out)
}
