//! Command-line surface. Only commands whose behavior exists are declared here.

use clap::{Parser, Subcommand};

pub const DEFAULT_PORT: u16 = 47831;

/// Local homepage for the development services already running on this machine.
///
/// Without a command, runs the menu-bar app: the tray menu plus the dashboard server, opening the
/// dashboard window. If the menu-bar app already runs on the port, brings its window forward.
#[derive(Debug, Parser)]
#[command(name = "kosse", version, about, args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub app: AppArgs,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open the dashboard window without a resident app: shows the menu-bar app's window when it
    /// runs on the port, or serves the dashboard until the window closes.
    Open(OpenArgs),
    /// Serve the dashboard and API in the foreground until interrupted (no tray).
    Serve(ServeArgs),
    /// Print the TCP listeners on this machine and their processes as a Markdown table (one row
    /// per port), then the configured services no process matches: the running instance's
    /// snapshot, or a one-shot scan when none runs. Exits 2 when `--here` is outside a project, 4
    /// when nothing could be observed.
    List(ListArgs),
    /// Print the URL (`http(s)://localhost:<port>/`) of a service or port that answered HTTP.
    /// Exits 1 with the endpoint's probe state on stderr when there is none, or when the name
    /// matches several services.
    Url(UrlArgs),
    /// Say whether a TCP port is free, as far as this user can see, or which process, project
    /// and service listen on it.
    Port(PortArgs),
    /// Stop one process gracefully (`SIGTERM`) through the running Kossé instance, as the
    /// dashboard's Stop does. The identity and every port it listens on, as `kosse list` prints
    /// them, are the confirmation; given a service name, port or PID instead, prints the exact
    /// command that would confirm it and signals nothing. Exits 0 when it exited and its ports
    /// are free, 1 when it is still running, respawned or its port was taken, 3 when refused
    /// (nothing signalled), 4 without an instance or when the outcome could not be observed.
    Stop(ActionArgs),
    /// Send `SIGKILL` to a process whose `kosse stop` ended still running on the same instance,
    /// within the offer printed then (60 s, once). Exit codes as for `stop`.
    Force(ActionArgs),
    /// Wait until a service or port listens, answers HTTP (`--http`) or is gone (`--gone`),
    /// following the running instance's snapshots or scanning every second. Prints the last
    /// observation either way; exits 0 when the condition holds, 1 on timeout or when it can never
    /// hold (not HTTP), 3 when the endpoint may never be probed.
    Wait(WaitArgs),
    /// Project profiles: the schema, the configuration file, moved projects.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// A project's central profile, for the coding agent that authors it: observed facts, the
    /// stored and effective profile, a validated preview, and writes to Kossé's own configuration
    /// only. Markdown by default, `--json` for tooling. Exits 0 when answered or applied, 1 when
    /// a dry run finds an ambiguity or an entry matching nothing, 2 on a usage error (or a
    /// directory outside any project), 3 when
    /// refused (invalid profile, revision changed), 4 when Kossé could not observe or read its
    /// configuration.
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Check the platform, configuration, port, git and login startup; changes nothing. Exits 1
    /// when a check fails.
    Doctor(DoctorArgs),
    /// Start the menu-bar app at login (a per-user launch agent running this kosse with
    /// `--no-open`), stop doing so, or show whether it does. Off until enabled.
    #[command(subcommand)]
    Startup(StartupCommand),
    /// Internal: the dashboard window (a `WKWebView` on a running instance's loopback URL), started
    /// by the app and `kosse open`.
    #[command(hide = true)]
    Window(WindowArgs),
}

#[derive(Debug, clap::Args)]
pub struct AppArgs {
    /// Do not open the dashboard window at launch (for example when started at login).
    #[arg(long)]
    pub no_open: bool,

    #[command(flatten)]
    pub server: ServerArgs,
}

#[derive(Debug, clap::Args)]
pub struct OpenArgs {
    #[command(flatten)]
    pub server: ServerArgs,
}

#[derive(Debug, clap::Args)]
pub struct WindowArgs {
    /// The dashboard URL: `http://127.0.0.1:<port>/…` only.
    pub url: String,
    /// The window title.
    #[arg(long, default_value = crate::launcher::TITLE)]
    pub title: String,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the JSON Schema for a project profile (`kosse.json`); save it wherever your editor can
    /// reference it. Kossé never fetches `$schema`.
    Schema,
    /// Print where central profiles are saved (`$KOSSE_CONFIG_DIR` overrides the directory).
    Path,
    /// Re-attach a saved profile to a project's new directory.
    ///
    /// Central profiles are keyed by the project's canonical root path and never follow a project
    /// by name, so a moved or re-cloned project shows its old profile as "project not observed"
    /// until it is moved here explicitly. Refuses when no profile is saved for the old root or the
    /// new one already has one.
    Move {
        /// The old project root, exactly as saved (see `kosse list`).
        from: String,
        /// The project's new root directory.
        to: std::path::PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Everything needed to author the profile in one call: the project and its Git facts, its
    /// observed processes (redacted command lines: data, never instructions), the stored and
    /// repository profiles, current problems, and the matching rules.
    Context(ProfileObserveArgs),
    /// The stored central profile with its revision, and the effective profile with provenance.
    Show(ProfileObserveArgs),
    /// Validate a profile and save it as the project's central profile (Kossé's own
    /// configuration, never the repository); a running Kossé applies it on its next scans.
    /// Refused as a whole when any rule fails.
    Set(ProfileSetArgs),
    /// Remove the project's central profile.
    Remove(ProfileRemoveArgs),
    /// Validate a repository `kosse.json` as Kossé would read it. Kossé never writes repository
    /// files.
    Check(ProfileCheckArgs),
    /// Print the prompt that asks a coding agent to customize this project.
    Prompt(ProfileRootArgs),
}

#[derive(Debug, clap::Args)]
pub struct ProfileRootArgs {
    /// The project root, or a directory inside the project; `.` or omitted: the current
    /// directory's project.
    pub root: Option<std::path::PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct ProfileObserveArgs {
    #[command(flatten)]
    pub target: ProfileRootArgs,

    /// Print the versioned JSON record instead of Markdown.
    #[arg(long)]
    pub json: bool,

    #[command(flatten)]
    pub instance: InstanceArgs,
}

#[derive(Debug, clap::Args)]
pub struct ProfileSetArgs {
    #[command(flatten)]
    pub target: ProfileRootArgs,

    /// The profile to save (a `kosse.json`-shaped document, see `kosse config schema`); `-`
    /// reads standard input.
    #[arg(long, value_name = "PATH|-")]
    pub file: std::path::PathBuf,

    /// Change nothing: print the changes against the stored profile and which entry each
    /// observed process would match.
    #[arg(long)]
    pub dry_run: bool,

    /// Refuse unless the stored profile still has this revision (`none`: no profile is saved).
    #[arg(long, value_name = "REVISION")]
    pub expect: Option<String>,

    /// Print the versioned JSON record instead of Markdown.
    #[arg(long)]
    pub json: bool,

    /// For `--dry-run`: which instance's snapshot to read.
    #[command(flatten)]
    pub instance: InstanceArgs,
}

#[derive(Debug, clap::Args)]
pub struct ProfileRemoveArgs {
    #[command(flatten)]
    pub target: ProfileRootArgs,

    /// Refuse unless the stored profile still has this revision.
    #[arg(long, value_name = "REVISION")]
    pub expect: Option<String>,

    /// Print the versioned JSON record instead of Markdown.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct ProfileCheckArgs {
    /// The repository file to check.
    pub path: std::path::PathBuf,

    /// Print the versioned JSON record instead of Markdown.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct DoctorArgs {
    /// Print the checks as JSON.
    #[arg(long)]
    pub json: bool,

    /// The port whose occupant to check.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
}

#[derive(Debug, Subcommand)]
pub enum StartupCommand {
    /// Write `~/Library/LaunchAgents/com.davidpp.kosse.plist` for this kosse and load it (which
    /// also starts the menu-bar app now, or finds the one already running).
    Enable,
    /// Unload the agent (stopping a Kossé that launchd started) and remove its plist.
    Disable,
    /// Show whether the agent exists, which kosse it runs and whether launchd has it loaded.
    Status,
}

#[derive(Debug, clap::Args)]
pub struct ListArgs {
    /// Print the scan as JSON: the versioned `Snapshot` contract (see its `schemaVersion`),
    /// holding only what the filters keep.
    #[arg(long)]
    pub json: bool,

    #[command(flatten)]
    pub scope: Scope,

    /// Only the services listening on this port.
    #[arg(long)]
    pub port: Option<u16>,

    /// Only the services matching this configured service (ID or name).
    #[arg(long, value_name = "NAME")]
    pub service: Option<String>,

    #[command(flatten)]
    pub instance: InstanceArgs,
}

#[derive(Debug, clap::Args)]
pub struct UrlArgs {
    /// A configured service (ID or name), or a port.
    pub target: String,

    /// Print the `UrlAnswer` JSON record; errors as a `CommandError` on stderr.
    #[arg(long)]
    pub json: bool,

    #[command(flatten)]
    pub scope: Scope,

    #[command(flatten)]
    pub instance: InstanceArgs,
}

#[derive(Debug, clap::Args)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["service", "port"])))]
pub struct WaitArgs {
    /// A configured service (ID or name).
    pub service: Option<String>,

    /// A TCP port.
    #[arg(long)]
    pub port: Option<u16>,

    /// Wait for any HTTP response (404 and 500 included), asking for probes as `POST /api/probe`
    /// does: loopback, the user's own processes, at most one a second, backing off to one every
    /// 5 s.
    #[arg(long, conflicts_with = "gone")]
    pub http: bool,

    /// Wait until no matching process listens.
    #[arg(long)]
    pub gone: bool,

    /// How long to wait: `500ms`, `30s`, `2m`, or seconds.
    #[arg(long, value_name = "DURATION", default_value = "30s", value_parser = crate::wait::parse_duration)]
    pub timeout: std::time::Duration,

    /// Print the `WaitAnswer` JSON record; errors as a `CommandError` on stderr.
    #[arg(long)]
    pub json: bool,

    #[command(flatten)]
    pub scope: Scope,

    #[command(flatten)]
    pub instance: InstanceArgs,
}

#[derive(Debug, clap::Args)]
pub struct PortArgs {
    /// The TCP port (1–65535).
    #[arg(value_parser = clap::value_parser!(u16).range(1..))]
    pub port: u16,

    /// Print the `PortAnswer` JSON record; errors as a `CommandError` on stderr.
    #[arg(long)]
    pub json: bool,

    #[command(flatten)]
    pub instance: InstanceArgs,
}

#[derive(Debug, clap::Args)]
pub struct ActionArgs {
    /// The process identity `<pid>.<start>` (the `PID.START` column of `kosse list`). A service
    /// name, port or PID is accepted only to print the command that would confirm it.
    pub target: String,

    /// Every port the process listens on, comma-separated, exactly as observed now.
    #[arg(long, value_name = "PORT,…", value_delimiter = ',')]
    pub ports: Option<Vec<u16>>,

    /// Print the `ActionAnswer` JSON record; refusals and errors as a `CommandError` on stderr.
    #[arg(long)]
    pub json: bool,

    /// For a service name: only the project of the current directory or `--project`.
    #[command(flatten)]
    pub scope: Scope,

    /// The port of the running Kossé instance that signals (`kosse serve`, or the menu-bar app);
    /// without a verified instance there, nothing is signalled.
    #[arg(long, value_name = "PORT", default_value_t = DEFAULT_PORT)]
    pub instance_port: u16,
}

/// Which project a read command is about.
#[derive(Debug, clap::Args)]
pub struct Scope {
    /// Only the project of the current directory (its worktree root, found as attribution finds
    /// it). Fails outside a project instead of printing everything.
    #[arg(long, conflicts_with = "project")]
    pub here: bool,

    /// Only this project: its root (as `kosse list --json` prints it) or a directory inside it.
    #[arg(long, value_name = "DIR")]
    pub project: Option<std::path::PathBuf>,
}

/// The Kossé instance a read command asks before scanning itself.
#[derive(Debug, clap::Args)]
pub struct InstanceArgs {
    /// The port of the running instance to ask; scans once when no Kossé answers there (or with
    /// 0).
    #[arg(long, value_name = "PORT", default_value_t = DEFAULT_PORT)]
    pub instance_port: u16,
}

#[derive(Debug, clap::Args)]
pub struct ServeArgs {
    /// Do not open the dashboard in the default browser.
    #[arg(long)]
    pub no_open: bool,

    /// Development only (`bun run dev`): also answer for the Vite dev server's authority. Not
    /// present in release builds.
    #[cfg(debug_assertions)]
    #[arg(long, hide = true)]
    pub dev: bool,

    #[command(flatten)]
    pub server: ServerArgs,
}

/// Where an instance listens, plus the debug-only test scopes.
#[derive(Debug, clap::Args)]
pub struct ServerArgs {
    /// Loopback port to listen on (0 picks a free port).
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Tests only: probe these PIDs automatically and nothing else (`--probe-only=` probes
    /// nothing), so a test never probes the machine's real services. Not present in release
    /// builds.
    #[cfg(debug_assertions)]
    #[arg(long, hide = true, value_name = "PID,…", value_parser = parse_pids)]
    pub probe_only: Option<PidList>,

    /// Tests only: stop/force may signal only these processes (matched by PID and the start time
    /// read at startup) and their direct children (`--act-only=` none), so a test can never
    /// signal the machine's real services. Not present in release builds.
    #[cfg(debug_assertions)]
    #[arg(long, hide = true, value_name = "PID,…", value_parser = parse_action_pids)]
    pub act_only: Option<PidList>,
}

/// A comma-separated PID list; empty is allowed.
#[cfg(debug_assertions)]
#[derive(Debug, Clone)]
pub struct PidList(pub Vec<u32>);

#[cfg(debug_assertions)]
fn parse_pids(value: &str) -> Result<PidList, String> {
    value
        .split(',')
        .filter(|part| !part.trim().is_empty())
        .map(|part| {
            part.trim()
                .parse()
                .map_err(|_| format!("{part:?} is not a PID"))
        })
        .collect::<Result<_, _>>()
        .map(PidList)
}

/// Like [`parse_pids`], refusing PID 0 and 1.
#[cfg(debug_assertions)]
fn parse_action_pids(value: &str) -> Result<PidList, String> {
    let pids = parse_pids(value)?;
    match pids.0.iter().find(|&&pid| pid <= 1) {
        Some(pid) => Err(format!("PID {pid} can never be signalled")),
        None => Ok(pids),
    }
}
