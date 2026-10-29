// Generated from the Rust contract types in crates/core. Do not edit.
// Regenerate with `bun run contracts`.

/**
 * How the instance runs, so a second launch knows what it can ask of it.
 */
export type InstanceMode = "resident" | "ephemeral" | "headless";

/**
 * Identifies the Kossé instance answering on this endpoint.
 */
export type Health = { name: string, version: string, mode: InstanceMode, };

/**
 * The per-instance action token. Unsafe API requests carry it in the `X-Kosse-Token`
 * header. It is served only to same-origin requests and changes on every restart.
 */
export type Session = { token: string, };

export type ApiErrorCode = "hostNotAllowed" | "originNotAllowed" | "tokenRequired" | "unsupportedMediaType" | "payloadTooLarge" | "invalidPayload" | "rateLimited" | "staleIdentity" | "notProbeable" | "scannerBusy" | "scannerStopped" | "invalidProfile" | "unknownProject" | "configUnavailable" | "configWriteFailed" | "notResident";

/**
 * The body of every API error response.
 */
export type ApiError = { error: ApiErrorCode, 
/**
 * A human-readable explanation; not meant for parsing.
 */
message: string, };

/**
 * A PID plus the evidence that it still names the same process. A PID alone can be recycled.
 */
export type ProcessIdentity = { pid: number, 
/**
 * Process start time in Unix seconds; `null` when the OS would not disclose it. Without it
 * the identity is not eligible for destructive actions.
 */
startedAtSecs: number | null, };

/**
 * `POST /api/probe`: probe one endpoint now ("try HTTP"). The identity must match the current
 * snapshot, and the endpoint must allow requests (see `ProbeSkip`).
 */
export type ProbeRequest = { identity: ProcessIdentity, port: number, };

/**
 * `POST /api/stop` (graceful, `SIGTERM`) and `POST /api/force` (`SIGKILL`, only after a
 * `stillRunning` result for the same identity).
 */
export type ActionRequest = { 
/**
 * `Snapshot.instance` of the snapshot the user confirmed from.
 */
instance: string, identity: ProcessIdentity, 
/**
 * Every port the confirmation showed. It must be exactly the ports this identity listens
 * on in the current snapshot, so nobody stops a process without seeing all it serves.
 */
ports: Array<number>, };

export type Ineligible = "systemProcess" | "kosseItself" | "noStartTime" | "otherUser" | "ownerUnknown" | "unreadable" | "outsideTestScope";

/**
 * A process listening on one of the target's port numbers. `identity.startedAtSecs` is
 * `null` when the OS disclosed nothing about it (it is still listed).
 */
export type PortOwner = { port: number, identity: ProcessIdentity, 
/**
 * Untrusted text, as reported by the OS.
 */
name: string | null, parentPid: number | null, };

export type StaleReason = "instanceChanged" | "exited" | "identityChanged" | "notInSnapshot" | "portsChanged";

/**
 * The outcome of a stop or force request. Only `exited` means the process is gone and its
 * ports are free.
 */
export type ActionResult = { "outcome": "exited", target: ProcessIdentity, ports: Array<number>, shared: Array<PortOwner>, } | { "outcome": "stillRunning", target: ProcessIdentity, ports: Array<number>, forceUntilMs: number | null, } | { "outcome": "respawned", target: ProcessIdentity, parentPid: number, owners: Array<PortOwner>, shared: Array<PortOwner>, } | { "outcome": "endpointTaken", target: ProcessIdentity, owners: Array<PortOwner>, shared: Array<PortOwner>, } | { "outcome": "observationFailed", target: ProcessIdentity, 
/**
 * The process was seen gone; only its ports could not be checked.
 */
processExited: boolean, reason: string, } | { "outcome": "stale", reason: StaleReason, } | { "outcome": "forceNotOffered" } | { "outcome": "refused", reason: Ineligible, } | { "outcome": "permissionDenied", target: ProcessIdentity, } | { "outcome": "signalFailed", target: ProcessIdentity, reason: string, } | { "outcome": "internal", reason: string, };

export type LinkKind = "local" | "external" | "tunnel";

export type DeclaredProtocol = "http" | "https" | "tcp";

/**
 * A link shown with the service, opened only on user action.
 */
export type LinkProfile = { label: string, 
/**
 * `http` or `https` only, without credentials. Loopback and `.localhost` hosts count as
 * local; any other host is shown as declared and never probed.
 */
url: string, 
/**
 * Defaults to `local` for loopback and `.localhost` hosts, `external` otherwise.
 */
kind?: LinkKind, };

/**
 * Narrows the project's processes to this service. Both criteria must hold. A port alone
 * matches only processes of this project: it never attributes another project's process.
 * Unknown criteria are refused rather than ignored, since ignoring one would widen the match.
 */
export type ServiceMatch = { 
/**
 * `/`-separated directory relative to the project root (`.` for the root). The process's
 * working directory must be it or inside it. No `..`; symbolic links must stay inside the
 * project.
 */
cwd?: string, 
/**
 * The process must listen on this port.
 */
port?: number, };

/**
 * One service of a project: usually one dev server, possibly several processes.
 */
export type ServiceProfile = { 
/**
 * Display name, instead of the process name.
 */
name?: string, 
/**
 * Which of the project's processes this service is. Without it the service is only declared
 * (links, name) and never matches a process.
 */
match?: ServiceMatch, 
/**
 * What the service speaks: a hint, never proof of health. Only in the central profile does
 * `http`/`https` make an endpoint an HTTP probe candidate; in a repository file it is shown,
 * never acted on.
 */
protocol?: DeclaredProtocol, 
/**
 * Friendly links. Replaces (never extends) the other source's list.
 */
links?: Array<LinkProfile>, 
/**
 * Origin-relative path requested by the HTTP probe instead of `/`, only on a loopback
 * endpoint of the user's own process where `GET /` already got an HTTP response.
 */
probePath?: string, 
/**
 * `true` suppresses an entry with this ID from the other source (and this one).
 */
disabled?: boolean, };

/**
 * Optional metadata about one project (a worktree root). Nothing in it is executed, fetched or
 * applied to the system: it names services, narrows which processes they are, and declares
 * links and a health path.
 */
export type ProjectProfile = { 
/**
 * Editor metadata only: Kossé never resolves or fetches it.
 */
$schema?: string, 
/**
 * Profile format version; this build understands `1`.
 */
version: number, 
/**
 * Display name for the project, instead of its directory name.
 */
name?: string, 
/**
 * Services by ID. IDs are unique; a central entry overrides the repository entry with the
 * same ID field by field.
 */
services: { [key in string]: ServiceProfile }, };

/**
 * `POST /api/profile` ("Customize this project"): saves (`profile` set) or removes (`null`) the
 * central profile of `root`, which must be a project in the current snapshot or one that already
 * has a central profile. Only Kossé's own configuration is written, never the project. Fields
 * the dashboard does not know, and `$schema`, are kept from the profile being replaced: per
 * service ID, and per link by URL (a link whose URL changes loses them).
 */
export type SaveProfileRequest = { root: string, profile: ProjectProfile | null, };

/**
 * The central profile of `root` as stored; the next snapshot applies it.
 */
export type SavedProfile = { root: string, profile: ProjectProfile | null, };

/**
 * The dashboard's preferences, kept in Kossé's configuration file (never in a project).
 * `GET /api/preferences` returns them (defaults when there is no file yet); `POST
 * /api/preferences` replaces them and returns them as stored.
 */
export type Preferences = { 
/**
 * Show listeners outside a project and other users' processes ("All listeners").
 */
allListeners: boolean, };

/**
 * A dashboard address; the identity routes name a process by PID and start time.
 */
export type WindowRoute = { "kind": "dashboard" } | { "kind": "inspect", pid: number, startedAtSecs: bigint, } | { "kind": "stop", pid: number, startedAtSecs: bigint, };

/**
 * `POST /api/window`: show the resident app's dashboard window at `route` and bring it forward.
 * Only a [`InstanceMode::Resident`] instance accepts it (`409 notResident` otherwise).
 */
export type WindowRequest = { route: WindowRoute, };

export type GitHead = { "kind": "branch", name: string, } | { "kind": "detached", commit: string, } | { "kind": "unknown" };

/**
 * Git facts about a project's worktree, as reported by `git` itself.
 */
export type GitWorktree = { 
/**
 * Canonical path of the repository's common Git directory. Every worktree of one
 * repository shares it; it groups projects for display and never merges them.
 */
commonDir: string, 
/**
 * The repository's name, derived from the common directory.
 */
repositoryName: string, 
/**
 * Added with `git worktree add` (its `.git` file points into the common directory).
 */
linked: boolean, head: GitHead, };

/**
 * What a link's host is, judged from its text alone (nothing is resolved or contacted).
 */
export type LinkReach = "loopback" | "declared";

/**
 * A configured link, validated: `http(s)`, no credentials, host shown separately.
 */
export type EffectiveLink = { label: string, 
/**
 * Scheme and host lowercased. Open only on user action.
 */
url: string, 
/**
 * Shown next to the label so the destination is always visible.
 */
host: string, 
/**
 * The profile's own label for the link (or the default from `reach`). Display only: a
 * repository can call an external host `local`, so trust comes from `reach` and the visible
 * `host`, never from `kind`.
 */
kind: LinkKind, reach: LinkReach, };

/**
 * Where an effective value came from. Precedence: central over repository over inference; a
 * field without a source is not configured and falls back to what was observed.
 */
export type ProfileSource = "central" | "repository";

/**
 * Which source set each field of a [`ConfiguredService`]; `null` when no source set it.
 */
export type ServiceSources = { name: ProfileSource | null, match: ProfileSource | null, protocol: ProfileSource | null, probePath: ProfileSource | null, links: ProfileSource | null, disabled: ProfileSource | null, };

/**
 * One configured service after merging its central and repository entries field by field.
 */
export type ConfiguredService = { id: string, name: string | null, match: ServiceMatch | null, protocol: DeclaredProtocol | null, probePath: string | null, 
/**
 * Empty when none are configured.
 */
links: Array<EffectiveLink>, 
/**
 * Suppressed: it matches nothing and only shows that an entry was turned off.
 */
disabled: boolean, sources: ServiceSources, 
/**
 * Processes this entry matches in the current observation, ordered by PID; each remains its
 * own action target. Empty means configured, not observed: neither running nor stopped. A
 * process matched by several entries is listed by each and is ambiguous on its service.
 */
matched: Array<ProcessIdentity>, };

export type AttributionFailure = "cwdUnavailable" | "gitUnavailable" | "gitTimedOut" | "gitFailed" | "deferred";

export type ProcessField = "name" | "startTime" | "user" | "command" | "cwd" | "cpu" | "memory";

/**
 * Why a profile source is not (or no longer) in use.
 */
export type ProfileProblem = { "kind": "unreadable", reason: string, } | { "kind": "notAFile" } | { "kind": "tooLarge", limitBytes: number, } | { "kind": "invalid", reason: string, } | { "kind": "newerVersion", version: string, };

/**
 * Who can reach an endpoint, judged from its bind addresses alone (no firewall knowledge).
 */
export type Exposure = "loopback" | "interface" | "allInterfaces";

export type TlsFailure = "certificate" | "handshake";

/**
 * The outcome of one bounded `GET /`. Redirects are recorded, never followed.
 */
export type ProbeResult = { "kind": "response", 
/**
 * The `http://` or `https://` loopback URL that answered, for opening the service.
 */
url: string, status: number, 
/**
 * The HTML `<title>`, whitespace-collapsed; untrusted text.
 */
title: string | null, 
/**
 * A redirect's `Location` header, redacted and as sent: untrusted text, not a link.
 */
location: string | null, 
/**
 * The response arrived over TLS whose certificate verified against the keychains' trusted
 * roots (checked offline, in process).
 */
tls: boolean, 
/**
 * Only the first 64 KiB of the body were read.
 */
bodyTruncated: boolean, } | { "kind": "timedOut" } | { "kind": "refused" } | { "kind": "closed" } | { "kind": "tlsFailed", failure: TlsFailure, 
/**
 * The TLS library's description: untrusted text (it can quote the peer's certificate).
 */
reason: string, } | { "kind": "notHttp" } | { "kind": "invalidPath" };

/**
 * Why an endpoint is not probed automatically. Only the current user's processes that run in
 * a project are probed on their own; `otherUser` and `notOnLoopback` are never probed, the rest
 * can be probed on request ("try HTTP").
 */
export type ProbeSkip = "notOnLoopback" | "otherUser" | "knownNonHttpPort" | "knownNonHttpProcess" | "notAttributed";

/**
 * Whether an endpoint was probed over HTTP, and what was seen.
 */
export type HttpProbe = { "state": "skipped", reason: ProbeSkip, } | { "state": "pending" } | { "state": "probed", 
/**
 * When the request started, in Unix milliseconds. Results are reused for a while
 * (probing is slower than scanning), so this can be older than the scan.
 */
probedAtMs: number, 
/**
 * The user asked for this probe (`POST /api/probe`, "try HTTP") on an endpoint that is
 * not probed automatically; `false` for automatic probes.
 */
requested: boolean, result: ProbeResult, };

export type TransportProtocol = "tcp";

/**
 * One port of a service, with every address it is bound to on that port.
 */
export type Endpoint = { protocol: TransportProtocol, port: number, 
/**
 * Canonical IP text (IPv4-mapped IPv6 shown as IPv4), IPv4 first, each once. Separate
 * IPv4/IPv6 sockets on one port share an endpoint but keep their addresses here.
 */
bindAddresses: Array<string>, exposure: Exposure, 
/**
 * Bounded HTTP enrichment. Listening is a fact; this only says what an HTTP request saw.
 */
http: HttpProbe, };

/**
 * Recognised package manifests. Only their presence is checked; contents are never read.
 */
export type Manifest = "package.json" | "deno.json" | "Cargo.toml" | "pyproject.toml" | "go.mod" | "Gemfile" | "composer.json" | "mix.exs" | "pom.xml" | "build.gradle" | "build.gradle.kts" | "Package.swift";

/**
 * A package inside a project, named after its directory.
 */
export type PackageLabel = { 
/**
 * `/`-separated path relative to the project root; empty for the root itself.
 */
path: string, 
/**
 * The package directory's name.
 */
name: string, 
/**
 * Manifest files present in that directory, in a fixed order.
 */
manifests: Array<Manifest>, };

/**
 * A redacted command line.
 */
export type CommandLine = { 
/**
 * Arguments, `argv[0]` first, with secret values replaced by `[redacted]`.
 */
args: Array<string>, 
/**
 * At least one value was hidden.
 */
redacted: boolean, 
/**
 * Arguments or characters beyond the display limits were dropped (after redaction).
 */
truncated: boolean, };

/**
 * CPU use needs two observations of the same process; it is never reported as a made-up zero.
 */
export type CpuUsage = { "state": "warmingUp" } | { "state": "sampled", percent: number, } | { "state": "unavailable" };

/**
 * Resource use of a process at scan time.
 */
export type ProcessMetrics = { cpu: CpuUsage, 
/**
 * Resident memory in bytes; `null` when the OS withheld it.
 */
memoryBytes: number | null, };

/**
 * What was observed about a listening process. `null` means unknown, never a guess.
 */
export type ProcessRecord = { identity: ProcessIdentity, 
/**
 * Short process name as reported by the OS: untrusted text, truncated by the kernel to 31
 * bytes and decoded lossily (U+FFFD for a cut character).
 */
name: string | null, executable: string | null, 
/**
 * The argument vector after central redaction (see `redact`); `null` when the OS withheld
 * it. Untrusted text: display it, never execute it.
 */
command: CommandLine | null, cwd: string | null, parentPid: number | null, 
/**
 * Real user ID of the process owner.
 */
userId: number | null, 
/**
 * Seconds between the process start and the scan; `null` when the start time is unknown.
 */
ageSecs: number | null, metrics: ProcessMetrics, };

/**
 * Which configured service a process is, on its `Service`.
 */
export type ProfileMatch = { "kind": "matched", service: string, } | { "kind": "ambiguous", services: Array<string>, };

/**
 * A worktree (or, outside Git, a package directory) that services run in.
 */
export type Project = { 
/**
 * Canonical absolute path of the Git worktree root, or of the package directory when the
 * service is not in a Git worktree: the project's identity. Separate worktrees of one
 * repository are separate projects, even when they share a name.
 */
root: string, 
/**
 * Display name: the root directory's name. Not unique; `root` is.
 */
name: string, 
/**
 * `null` outside a Git worktree.
 */
git: GitWorktree | null, };

/**
 * The effective metadata of one project root.
 */
export type ProjectMetadata = { 
/**
 * Canonical worktree root: the key of a central profile, the directory of a repository file.
 */
root: string, 
/**
 * The project is in this observation. Otherwise it is known only from a saved central
 * profile (or a repository file seen within the last minute): configured, not observed.
 */
observed: boolean, 
/**
 * Sources in effect, in precedence order, each once.
 */
sources: Array<ProfileSource>, 
/**
 * Display name; `null` keeps the observed directory name.
 */
name: string | null, nameSource: ProfileSource | null, 
/**
 * Ordered by ID.
 */
services: Array<ConfiguredService>, };

/**
 * Partial data in a snapshot: something that exists but could not be observed.
 */
export type ScanWarning = { "kind": "otherUsersNotVisible" } | { "kind": "processExited", pid: number, ports: Array<number>, } | { "kind": "metadataUnavailable", pid: number, fields: Array<ProcessField>, } | { "kind": "attributionUnavailable", pid: number, reason: AttributionFailure, } | { "kind": "profileUnusable", source: ProfileSource, path: string, problem: ProfileProblem, lastGoodRetained: boolean, } | { "kind": "profileMatchOutsideProject", root: string, service: string, } | { "kind": "profileRootNotCanonical", root: string, canonical: string, };

/**
 * A process and every TCP port it listens on. One process is one action target.
 */
export type Service = { process: ProcessRecord, 
/**
 * Ordered by port; each port appears once.
 */
endpoints: Array<Endpoint>, 
/**
 * `Project.root` of the project this process runs in; `null` when its working directory is
 * unknown, unresolvable (see warnings) or not inside a project.
 */
project: string | null, 
/**
 * The nearest package manifest directory at or above the working directory, within the
 * project.
 */
package: PackageLabel | null, 
/**
 * The configured service (see `Observation.profiles`) this process is; `null` when none
 * matches.
 */
profile: ProfileMatch | null, };

/**
 * One successful scan of the machine's TCP listeners, the processes that own them and the
 * projects they run in.
 */
export type Observation = { 
/**
 * Wall-clock time the scan started, in Unix milliseconds.
 */
scannedAtMs: number, scanDurationMs: number, 
/**
 * One entry per listening process, ordered by lowest port then PID.
 */
services: Array<Service>, 
/**
 * Every project a service runs in, ordered by root, each once.
 */
projects: Array<Project>, 
/**
 * What this observation could not see. Empty only when nothing was missed.
 */
warnings: Array<ScanWarning>, 
/**
 * Effective project metadata (central and repository profiles), ordered by root: observed
 * projects that have a profile, and saved profiles whose project is not observed.
 */
profiles: Array<ProjectMetadata>, };

/**
 * How the scanner behind a snapshot is doing.
 */
export type ScannerStatus = { 
/**
 * Target time between scans; `null` for a one-shot scan (`kosse list`).
 */
intervalMs: number | null, 
/**
 * Start of the most recent attempt, successful or not, in Unix milliseconds.
 */
lastAttemptAtMs: number, lastAttemptDurationMs: number, 
/**
 * Why the most recent attempt failed. When set, the observation (if any) is older than
 * `lastAttemptAtMs`: the last good one, kept rather than replaced by an empty result.
 */
failure: string | null, consecutiveFailures: number, 
/**
 * Attempts so far that took longer than `intervalMs`. The next scan then waits a full
 * interval after the overrun instead of running back to back.
 */
overruns: number, };

/**
 * What one Kossé instance currently knows: the last successful observation plus how the
 * scanner has fared since. Views never scan; they read the latest published snapshot.
 *
 * Staleness: the observation is current when `scanner.failure` is `null` and it is younger than
 * about two `scanner.intervalMs`. A failing scanner keeps publishing the last good observation
 * with `failure` set; a scanner that hangs publishes nothing, so readers judge age from
 * `observation.scannedAtMs`.
 */
export type Snapshot = { schemaVersion: number, 
/**
 * Opaque ID of the publishing Kossé process; it changes on every restart. Order
 * snapshots by `revision` only within one instance: a new `instance` is a restart (revisions
 * start again at 1), not an out-of-order event.
 */
instance: string, 
/**
 * PID of the publishing Kossé process, which never signals itself: the dashboard disables
 * Stop on that row. `0` (never a real PID) in a snapshot from before this field existed.
 */
instancePid: number, 
/**
 * Increases with every publication by this instance (successful or failed attempt), so
 * readers can order snapshots and drop out-of-order ones. A one-shot scan is revision 1.
 */
revision: number, 
/**
 * The last successful observation; `null` only when no scan has succeeded yet.
 */
observation: Observation | null, scanner: ScannerStatus, };
