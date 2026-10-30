import { GitBranch, Globe, Settings2 } from "lucide-react";

import type { ConfiguredService, Endpoint, ProcessIdentity } from "@/generated/contracts";
import {
  formatBytes,
  formatClock,
  formatCpu,
  formatDuration,
  formatHead,
  identityKey,
  shortPath,
} from "@/model/format";
import { splitEndpoints } from "@/model/endpoints";
import { summarizeProbe } from "@/model/probe";
import type { ConfiguredRow, ProjectGroup, Row, ServiceRow } from "@/model/services";

import { OutLink } from "./link";
import { Marker } from "./marker";
import { probeFor, TryHttp, type Tried } from "./try-http";
import { mono, quiet, quietDanger, rowButton } from "./ui";

export type RowActions = {
  inspect: (identity: ProcessIdentity) => void;
  stop: (identity: ProcessIdentity) => void;
  customize: (root: string) => void;
  tryHttp: (identity: ProcessIdentity, port: number) => void;
  /** Results of "Try HTTP" requests, by `pid.start:port`; shown while the snapshot still says skipped. */
  tried: ReadonlyMap<string, Tried>;
  /** This Kossé's own PID: its row offers no Stop (Kossé never signals itself). */
  kossePid: number | null;
};

export function ProjectCard({ group, actions }: { group: ProjectGroup; actions: RowActions }) {
  const headingId = `project-${group.root ?? "none"}`;
  const git = group.project?.git ?? null;
  return (
    <section aria-labelledby={headingId} className="rounded-panel border border-line bg-surface">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-hairline px-3 py-2 sm:px-4">
        <h2 id={headingId} className="min-w-0 font-semibold tracking-tight">
          {group.title}
        </h2>
        {group.titleSource === null ? null : <Provenance source={group.titleSource} />}
        {git === null ? null : (
          <span className="flex min-w-0 items-center gap-1 text-ink-muted">
            <GitBranch className="size-3.5 shrink-0" aria-hidden />
            <span className={`truncate ${mono}`}>{formatHead(git.head)}</span>
            {git.linked ? <Tag>worktree</Tag> : null}
            {git.repositoryName !== group.title ? (
              <span className="truncate">of {git.repositoryName}</span>
            ) : null}
          </span>
        )}
        {group.root === null ? (
          <span className="text-ink-muted">Working directory unknown or outside any project</span>
        ) : (
          <span className={`min-w-0 truncate text-ink-muted ${mono}`} title={group.root}>
            {shortPath(group.root)}
          </span>
        )}
        {group.observed ? null : <Tag>not running</Tag>}
        {group.root === null ? null : (
          <button
            type="button"
            className={`${quiet} ${rowButton} ml-auto text-ink-muted`}
            onClick={() => group.root !== null && actions.customize(group.root)}
          >
            <Settings2 className="size-3.5" aria-hidden />
            <span className="sr-only sm:not-sr-only">Customize</span>
          </button>
        )}
      </div>
      <ul>
        {group.rows.map((row) => (
          <RowView key={row.key} row={row} actions={actions} />
        ))}
      </ul>
    </section>
  );
}

function RowView({ row, actions }: { row: Row; actions: RowActions }) {
  return row.kind === "configured" ? (
    <ConfiguredLine row={row} />
  ) : (
    <ServiceLine row={row} actions={actions} />
  );
}

const rowClass =
  "group/row grid grid-cols-1 items-start gap-x-4 gap-y-1.5 border-b border-hairline px-3 py-2.5 last:border-b-0 focus:bg-row focus:outline-2 focus:-outline-offset-2 focus:outline-focus sm:px-4 md:grid-cols-[minmax(9rem,0.8fr)_minmax(14rem,2.2fr)_auto]";

function ServiceLine({ row, actions }: { row: ServiceRow; actions: RowActions }) {
  const { service } = row;
  const identity = service.process.identity;
  const ended = row.kind === "ended";
  const { summary, details } = splitEndpoints(service);
  return (
    <li
      data-row
      tabIndex={-1}
      className={`${rowClass} ${ended ? "text-ink-muted" : ""}`}
      aria-label={`${row.name}, ${service.endpoints.map((endpoint) => `port ${endpoint.port}${endpoint.display.show === "details" ? " (details)" : ""}`).join(", ")}${ended ? ", ended" : ""}`}
    >
      <div className="flex min-w-0 items-start gap-2">
        <div className="min-w-0">
          <div className="flex min-h-5 min-w-0 items-center gap-1.5">
            {ended ? <Marker kind="ended" /> : null}
            <span className={`truncate font-medium ${ended ? "" : "text-ink"}`}>{row.name}</span>
            {identity.pid === actions.kossePid ? (
              <span className="shrink-0 text-small text-ink-muted">this Kossé</span>
            ) : null}
          </div>
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 text-ink-muted">
            {row.nameSource === null ? null : <Provenance source={row.nameSource} />}
            {row.nameSource !== null && service.process.name !== null ? (
              <span className={mono}>{service.process.name}</span>
            ) : null}
            {service.package !== null && service.package.path !== "" ? (
              <span className={`truncate ${mono}`}>{service.package.path}</span>
            ) : null}
            {row.ambiguous === null ? null : <span>matches {row.ambiguous.join(", ")}</span>}
            {ended && row.lastSeenMs !== null ? (
              <span>ended · last seen {formatClock(row.lastSeenMs)}</span>
            ) : null}
          </div>
        </div>
      </div>
      <ul className="min-w-0 space-y-1">
        {summary.map((endpoint) => (
          <EndpointLine
            key={endpoint.port}
            endpoint={endpoint}
            identity={identity}
            ended={ended}
            actions={actions}
          />
        ))}
        {details.length === 0 ? null : (
          <DetailsLine endpoints={details} identity={identity} actions={actions} />
        )}
        {row.configured === null
          ? null
          : row.configured.links.map((link) => <DeclaredLink key={link.url} link={link} />)}
      </ul>
      {/* Wraps at every width: in the md grid this column is `auto`, so a nowrap line here would
          push the row past the window (S11). */}
      <div className="flex min-h-5 flex-wrap items-center justify-between gap-x-4 gap-y-2 md:justify-end">
        {ended ? null : <Metrics row={row} />}
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            className={`${quiet} ${rowButton}`}
            data-inspect
            onClick={() => actions.inspect(identity)}
          >
            Inspect
          </button>
          {ended ? null : identity.pid === actions.kossePid ? (
            <button
              type="button"
              className={`${quietDanger} ${rowButton}`}
              disabled
              title="This is Kossé itself. Quit it from its menu-bar icon instead."
            >
              Stop
              <span className="sr-only">
                {" "}
                (unavailable: this is Kossé itself; quit it from its menu-bar icon)
              </span>
            </button>
          ) : (
            <button
              type="button"
              className={`${quietDanger} ${rowButton}`}
              onClick={() => actions.stop(identity)}
            >
              Stop
            </button>
          )}
        </div>
      </div>
    </li>
  );
}

function Metrics({ row }: { row: ServiceRow }) {
  const { process } = row.service;
  return (
    <dl className="flex items-baseline gap-3 whitespace-nowrap text-ink-muted">
      <Metric
        label="up"
        value={process.ageSecs === null ? "unknown" : formatDuration(process.ageSecs)}
      />
      <Metric label="cpu" value={formatCpu(process.metrics.cpu)} />
      <Metric
        label="mem"
        value={
          process.metrics.memoryBytes === null
            ? "unknown"
            : formatBytes(process.metrics.memoryBytes)
        }
      />
    </dl>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline gap-1">
      <dt className="text-small">{label}</dt>
      <dd className={`${mono} text-ink`}>{value}</dd>
    </div>
  );
}

function EndpointLine({
  endpoint,
  identity,
  ended,
  actions,
}: {
  endpoint: Endpoint;
  identity: ProcessIdentity;
  ended: boolean;
  actions: RowActions;
}) {
  const tried = actions.tried.get(`${identityKey(identity)}:${endpoint.port}`);
  const probe = summarizeProbe(probeFor(endpoint, tried));
  return (
    <li className="flex min-h-5 min-w-0 items-center gap-x-2">
      <Marker kind={ended ? "ended" : probe.marker} />
      <span className={`${mono} shrink-0 font-medium ${ended ? "" : "text-ink"}`}>
        :{endpoint.port}
      </span>
      {endpoint.display.name === null ? null : (
        <span className={`shrink-0 font-medium ${ended ? "" : "text-ink"}`}>
          {endpoint.display.name}
        </span>
      )}
      {endpoint.exposure === "loopback" ? null : (
        <span
          className="inline-flex shrink-0 items-center gap-1 text-ink-muted"
          title={endpoint.bindAddresses.join(", ")}
        >
          <Globe className="size-3.5" aria-hidden />
          {endpoint.exposure === "allInterfaces" ? "all interfaces" : "network interface"}
        </span>
      )}
      <span className="min-w-0 truncate text-ink-muted" title={probe.text}>
        {probe.text}
      </span>
      {probe.url !== null && !ended ? (
        <OutLink url={probe.url} className="shrink-0" withPort={false}>
          Open
        </OutLink>
      ) : null}
      {probe.canTry && !ended && endpoint.http.state === "skipped" ? (
        <TryHttp tried={tried} onTry={() => actions.tryHttp(identity, endpoint.port)} />
      ) : null}
    </li>
  );
}

/**
 * Details endpoints (inspectors, internal ports) on one muted line: visible, never a launch
 * target, and their HTTP errors never mark the service failed. Inspect has each in full.
 */
function DetailsLine({
  endpoints,
  identity,
  actions,
}: {
  endpoints: Endpoint[];
  identity: ProcessIdentity;
  actions: RowActions;
}) {
  return (
    <li className="flex min-h-5 min-w-0 items-center gap-x-2 text-ink-muted" data-details>
      <span className="w-3.5 shrink-0" aria-hidden />
      <span className="shrink-0 text-small">details</span>
      <span className="min-w-0 truncate">
        {endpoints.map((endpoint, index) => {
          const tried = actions.tried.get(`${identityKey(identity)}:${endpoint.port}`);
          const probe = summarizeProbe(probeFor(endpoint, tried));
          return (
            <span key={endpoint.port} title={probe.text}>
              {index === 0 ? null : <span aria-hidden> · </span>}
              <span className={mono}>:{endpoint.port}</span>
              {endpoint.display.name === null ? null : ` ${endpoint.display.name}`}
              {` ${probe.text}`}
            </span>
          );
        })}
      </span>
    </li>
  );
}

function DeclaredLink({ link }: { link: ConfiguredService["links"][number] }) {
  return (
    <li className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5 pl-5.5">
      <OutLink url={link.url}>{link.label}</OutLink>
      <Tag>{link.reach === "loopback" ? "declared, local" : "declared, not verified"}</Tag>
    </li>
  );
}

function ConfiguredLine({ row }: { row: ConfiguredRow }) {
  const { configured } = row;
  return (
    <li data-row tabIndex={-1} className={rowClass}>
      <div className="flex min-w-0 items-start gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            <Marker kind="configured" />
            <span className="truncate font-medium">{configured.name ?? configured.id}</span>
            {configured.sources.name === null ? null : (
              <Provenance source={configured.sources.name} />
            )}
          </div>
          <div className="text-ink-muted">configured, not observed</div>
        </div>
      </div>
      <ul className="min-w-0 space-y-1">
        {configured.match?.port === undefined ? null : (
          <li className="flex items-center gap-2 text-ink-muted">
            <span className={mono}>:{configured.match.port}</span> expected
          </li>
        )}
        {configured.links.map((link) => (
          <DeclaredLink key={link.url} link={link} />
        ))}
      </ul>
      <div />
    </li>
  );
}

export function Provenance({ source }: { source: "central" | "repository" }) {
  return <Tag>{source === "central" ? "Kossé profile" : "kosse.json"}</Tag>;
}

export function Tag({ children }: { children: string }) {
  return (
    <span className="inline-flex h-4.5 shrink-0 items-center rounded-[4px] border border-hairline px-1.5 text-small whitespace-nowrap text-ink-muted">
      {children}
    </span>
  );
}
