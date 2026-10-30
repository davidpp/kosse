import type { ReactNode } from "react";

import type { Endpoint, Observation, ProfileSource, ScanWarning } from "@/generated/contracts";
import { formatBytes, formatClock, formatCpu, formatDuration, formatHead } from "@/model/format";
import { displayReason } from "@/model/endpoints";
import { EXPOSURE, summarizeProbe } from "@/model/probe";
import type { ServiceRow } from "@/model/services";

import { Dialog } from "./dialog";
import { OutLink } from "./link";
import { Marker } from "./marker";
import { Provenance, Tag } from "./project-card";
import { probeFor, TryHttp, type Tried } from "./try-http";
import { button, mono, quietDanger } from "./ui";

const FIELD_NAMES: Record<string, string> = {
  name: "name",
  startTime: "start time",
  user: "owner",
  command: "command line",
  cwd: "working directory",
  cpu: "CPU",
  memory: "memory",
};

const ATTRIBUTION: Record<string, string> = {
  cwdUnavailable: "its working directory could not be read",
  gitUnavailable: "git is not available (install the Xcode Command Line Tools)",
  gitTimedOut: "git did not answer in time",
  gitFailed: "git failed for this directory",
  deferred: "attribution is deferred to the next scan",
};

export function InspectSheet({
  row,
  observation,
  onClose,
  onStop,
  onTryHttp,
  tried,
}: {
  row: ServiceRow;
  observation: Observation;
  onClose: () => void;
  onStop: () => void;
  onTryHttp: (port: number) => void;
  /** "Try HTTP" state by port, as in the row. */
  tried: (port: number) => Tried | undefined;
}) {
  const { service } = row;
  const { process } = service;
  const project =
    observation.projects.find((candidate) => candidate.root === service.project) ?? null;
  const warnings = observation.warnings.filter(
    (warning) => "pid" in warning && warning.pid === process.identity.pid,
  );
  const ended = row.kind === "ended";
  return (
    <Dialog
      sheet
      title={`${row.name} · PID ${process.identity.pid}`}
      labelledBy="inspect-title"
      onClose={onClose}
    >
      <div className="flex-1 space-y-5 overflow-y-auto px-4 py-4">
        {ended && row.lastSeenMs !== null ? (
          <p className="flex items-center gap-2 text-ink-muted">
            <Marker kind="ended" /> Ended. Last seen at {formatClock(row.lastSeenMs)}; these facts
            are from that scan.
          </p>
        ) : null}
        <Section title="Identity">
          <Fact label="PID">{process.identity.pid}</Fact>
          <Fact label="Started">
            {process.identity.startedAtSecs === null ? (
              unknown("start time not disclosed; this process cannot be stopped from Kossé")
            ) : (
              <span title={`Unix time ${process.identity.startedAtSecs}`}>
                {new Date(process.identity.startedAtSecs * 1000).toLocaleString()}
              </span>
            )}
          </Fact>
          <Fact label="Running for">
            {process.ageSecs === null ? unknown() : formatDuration(process.ageSecs)}
          </Fact>
          <Fact label="Process">{process.name ?? unknown()}</Fact>
          <Fact label="Executable" mono>
            {process.executable ?? unknown()}
          </Fact>
          <Fact label="Parent PID">{process.parentPid ?? unknown()}</Fact>
          <Fact label="Owner (UID)">{process.userId ?? unknown()}</Fact>
          <Fact label="CPU">{formatCpu(process.metrics.cpu)}</Fact>
          <Fact label="Memory">
            {process.metrics.memoryBytes === null
              ? unknown()
              : formatBytes(process.metrics.memoryBytes)}
          </Fact>
        </Section>

        <Section title="Command">
          {process.command === null ? (
            <p>{unknown("the command line was not disclosed")}</p>
          ) : (
            <>
              <pre
                className={`overflow-x-auto rounded-control bg-canvas px-2.5 py-2 break-all whitespace-pre-wrap ${mono}`}
              >
                {process.command.args.join(" ")}
              </pre>
              <p className="mt-1 text-small text-ink-muted">
                {process.command.redacted
                  ? "Secret-looking values are replaced with [redacted]. "
                  : "Redaction found nothing to hide. "}
                {process.command.truncated ? "Long arguments were cut." : ""}
              </p>
            </>
          )}
        </Section>

        <Section title="Endpoints">
          <ul className="space-y-3">
            {service.endpoints.map((endpoint) => (
              <EndpointDetail
                key={endpoint.port}
                endpoint={endpoint}
                ended={ended}
                tried={tried(endpoint.port)}
                onTryHttp={() => onTryHttp(endpoint.port)}
              />
            ))}
          </ul>
        </Section>

        <Section title="Project">
          <Fact label="Working directory" mono>
            {process.cwd ?? unknown()}
          </Fact>
          {project === null ? (
            <p className="text-ink-muted">
              {attributionReason(service.project, warnings, process.cwd)}
            </p>
          ) : (
            <>
              <Fact label="Project root" mono>
                {project.root}
              </Fact>
              <Fact label="Why">
                {project.git === null
                  ? "The nearest package directory above the working directory (not in a Git worktree)."
                  : `The working directory is inside this Git ${project.git.linked ? "linked worktree" : "worktree"}.`}
              </Fact>
              {project.git === null ? null : (
                <>
                  <Fact label="Repository">{project.git.repositoryName}</Fact>
                  <Fact label="Head">{formatHead(project.git.head)}</Fact>
                </>
              )}
              <Fact label="Package">
                {service.package === null
                  ? unknown("no manifest between the working directory and the project root")
                  : `${service.package.path === "" ? "project root" : service.package.path} (${service.package.manifests.join(", ") || "no manifest"})`}
              </Fact>
            </>
          )}
        </Section>

        <Section title="Metadata">
          {row.configured === null ? (
            <p className="text-ink-muted">
              {row.ambiguous === null
                ? "No profile names this process; it is shown as observed."
                : `Several configured services match it (${row.ambiguous.join(", ")}), so none is applied.`}
            </p>
          ) : (
            <>
              <Fact label="Service ID" mono>
                {row.configured.id}
              </Fact>
              <Sourced
                label="Name"
                source={row.configured.sources.name}
                value={row.configured.name}
              />
              <Sourced
                label="Protocol"
                source={row.configured.sources.protocol}
                value={row.configured.protocol}
              />
              <Sourced
                label="Probe path"
                source={row.configured.sources.probePath}
                value={row.configured.probePath}
              />
              <Fact label="Links">
                {row.configured.links.length === 0 ? (
                  "none"
                ) : (
                  <ul className="space-y-1">
                    {row.configured.links.map((link) => (
                      <li key={link.url} className="flex flex-wrap items-center gap-2">
                        <OutLink url={link.url}>{link.label}</OutLink>
                        <Tag>
                          {link.reach === "loopback" ? "declared, local" : "declared, not verified"}
                        </Tag>
                      </li>
                    ))}
                  </ul>
                )}
              </Fact>
            </>
          )}
        </Section>

        {warnings.length === 0 ? null : (
          <Section title="Gaps in this scan">
            <ul className="list-disc space-y-1 pl-4 text-ink-muted">
              {warnings.map((warning) => (
                <li key={JSON.stringify(warning)}>{describeWarning(warning)}</li>
              ))}
            </ul>
          </Section>
        )}
      </div>
      <div className="flex justify-end gap-2 border-t border-hairline px-4 py-3">
        {ended ? null : (
          <button type="button" className={`${quietDanger} mx-0 px-2.5`} onClick={onStop}>
            Stop…
          </button>
        )}
        <button type="button" className={button} data-autofocus onClick={onClose}>
          Close
        </button>
      </div>
    </Dialog>
  );
}

function EndpointDetail({
  endpoint,
  ended,
  tried,
  onTryHttp,
}: {
  endpoint: Endpoint;
  ended: boolean;
  tried: Tried | undefined;
  onTryHttp: () => void;
}) {
  const http = probeFor(endpoint, tried);
  const probe = summarizeProbe(http);
  const details = endpoint.display.show === "details";
  return (
    <li className="rounded-control border border-hairline px-3 py-2" data-endpoint={endpoint.port}>
      <div className="flex flex-wrap items-center gap-2">
        <Marker
          kind={ended ? "ended" : details && probe.marker === "failed" ? "listening" : probe.marker}
        />
        <span className={`${mono} font-medium`}>:{endpoint.port}</span>
        {endpoint.display.name === null ? null : (
          <span className="font-medium">{endpoint.display.name}</span>
        )}
        <Tag>{details ? "details" : "summary"}</Tag>
        <span className="text-ink-muted">{endpoint.protocol.toUpperCase()}</span>
        <span className="text-ink-muted">· reachable from {EXPOSURE[endpoint.exposure]}</span>
      </div>
      <dl className="mt-1.5 grid grid-cols-[7rem_1fr] gap-x-3 gap-y-0.5">
        <dt className="text-ink-muted">Why</dt>
        <dd>
          {details ? "not a launch target · " : ""}
          <span className="text-ink-muted">{displayReason(endpoint.display)}</span>
        </dd>
        <dt className="text-ink-muted">Bound to</dt>
        <dd className={mono}>{endpoint.bindAddresses.join(", ")}</dd>
        <dt className="text-ink-muted">HTTP</dt>
        <dd>
          {probe.text}
          {http.state === "probed" ? (
            <span className="text-ink-muted"> · checked {formatClock(http.probedAtMs)}</span>
          ) : null}
        </dd>
        {http.state === "probed" && http.result.kind === "response" ? (
          <>
            <dt className="text-ink-muted">URL</dt>
            <dd>
              <OutLink url={http.result.url} />
            </dd>
            {http.result.location === null ? null : (
              <>
                <dt className="text-ink-muted">Redirects to</dt>
                <dd className={`break-all ${mono}`}>{http.result.location} (not followed)</dd>
              </>
            )}
            {http.result.bodyTruncated ? (
              <>
                <dt className="text-ink-muted">Body</dt>
                <dd>only the first 64 KiB were read</dd>
              </>
            ) : null}
          </>
        ) : null}
        {http.state === "probed" && http.result.kind === "tlsFailed" ? (
          <>
            <dt className="text-ink-muted">TLS</dt>
            <dd className="break-words">{http.result.reason}</dd>
          </>
        ) : null}
      </dl>
      {probe.canTry && !ended && endpoint.http.state === "skipped" ? (
        <div className="mt-2 flex min-w-0 items-center gap-2">
          <TryHttp tried={tried} onTry={onTryHttp} />
        </div>
      ) : null}
    </li>
  );
}

function attributionReason(
  root: string | null,
  warnings: ScanWarning[],
  cwd: string | null,
): string {
  const failure = warnings.find((warning) => warning.kind === "attributionUnavailable");
  if (failure?.kind === "attributionUnavailable") {
    return `Project unknown: ${ATTRIBUTION[failure.reason] ?? failure.reason}.`;
  }
  if (cwd === null) return "Project unknown. The working directory isn't available.";
  if (root === null)
    return "Not in a project: no Git worktree or package manifest above the working directory (the walk stops at the home directory).";
  return "Project not in this scan.";
}

function describeWarning(warning: ScanWarning): string {
  switch (warning.kind) {
    case "metadataUnavailable":
      return `Not disclosed by macOS: ${warning.fields.map((field) => FIELD_NAMES[field] ?? field).join(", ")}.`;
    case "attributionUnavailable":
      return `Project attribution failed: ${ATTRIBUTION[warning.reason] ?? warning.reason}.`;
    case "processExited":
      return "The process exited while it was being scanned.";
    default:
      return "Partial data in this scan.";
  }
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h3 className="mb-1.5 font-semibold tracking-tight">{title}</h3>
      <div className="space-y-1">{children}</div>
    </section>
  );
}

function Fact({
  label,
  children,
  mono: isMono = false,
}: {
  label: string;
  children: ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="grid grid-cols-[8rem_1fr] gap-x-3">
      <span className="text-ink-muted">{label}</span>
      <span className={`min-w-0 break-words ${isMono ? mono : ""}`}>{children}</span>
    </div>
  );
}

function Sourced({
  label,
  source,
  value,
}: {
  label: string;
  source: ProfileSource | null;
  value: string | null;
}) {
  return (
    <Fact label={label}>
      {value === null ? (
        <span className="text-ink-muted">not configured</span>
      ) : (
        <span className="inline-flex flex-wrap items-center gap-1.5">
          {value}
          {source === null ? null : <Provenance source={source} />}
        </span>
      )}
    </Fact>
  );
}

function unknown(why = "unknown") {
  return (
    <span className="inline-flex items-center gap-1.5 text-ink-muted">
      <Marker kind="unknown" />
      {why}
    </span>
  );
}
