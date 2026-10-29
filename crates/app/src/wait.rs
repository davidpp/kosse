//! `kosse wait`: until a service or port listens, answers HTTP (`--http`) or is gone (`--gone`),
//! within `--timeout`.
//!
//! Observation, with no second scanner beside a running one:
//! - With a verified instance (see [`crate::instance`]) whose snapshots this build reads, it
//!   follows the instance's `GET /api/events` stream, ordered by `instance` + `revision`; the
//!   instance scans every 3 s.
//! - Otherwise it scans in process every [`SCAN_INTERVAL`] with its own [`Scanner`], profiles
//!   applied as `serve` applies them.
//!
//! `--http` asks for probes exactly as the dashboard's "try HTTP" does, so there is one probe
//! policy: through the instance's `POST /api/probe` (session token, same origin, its rate
//! limit), or in process through [`Probes::request`] after the same
//! [`probe::requested_target`] check. Only the endpoints of the one matching service are probed,
//! first at once, then after 1 s, doubling to at most [`PROBE_BACKOFF_MAX`]; a rate-limited or
//! stale request is retried after a second. Any HTTP response counts (404 and 500 included). An
//! endpoint that may never be probed (another user's, not on loopback) fails at once (exit 3),
//! and one that answered but not HTTP (a banner, a failed TLS check) is a negative answer at once
//! (exit 1), instead of waiting out the timeout.
//!
//! Either way the last observation is printed: the service table's rows (or the `WaitAnswer`
//! record); a timeout exits 1.

use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    process::ExitCode,
    time::{Duration, Instant, SystemTime},
};

use hyper::StatusCode;
use kosse_core::{
    agent::{AGENT_SCHEMA_VERSION, CommandErrorCode, WaitAnswer, WaitCondition},
    contract::ProbeRequest,
    model::{
        Endpoint, HttpProbe, ProbeResult, ProcessIdentity, SNAPSHOT_SCHEMA_VERSION, ScanWarning,
        Snapshot,
    },
    probe,
    profiles::{self, CentralStore, Profiles},
    scan::{Scanner, unix_millis},
    snapshot::{AttemptTimer, SnapshotTracker},
};

use crate::{
    cli::WaitArgs,
    events::Events,
    failure::{self, Failure},
    instance::{self, Found},
    list,
    prober::{Probes, Scope},
    scope::{self, Filter},
};

/// How often the in-process scanner looks when no instance runs.
pub const SCAN_INTERVAL: Duration = Duration::from_secs(1);
/// The first retry of a probe: the API's limit of one requested probe a second.
const PROBE_BACKOFF_START: Duration = crate::api::WRITE_MIN_INTERVAL;
const PROBE_BACKOFF_MAX: Duration = Duration::from_secs(5);
const MAX_TIMEOUT: Duration = Duration::from_hours(24);

/// `500ms`, `30s`, `2m`, `1h`, or a number of seconds.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let invalid = || format!("{text:?} is not a duration such as 500ms, 30s or 2m");
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: u64 = number.parse().map_err(|_| invalid())?;
    let duration = match unit {
        "ms" => Duration::from_millis(value),
        "" | "s" => Duration::from_secs(value),
        "m" => Duration::from_secs(value.saturating_mul(60)),
        "h" => Duration::from_secs(value.saturating_mul(3600)),
        _ => return Err(invalid()),
    };
    if duration > MAX_TIMEOUT {
        return Err(format!("{text:?} is longer than a day"));
    }
    Ok(duration)
}

pub fn run(args: &WaitArgs) -> ExitCode {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            Failure::new(
                CommandErrorCode::ScanFailed,
                format!("could not start the async runtime: {error}"),
            )
        })
        .and_then(|runtime| runtime.block_on(wait(args)));
    failure::exit(result, args.json)
}

async fn wait(args: &WaitArgs) -> Result<(), Failure> {
    let started = Instant::now();
    let deadline = started + args.timeout;
    let condition = if args.http {
        WaitCondition::Http
    } else if args.gone {
        WaitCondition::Gone
    } else {
        WaitCondition::Listening
    };
    let what = args.port.map_or_else(
        || format!("service {:?}", args.service.as_deref().unwrap_or_default()),
        |port| format!("port {port}"),
    );
    let mut source = Source::open(args.instance.instance_port).await?;
    let Some(mut latest) = source.next(deadline).await? else {
        return Err(Failure::new(
            CommandErrorCode::TimedOut,
            format!("no observation within {}", shown(args.timeout)),
        ));
    };
    let filter = Filter {
        root: scope::root(&args.scope, &latest)?,
        port: args.port,
        service: args.service.clone(),
    };
    // A name configured nowhere in scope can never match: say so now, for every condition (and
    // never report it "gone").
    if let Some(name) = &args.service {
        scope::require_configured(&latest, filter.root.as_deref(), name)?;
    }
    let probes_run = source.probes_run();
    let mut schedule = Schedule::default();
    let mut announced = false;
    loop {
        let mut view = latest.clone();
        filter.apply(&mut view);
        let unknown = unknown_on_port(&latest, &filter);
        let output = Output {
            args,
            condition,
            view: &view,
            probes_run,
            started,
        };
        let due = match judge(condition, &view, unknown, &filter, &what) {
            Verdict::Met { url } => return output.print(url, true),
            Verdict::Never(failure) => {
                output.print(None, false)?;
                return Err(failure);
            }
            Verdict::Waiting { probe } => probe,
        };
        let mut probed = false;
        for key in due {
            if !schedule.due(key, Instant::now()) || Instant::now() >= deadline {
                continue;
            }
            match source.probe(&latest, key).await? {
                Some(result) => {
                    patch(&mut latest, key, result);
                    schedule.tried(key);
                    probed = true;
                }
                None => schedule.later(key),
            }
        }
        if probed {
            continue;
        }
        if Instant::now() >= deadline {
            output.print(None, false)?;
            let failed = match condition {
                WaitCondition::Listening => "did not listen",
                WaitCondition::Http => "did not answer HTTP",
                WaitCondition::Gone => "was not gone",
            };
            return Err(Failure::new(
                CommandErrorCode::TimedOut,
                format!(
                    "{what} {failed} within {}{}",
                    shown(args.timeout),
                    last_states(condition, &view, unknown, args.port, probes_run)
                ),
            ));
        }
        if !announced && !args.json {
            report!(
                "kosse: waiting up to {} for {what} to {}",
                shown(args.timeout),
                verb(condition)
            );
            announced = true;
        }
        let wake = schedule.next().map_or(deadline, |next| next.min(deadline));
        if let Some(snapshot) = source.next(wake).await? {
            latest = snapshot;
        }
    }
}

enum Verdict {
    Met {
        url: Option<String>,
    },
    /// Not yet; for `--http`, the endpoints worth a probe.
    Waiting {
        probe: Vec<Key>,
    },
    /// It can never hold (or is ambiguous): stop now.
    Never(Failure),
}

type Key = (ProcessIdentity, u16);

/// Under `--here`/`--project` with `--port`: a process listens on the port but its project is
/// unknown, so it may be in scope (and the port is not known to be released).
fn unknown_on_port(snapshot: &Snapshot, filter: &Filter) -> bool {
    let (Some(_), Some(port)) = (&filter.root, filter.port) else {
        return false;
    };
    snapshot.observation.as_ref().is_some_and(|observation| {
        observation.services.iter().any(|service| {
            service.project.is_none()
                && service
                    .endpoints
                    .iter()
                    .any(|endpoint| endpoint.port == port)
        })
    })
}

/// Whether `condition` holds in `view` (already filtered to the matching services); `unknown`
/// is [`unknown_on_port`].
fn judge(
    condition: WaitCondition,
    view: &Snapshot,
    unknown: bool,
    filter: &Filter,
    what: &str,
) -> Verdict {
    let port = filter.port;
    let services = view
        .observation
        .as_ref()
        .map_or(&[][..], |observation| &observation.services);
    match condition {
        WaitCondition::Listening if services.is_empty() => Verdict::Waiting { probe: Vec::new() },
        WaitCondition::Listening => Verdict::Met { url: None },
        WaitCondition::Gone if services.is_empty() && !unknown => Verdict::Met { url: None },
        WaitCondition::Gone => Verdict::Waiting { probe: Vec::new() },
        WaitCondition::Http => match services {
            [] => Verdict::Waiting { probe: Vec::new() },
            [service] => {
                let endpoints: Vec<&Endpoint> = service
                    .endpoints
                    .iter()
                    .filter(|endpoint| port.is_none_or(|port| endpoint.port == port))
                    .collect();
                if let Some(url) = endpoints
                    .iter()
                    .find_map(|endpoint| list::reachable_url(endpoint))
                {
                    return Verdict::Met { url: Some(url) };
                }
                let retry: Vec<Key> = endpoints
                    .iter()
                    .filter(|endpoint| {
                        probe::requested_target(endpoint).is_some() && !answered_not_http(endpoint)
                    })
                    .map(|endpoint| (service.process.identity, endpoint.port))
                    .collect();
                if !retry.is_empty() {
                    return Verdict::Waiting { probe: retry };
                }
                let states = states(&endpoints, true);
                if endpoints
                    .iter()
                    .all(|endpoint| probe::requested_target(endpoint).is_none())
                {
                    Verdict::Never(Failure::new(
                        CommandErrorCode::NotProbeable,
                        format!(
                            "{what} may never be probed ({states}): Kossé probes only the user's own processes, on loopback"
                        ),
                    ))
                } else {
                    Verdict::Never(Failure::new(
                        CommandErrorCode::NoHttp,
                        format!("{what} answered, but not over HTTP ({states})"),
                    ))
                }
            }
            several => Verdict::Never(Failure::new(
                CommandErrorCode::Ambiguous,
                scope::ambiguity(what, several, filter.root.as_deref(), |port| {
                    format!("kosse wait --port {port} --http")
                }),
            )),
        },
    }
}

/// A definite answer that is not HTTP: retrying cannot change it.
fn answered_not_http(endpoint: &Endpoint) -> bool {
    matches!(
        endpoint.http,
        HttpProbe::Probed {
            result: ProbeResult::NotHttp | ProbeResult::TlsFailed { .. } | ProbeResult::InvalidPath,
            ..
        }
    )
}

fn states(endpoints: &[&Endpoint], probes_run: bool) -> String {
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
    states.join("; ")
}

fn last_states(
    condition: WaitCondition,
    view: &Snapshot,
    unknown: bool,
    port: Option<u16>,
    probes_run: bool,
) -> String {
    let Some(observation) = view.observation.as_ref() else {
        return String::new();
    };
    let endpoints: Vec<&Endpoint> = observation
        .services
        .iter()
        .flat_map(|service| &service.endpoints)
        .filter(|endpoint| port.is_none_or(|port| endpoint.port == port))
        .collect();
    match condition {
        WaitCondition::Http if !endpoints.is_empty() => {
            format!(" (last: {})", states(&endpoints, probes_run))
        }
        WaitCondition::Http | WaitCondition::Listening => " (nothing listens)".to_owned(),
        WaitCondition::Gone if endpoints.is_empty() && unknown => {
            " (a process whose project is unknown still listens on the port)".to_owned()
        }
        WaitCondition::Gone => " (still listening)".to_owned(),
    }
}

fn verb(condition: WaitCondition) -> &'static str {
    match condition {
        WaitCondition::Listening => "listen",
        WaitCondition::Http => "answer HTTP",
        WaitCondition::Gone => "be gone",
    }
}

fn shown(duration: Duration) -> String {
    if duration.subsec_millis() == 0 {
        format!("{} s", duration.as_secs())
    } else {
        format!("{} ms", duration.as_millis())
    }
}

/// Puts a probe's result on its endpoint, as the next published snapshot will.
fn patch(snapshot: &mut Snapshot, (identity, port): Key, result: HttpProbe) {
    let endpoint = snapshot
        .observation
        .as_mut()
        .and_then(|observation| {
            observation
                .services
                .iter_mut()
                .find(|service| service.process.identity == identity)
        })
        .and_then(|service| {
            service
                .endpoints
                .iter_mut()
                .find(|endpoint| endpoint.port == port)
        });
    if let Some(endpoint) = endpoint {
        endpoint.http = result;
    }
}

/// What the wait prints once it ends.
struct Output<'a> {
    args: &'a WaitArgs,
    condition: WaitCondition,
    view: &'a Snapshot,
    probes_run: bool,
    started: Instant,
}

impl Output<'_> {
    /// The last observation: the service table's rows, or the `WaitAnswer` record. A met
    /// `--gone` says, on stderr, that other users' listeners are invisible when they are.
    fn print(&self, url: Option<String>, met: bool) -> Result<(), Failure> {
        let Some(observation) = self.view.observation.as_ref() else {
            return Ok(());
        };
        let other_users_visible = !observation
            .warnings
            .contains(&ScanWarning::OtherUsersNotVisible);
        let mut stdout = io::stdout().lock();
        if self.args.json {
            let answer = WaitAnswer {
                schema_version: AGENT_SCHEMA_VERSION,
                condition: self.condition,
                met,
                waited_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                instance: self.view.instance.clone(),
                url,
                other_users_visible,
                scanned_at_ms: observation.scanned_at_ms,
                services: observation.services.clone(),
                projects: observation.projects.clone(),
            };
            return failure::written(list::write_json(&mut stdout, &answer));
        }
        failure::written(
            list::services_table(observation, self.args.port, self.probes_run).write(&mut stdout),
        )?;
        if met && self.condition == WaitCondition::Gone && !other_users_visible {
            report!(
                "kosse: gone as far as this user can see (other users' listeners, system services included, are hidden without administrator privileges)"
            );
        }
        Ok(())
    }
}

/// When each endpoint may be probed again.
#[derive(Default)]
struct Schedule {
    attempts: HashMap<Key, (u32, Instant)>,
}

impl Schedule {
    fn due(&self, key: Key, now: Instant) -> bool {
        self.attempts.get(&key).is_none_or(|(_, next)| now >= *next)
    }

    /// After a probe: 1 s, then doubling, at most [`PROBE_BACKOFF_MAX`].
    fn tried(&mut self, key: Key) {
        let entry = self.attempts.entry(key).or_insert((0, Instant::now()));
        let delay = PROBE_BACKOFF_START
            .saturating_mul(1 << entry.0.min(8))
            .min(PROBE_BACKOFF_MAX);
        *entry = (entry.0 + 1, Instant::now() + delay);
    }

    /// Rate-limited or stale: try again after the API's interval, without backing off.
    fn later(&mut self, key: Key) {
        let entry = self.attempts.entry(key).or_insert((0, Instant::now()));
        entry.1 = Instant::now() + PROBE_BACKOFF_START;
    }

    fn next(&self) -> Option<Instant> {
        self.attempts.values().map(|(_, next)| *next).min()
    }
}

/// Where snapshots come from.
enum Source {
    Remote(Box<Remote>),
    Local(Box<Local>),
}

struct Remote {
    addr: SocketAddr,
    url: String,
    events: Events,
    /// Snapshots from scans that started before the wait did are skipped: the instance's current
    /// snapshot can predate a process that just started or exited.
    since_ms: u64,
    /// The result of the refresh asked for at the start, returned first.
    refreshed: Option<Snapshot>,
}

struct Local {
    scanner: Scanner,
    profiles: Profiles,
    probes: Probes,
    tracker: SnapshotTracker,
    next_scan: Instant,
}

impl Source {
    async fn open(instance_port: u16) -> Result<Self, Failure> {
        if instance_port != 0
            && let Found::Kosse(existing) = instance::find(instance_port).await
        {
            let compatible = existing
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.schema_version == SNAPSHOT_SCHEMA_VERSION);
            if compatible {
                let events = Events::open(existing.addr).await.map_err(|reason| {
                    Failure::new(
                        CommandErrorCode::ScanFailed,
                        format!("could not follow the Kossé at {}: {reason}", existing.url),
                    )
                })?;
                let mut remote = Remote {
                    addr: existing.addr,
                    url: existing.url,
                    events,
                    since_ms: unix_millis(SystemTime::now()),
                    refreshed: None,
                };
                remote.refreshed = remote.refresh().await;
                return Ok(Self::Remote(Box::new(remote)));
            }
            // Scanning in a loop beside it would be a second scanner.
            let version = existing.snapshot.as_ref().map_or_else(
                || "none".to_owned(),
                |snapshot| snapshot.schema_version.to_string(),
            );
            return Err(Failure::new(
                CommandErrorCode::ScanFailed,
                format!(
                    "the Kossé at {} publishes snapshot version {version}, this kosse reads {SNAPSHOT_SCHEMA_VERSION}; `kosse wait` follows it rather than scanning beside it, so use a matching kosse (or stop that one)",
                    existing.url
                ),
            ));
        }
        Ok(Self::Local(Box::new(Local {
            scanner: Scanner::new(),
            profiles: Profiles::new(CentralStore::from_env()?),
            // Only requested probes run here (no prober task), so the automatic scope is moot.
            probes: Probes::new(Scope::All),
            tracker: SnapshotTracker::new(Some(SCAN_INTERVAL)),
            next_scan: Instant::now(),
        })))
    }

    /// Whether a prober follows these snapshots (an instance's), so `pending` means a probe is
    /// coming.
    fn probes_run(&self) -> bool {
        matches!(self, Self::Remote(_))
    }

    /// The next snapshot published before `until`; `None` when `until` comes first.
    async fn next(&mut self, until: Instant) -> Result<Option<Snapshot>, Failure> {
        match self {
            Self::Remote(remote) => {
                if let Some(snapshot) = remote.refreshed.take() {
                    return Ok(Some(snapshot));
                }
                loop {
                    let snapshot = match tokio::time::timeout_at(until.into(), remote.events.next())
                        .await
                    {
                        Err(_) => return Ok(None),
                        Ok(Ok(Some(snapshot))) => snapshot,
                        Ok(Ok(None)) => {
                            return Err(Failure::new(
                                CommandErrorCode::ScanFailed,
                                format!("the Kossé at {} stopped publishing snapshots", remote.url),
                            ));
                        }
                        Ok(Err(reason)) => {
                            return Err(Failure::new(
                                CommandErrorCode::ScanFailed,
                                format!(
                                    "the Kossé at {} sent an unreadable event: {reason}",
                                    remote.url
                                ),
                            ));
                        }
                    };
                    if remote.is_fresh(&snapshot) {
                        return Ok(Some(snapshot));
                    }
                }
            }
            Self::Local(local) => {
                if until < local.next_scan {
                    tokio::time::sleep_until(until.into()).await;
                    return Ok(None);
                }
                tokio::time::sleep_until(local.next_scan.into()).await;
                let timer = AttemptTimer::start();
                let mut observation = local.scanner.scan()?;
                local
                    .profiles
                    .apply(&mut observation, local.scanner.own_user());
                local.probes.annotate(&mut observation);
                local.next_scan = Instant::now() + SCAN_INTERVAL;
                Ok(Some(local.tracker.succeeded(observation, timer.finish())))
            }
        }
    }

    /// One requested probe of `key` as `snapshot` shows it; `None` when it should be retried
    /// later (rate-limited, or the identity is no longer current).
    async fn probe(&mut self, snapshot: &Snapshot, key: Key) -> Result<Option<HttpProbe>, Failure> {
        let not_probeable = || {
            Failure::new(
                CommandErrorCode::NotProbeable,
                format!(
                    "port {} may never be probed: Kossé probes only the user's own processes, on loopback",
                    key.1
                ),
            )
        };
        match self {
            Self::Remote(remote) => remote.probe(key, not_probeable).await,
            Self::Local(local) => {
                let Some(observation) = snapshot.observation.as_ref() else {
                    return Ok(None);
                };
                let Some((service, endpoint)) = observation.services.iter().find_map(|service| {
                    (service.process.identity == key.0)
                        .then(|| service.endpoints.iter().find(|e| e.port == key.1))
                        .flatten()
                        .map(|endpoint| (service, endpoint))
                }) else {
                    return Ok(None);
                };
                let target = probe::requested_target(endpoint).ok_or_else(not_probeable)?;
                let path = profiles::probe_path(observation, service, key.1);
                Ok(Some(
                    local
                        .probes
                        .request(key, target, path, &snapshot.instance)
                        .await,
                ))
            }
        }
    }
}

impl Remote {
    fn is_fresh(&self, snapshot: &Snapshot) -> bool {
        snapshot
            .observation
            .as_ref()
            .is_some_and(|observation| observation.scanned_at_ms >= self.since_ms)
    }

    /// Asks for a scan now (`POST /api/refresh`, the dashboard's Refresh), so the first answer
    /// need not wait for the next scheduled scan. Best effort: when it is rate-limited or fails,
    /// the event stream brings the next scan.
    async fn refresh(&mut self) -> Option<Snapshot> {
        let (status, body) = instance::post(
            self.addr,
            "/api/refresh",
            b"{}".to_vec(),
            instance::REQUEST_TIMEOUT,
        )
        .await
        .ok()?;
        let snapshot: Snapshot = (status == StatusCode::OK)
            .then(|| serde_json::from_slice(&body).ok())
            .flatten()?;
        self.is_fresh(&snapshot).then_some(snapshot)
    }

    /// `POST /api/probe` as the dashboard sends it.
    async fn probe(
        &mut self,
        (identity, port): Key,
        not_probeable: impl Fn() -> Failure,
    ) -> Result<Option<HttpProbe>, Failure> {
        let url = self.url.clone();
        let failed = |reason: String| {
            Failure::new(
                CommandErrorCode::ScanFailed,
                format!("could not ask the Kossé at {url} for a probe: {reason}"),
            )
        };
        let body = serde_json::to_vec(&ProbeRequest { identity, port })
            .map_err(|error| failed(error.to_string()))?;
        let (status, body) =
            instance::post(self.addr, "/api/probe", body, instance::REQUEST_TIMEOUT)
                .await
                .map_err(|error| failed(error.to_string()))?;
        match status {
            StatusCode::OK => serde_json::from_slice(&body)
                .map(Some)
                .map_err(|error| failed(error.to_string())),
            StatusCode::TOO_MANY_REQUESTS | StatusCode::CONFLICT => Ok(None),
            StatusCode::UNPROCESSABLE_ENTITY => Err(not_probeable()),
            other => Err(failed(format!(
                "{other} {}",
                String::from_utf8_lossy(&body)
            ))),
        }
    }
}
