// Generated from the Rust contract types in crates/core. Do not edit.
// Regenerate with `bun run contracts`.

/**
 * Identifies the Stackscan instance answering on this endpoint.
 */
export type Health = { name: string, version: string, };

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

export type AttributionFailure = "cwdUnavailable" | "gitUnavailable" | "gitTimedOut" | "gitFailed" | "deferred";

export type ProcessField = "name" | "startTime" | "user" | "cwd" | "cpu" | "memory";

/**
 * Who can reach an endpoint, judged from its bind addresses alone (no firewall knowledge).
 */
export type Exposure = "loopback" | "interface" | "allInterfaces";

export type TransportProtocol = "tcp";

/**
 * One port of a service, with every address it is bound to on that port.
 */
export type Endpoint = { protocol: TransportProtocol, port: number, 
/**
 * Canonical IP text (IPv4-mapped IPv6 shown as IPv4), IPv4 first, each once. Separate
 * IPv4/IPv6 sockets on one port share an endpoint but keep their addresses here.
 */
bindAddresses: Array<string>, exposure: Exposure, };

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
 * A PID plus the evidence that it still names the same process. A PID alone can be recycled.
 */
export type ProcessIdentity = { pid: number, 
/**
 * Process start time in Unix seconds; `null` when the OS would not disclose it. Without it
 * the identity is not eligible for destructive actions.
 */
startedAtSecs: number | null, };

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
name: string | null, executable: string | null, cwd: string | null, parentPid: number | null, 
/**
 * Real user ID of the process owner.
 */
userId: number | null, 
/**
 * Seconds between the process start and the scan; `null` when the start time is unknown.
 */
ageSecs: number | null, metrics: ProcessMetrics, };

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
 * Partial data in a snapshot: something that exists but could not be observed.
 */
export type ScanWarning = { "kind": "otherUsersNotVisible" } | { "kind": "processExited", pid: number, ports: Array<number>, } | { "kind": "metadataUnavailable", pid: number, fields: Array<ProcessField>, } | { "kind": "attributionUnavailable", pid: number, reason: AttributionFailure, };

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
package: PackageLabel | null, };

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
warnings: Array<ScanWarning>, };

/**
 * How the scanner behind a snapshot is doing.
 */
export type ScannerStatus = { 
/**
 * Target time between scans; `null` for a one-shot scan (`stackscan list`).
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
 * What one Stackscan instance currently knows: the last successful observation plus how the
 * scanner has fared since. Views never scan; they read the latest published snapshot.
 *
 * Staleness: the observation is current when `scanner.failure` is `null` and it is younger than
 * about two `scanner.intervalMs`. A failing scanner keeps publishing the last good observation
 * with `failure` set; a scanner that hangs publishes nothing, so readers judge age from
 * `observation.scannedAtMs`.
 */
export type Snapshot = { schemaVersion: number, 
/**
 * Opaque ID of the publishing Stackscan process; it changes on every restart. Order
 * snapshots by `revision` only within one instance: a new `instance` is a restart (revisions
 * start again at 1), not an out-of-order event.
 */
instance: string, 
/**
 * Increases with every publication by this instance (successful or failed attempt), so
 * readers can order snapshots and drop out-of-order ones. A one-shot scan is revision 1.
 */
revision: number, 
/**
 * The last successful observation; `null` only when no scan has succeeded yet.
 */
observation: Observation | null, scanner: ScannerStatus, };
