import type { CpuUsage, GitHead, ProcessIdentity } from "@/generated/contracts";

/** Compact elapsed time: `42s`, `7m`, `3h 12m`, `5d 4h`. */
export function formatDuration(totalSecs: number): string {
  const secs = Math.max(0, Math.floor(totalSecs));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return mins % 60 === 0 ? `${hours}h` : `${hours}h ${mins % 60}m`;
  const days = Math.floor(hours / 24);
  return hours % 24 === 0 ? `${days}d` : `${days}d ${hours % 24}h`;
}

export function formatBytes(bytes: number): string {
  const mb = bytes / 1_000_000;
  if (mb < 10) return `${mb.toFixed(1)} MB`;
  if (mb < 1000) return `${Math.round(mb)} MB`;
  return `${(mb / 1000).toFixed(1)} GB`;
}

export function formatCpu(cpu: CpuUsage): string {
  switch (cpu.state) {
    case "sampled":
      return cpu.percent < 10 ? `${cpu.percent.toFixed(1)}%` : `${Math.round(cpu.percent)}%`;
    case "warmingUp":
      return "warming up";
    case "unavailable":
      return "unknown";
  }
}

export function formatClock(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour12: false });
}

export function formatHead(head: GitHead): string {
  switch (head.kind) {
    case "branch":
      return head.name;
    case "detached":
      return `detached at ${head.commit.slice(0, 7)}`;
    case "unknown":
      return "branch unknown";
  }
}

/**
 * A path short enough for a card header: home reads as `~`, and longer paths keep their last
 * two segments, where worktrees and same-name checkouts differ. The full path stays in Inspect.
 */
export function shortPath(path: string): string {
  const home = path.replace(/^\/Users\/[^/]+(?=\/|$)/, "~");
  const segments = home.split("/").filter(Boolean);
  return segments.length <= 3 ? home : `…/${segments.slice(-2).join("/")}`;
}

/** `4821.1730131200`: the form used in deep links (`?inspect=`, `?stop=`). */
export function identityKey(identity: ProcessIdentity): string {
  return `${identity.pid}.${identity.startedAtSecs ?? "unknown"}`;
}

export function sameIdentity(a: ProcessIdentity, b: ProcessIdentity): boolean {
  return a.pid === b.pid && a.startedAtSecs === b.startedAtSecs;
}
