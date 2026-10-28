// Generated from the Rust contract types in crates/core. Do not edit.
// Regenerate with `bun run contracts`.

/**
 * Identifies the Stackscan instance answering on this endpoint.
 */
export type Health = { name: string, version: string, };

export type ProcessField = "name" | "startTime" | "user" | "cwd";

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
 * A PID plus the evidence that it still names the same process. A PID alone can be recycled.
 */
export type ProcessIdentity = { pid: number, 
/**
 * Process start time in Unix seconds; `null` when the OS would not disclose it. Without it
 * the identity is not eligible for destructive actions.
 */
startedAtSecs: number | null, };

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
userId: number | null, };

/**
 * Partial data in a snapshot: something that exists but could not be observed.
 */
export type ScanWarning = { "kind": "otherUsersNotVisible" } | { "kind": "processExited", pid: number, ports: Array<number>, } | { "kind": "metadataUnavailable", pid: number, fields: Array<ProcessField>, };

/**
 * A process and every TCP port it listens on. One process is one action target.
 */
export type Service = { process: ProcessRecord, 
/**
 * Ordered by port; each port appears once.
 */
endpoints: Array<Endpoint>, };

/**
 * One observation of the machine's TCP listeners and the processes that own them.
 */
export type Snapshot = { schemaVersion: number, 
/**
 * Wall-clock time the scan started, in Unix milliseconds.
 */
scannedAtMs: number, scanDurationMs: number, 
/**
 * One entry per listening process, ordered by lowest port then PID.
 */
services: Array<Service>, 
/**
 * What this snapshot could not observe. Empty only when nothing was missed.
 */
warnings: Array<ScanWarning>, };
