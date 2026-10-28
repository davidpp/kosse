// What the dashboard lists, derived from one snapshot: project cards with their service rows,
// the default noise filter, search, ended rows and configured-but-not-observed services.
import type {
  ConfiguredService,
  Observation,
  ProfileSource,
  Project,
  ProjectMetadata,
  Service,
} from "@/generated/contracts";

import { identityKey } from "./format";

/** How long a listener that disappeared stays listed (memory only). */
export const ENDED_RETENTION_MS = 60_000;

/** A service seen in an earlier snapshot and gone from the latest one. */
export type EndedService = { service: Service; project: Project | null; lastSeenMs: number };

export type ServiceRow = {
  kind: "service" | "ended";
  key: string;
  service: Service;
  name: string;
  /** Set when a profile named the service; `null` means the process name is shown. */
  nameSource: ProfileSource | null;
  configured: ConfiguredService | null;
  /** IDs of every configured service that matches this process, when more than one does. */
  ambiguous: string[] | null;
  lastSeenMs: number | null;
};

export type ConfiguredRow = { kind: "configured"; key: string; configured: ConfiguredService };

export type Row = ServiceRow | ConfiguredRow;

export type ProjectGroup = {
  /** The project root, or `null` for listeners outside any project. */
  root: string | null;
  project: Project | null;
  metadata: ProjectMetadata | null;
  title: string;
  titleSource: ProfileSource | null;
  /** `false` for a saved profile whose project has no running process. */
  observed: boolean;
  rows: Row[];
};

export type Dashboard = {
  groups: ProjectGroup[];
  /** Services the noise filter hides (0 when all listeners are shown). */
  hidden: number;
  /** Hidden services that match the search; shown as a hint to widen it. */
  hiddenMatches: number;
  /** Rows the search filtered out of the default view. */
  total: number;
};

/**
 * Hidden by default: listeners outside any project (system daemons, apps, other users'
 * processes whose working directory Kossé cannot read) and any process the scanner reports as
 * another user's. "All listeners" shows them; nothing is ever dropped from the snapshot.
 */
export function isNoise(service: Service): boolean {
  return (
    service.project === null ||
    service.endpoints.some(
      (endpoint) => endpoint.http.state === "skipped" && endpoint.http.reason === "otherUser",
    )
  );
}

/**
 * Carries ended services forward: services in `previous` but not in `next` are added with the
 * previous scan time as last seen; entries older than the retention (by scan clock) or seen
 * again are dropped.
 */
export function trackEnded(
  ended: readonly EndedService[],
  previous: Observation | null,
  next: Observation,
): EndedService[] {
  const current = new Set(next.services.map((service) => identityKey(service.process.identity)));
  const kept = ended.filter(
    (entry) =>
      !current.has(identityKey(entry.service.process.identity)) &&
      next.scannedAtMs - entry.lastSeenMs < ENDED_RETENTION_MS,
  );
  if (previous === null || previous.scannedAtMs >= next.scannedAtMs) return kept;
  const known = new Set(kept.map((entry) => identityKey(entry.service.process.identity)));
  for (const service of previous.services) {
    const key = identityKey(service.process.identity);
    if (current.has(key) || known.has(key)) continue;
    kept.push({
      service,
      project: previous.projects.find((project) => project.root === service.project) ?? null,
      lastSeenMs: previous.scannedAtMs,
    });
  }
  return kept;
}

/** Lower-cased search terms; every term must match a row. */
export function searchTerms(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

export function buildDashboard(
  observation: Observation,
  ended: readonly EndedService[],
  options: { terms: string[]; allListeners: boolean },
): Dashboard {
  const { terms, allListeners } = options;
  const groups = new Map<string, ProjectGroup>();
  const metadataByRoot = new Map(observation.profiles.map((metadata) => [metadata.root, metadata]));
  const projects = new Map(observation.projects.map((project) => [project.root, project]));
  let hidden = 0;
  let hiddenMatches = 0;
  let total = 0;

  const groupFor = (root: string | null, project: Project | null): ProjectGroup => {
    const key = root ?? "";
    let group = groups.get(key);
    if (group === undefined) {
      const metadata = root === null ? null : (metadataByRoot.get(root) ?? null);
      group = {
        root,
        project,
        metadata,
        title:
          metadata?.name ?? project?.name ?? (root === null ? "No project" : lastSegment(root)),
        titleSource: metadata?.name == null ? null : metadata.nameSource,
        observed: project !== null || root === null,
        rows: [],
      };
      groups.set(key, group);
    }
    return group;
  };

  const place = (service: Service, project: Project | null, lastSeenMs: number | null) => {
    const group = groupFor(service.project, project);
    const row = serviceRow(service, group.metadata, lastSeenMs);
    const matches = rowMatches(rowText(row, group), terms);
    if (!allListeners && isNoise(service)) {
      hidden += 1;
      if (terms.length > 0 && matches) hiddenMatches += 1;
      return;
    }
    total += 1;
    if (matches) group.rows.push(row);
  };

  for (const service of observation.services) {
    place(service, service.project === null ? null : (projects.get(service.project) ?? null), null);
  }
  for (const entry of ended) {
    place(
      entry.service,
      entry.service.project === null
        ? null
        : (projects.get(entry.service.project) ?? entry.project),
      entry.lastSeenMs,
    );
  }

  // Configured, not observed: declared services nothing matches right now, in observed projects
  // and in saved profiles whose project is not running.
  for (const metadata of observation.profiles) {
    const project = projects.get(metadata.root) ?? null;
    for (const configured of metadata.services) {
      if (configured.disabled || configured.matched.length > 0) continue;
      const group = groupFor(metadata.root, project);
      const row: ConfiguredRow = {
        kind: "configured",
        key: `configured:${metadata.root}:${configured.id}`,
        configured,
      };
      total += 1;
      if (rowMatches(rowText(row, group), terms)) group.rows.push(row);
    }
  }

  const ordered = [...groups.values()]
    .filter((group) => group.rows.length > 0)
    .toSorted(compareGroups);
  for (const group of ordered) group.rows = group.rows.toSorted(compareRows);
  return { groups: ordered, hidden, hiddenMatches, total };
}

function serviceRow(
  service: Service,
  metadata: ProjectMetadata | null,
  lastSeenMs: number | null,
): ServiceRow {
  const match = service.profile;
  const configured =
    match?.kind === "matched"
      ? (metadata?.services.find((entry) => entry.id === match.service) ?? null)
      : null;
  const name = configured?.name ?? service.process.name ?? "unknown process";
  return {
    kind: lastSeenMs === null ? "service" : "ended",
    key: `${lastSeenMs === null ? "service" : "ended"}:${identityKey(service.process.identity)}`,
    service,
    name,
    nameSource: configured?.name == null ? null : configured.sources.name,
    configured,
    ambiguous: match?.kind === "ambiguous" ? match.services : null,
    lastSeenMs,
  };
}

/** Everything a search term can match for a row: project, package, names, command, ports. */
function rowText(row: Row, group: ProjectGroup): string[] {
  const project = [
    group.title,
    group.project?.name,
    group.project?.git?.repositoryName,
    group.root ?? undefined,
  ];
  const head = group.project?.git?.head;
  if (head?.kind === "branch") project.push(head.name);
  if (row.kind === "configured") {
    const { configured } = row;
    return lower([
      ...project,
      configured.id,
      configured.name ?? undefined,
      configured.match?.cwd,
      ...configured.links.map((link) => link.label),
    ]).concat(configured.match?.port === undefined ? [] : [`:${configured.match.port}`]);
  }
  const { service } = row;
  return lower([
    ...project,
    row.name,
    row.configured?.id,
    service.process.name ?? undefined,
    service.package?.name,
    service.package?.path,
    service.process.command?.args.join(" "),
  ]).concat(service.endpoints.map((endpoint) => `:${endpoint.port}`));
}

function rowMatches(text: string[], terms: string[]): boolean {
  return terms.every((term) => text.some((value) => value.includes(term)));
}

function lower(values: (string | null | undefined)[]): string[] {
  return values.flatMap((value) => (value ? [value.toLowerCase()] : []));
}

function lowestPort(row: Row): number {
  if (row.kind === "configured") return row.configured.match?.port ?? Number.MAX_SAFE_INTEGER;
  return row.service.endpoints[0]?.port ?? Number.MAX_SAFE_INTEGER;
}

const ROW_ORDER = { service: 0, ended: 1, configured: 2 } as const;

function compareRows(a: Row, b: Row): number {
  return (
    ROW_ORDER[a.kind] - ROW_ORDER[b.kind] ||
    lowestPort(a) - lowestPort(b) ||
    a.key.localeCompare(b.key)
  );
}

/** Running projects by name, then saved profiles not running, then listeners outside projects. */
function compareGroups(a: ProjectGroup, b: ProjectGroup): number {
  const rank = (group: ProjectGroup) => (group.root === null ? 2 : group.observed ? 0 : 1);
  return (
    rank(a) - rank(b) ||
    a.title.localeCompare(b.title, undefined, { sensitivity: "base" }) ||
    (a.root ?? "").localeCompare(b.root ?? "")
  );
}

function lastSegment(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  return trimmed.slice(trimmed.lastIndexOf("/") + 1) || path;
}
