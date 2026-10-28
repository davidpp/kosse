// Minimal contract values for unit tests of pure dashboard logic.
import type { Endpoint, HttpProbe, Observation, Service, Snapshot } from "@/generated/contracts";

export function endpoint(port: number, http: HttpProbe = { state: "pending" }): Endpoint {
  return { protocol: "tcp", port, bindAddresses: ["127.0.0.1"], exposure: "loopback", http };
}

export function service(
  pid: number,
  options: Partial<Service> & { name?: string; ports?: number[] } = {},
): Service {
  const { name = "node", ports = [3000 + pid], ...rest } = options;
  return {
    process: {
      identity: { pid, startedAtSecs: 1_700_000_000 + pid },
      name,
      executable: null,
      command: { args: [name, "--dev"], redacted: false, truncated: false },
      cwd: null,
      parentPid: 1,
      userId: 501,
      ageSecs: 60,
      metrics: { cpu: { state: "warmingUp" }, memoryBytes: null },
    },
    endpoints: ports.map((port) => endpoint(port)),
    project: "/work/shop",
    package: null,
    profile: null,
    ...rest,
  };
}

export function observation(services: Service[], scannedAtMs = 10_000): Observation {
  const roots = [
    ...new Set(services.flatMap((entry) => (entry.project === null ? [] : [entry.project]))),
  ];
  return {
    scannedAtMs,
    scanDurationMs: 5,
    services,
    projects: roots.map((root) => ({ root, name: root.split("/").at(-1) ?? root, git: null })),
    warnings: [],
    profiles: [],
  };
}

export function snapshot(revision: number, instance = "a", services: Service[] = []): Snapshot {
  return {
    schemaVersion: 2,
    instance,
    revision,
    observation: observation(services, revision * 3000),
    scanner: {
      intervalMs: 3000,
      lastAttemptAtMs: revision * 3000,
      lastAttemptDurationMs: 5,
      failure: null,
      consecutiveFailures: 0,
      overruns: 0,
    },
  };
}
