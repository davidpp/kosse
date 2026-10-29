import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";

import { failureOf, probe, refresh } from "@/api/client";
import { useAllListeners } from "@/api/preferences";
import { offerSnapshot, snapshotQuery, useLiveSnapshot } from "@/api/snapshot";
import { CustomizeDialog } from "@/components/customize-dialog";
import { Header, type Freshness } from "@/components/header";
import { InspectSheet } from "@/components/inspect-sheet";
import {
  ConnectionNotice,
  EmptyState,
  LoadingState,
  Notice,
  ScanProblem,
} from "@/components/notices";
import { ProjectCard, type RowActions } from "@/components/project-card";
import { StopDialog } from "@/components/stop-dialog";
import type { Tried } from "@/components/try-http";
import type { Observation, ProbeRequest, ProcessIdentity, Snapshot } from "@/generated/contracts";
import { formatClock, identityKey } from "@/model/format";
import { closeView, navigate, useRoute } from "@/model/route";
import { buildDashboard, searchTerms, type EndedService, type ServiceRow } from "@/model/services";

import { useKeyboard } from "./keyboard";

export function App() {
  const route = useRoute();
  if (route.view === "not-found") return <NotFound />;
  return <Dashboard />;
}

function Dashboard() {
  const client = useQueryClient();
  const route = useRoute();
  const snapshot = useQuery(snapshotQuery);
  const connection = useLiveSnapshot();
  const data = snapshot.data?.snapshot;
  const observation = data?.observation ?? null;
  const ended = snapshot.data?.ended ?? NONE_ENDED;
  const freshness = useFreshness(data);

  const [query, setQuery] = useState("");
  const {
    allListeners,
    choose: setAllListeners,
    ...preference
  } = useAllListeners(
    connection.state === "live" ? `live:${data?.instance ?? ""}` : connection.state,
  );
  const [customizing, setCustomizing] = useState<string | null>(null);
  const [tried, setTried] = useState<ReadonlyMap<string, Tried>>(new Map());
  const searchRef = useRef<HTMLInputElement>(null);
  useKeyboard(searchRef);

  const dashboard = useMemo(
    () =>
      observation === null
        ? null
        : buildDashboard(observation, ended, { terms: searchTerms(query), allListeners }),
    [observation, ended, query, allListeners],
  );

  const scan = useMutation({
    mutationFn: () => refresh(client),
    onSuccess: (next) => offerSnapshot(client, next),
  });

  // Each update also drops entries of processes no longer observed.
  const record = (request: ProbeRequest, state: Tried) =>
    setTried((current) => withTried(current, observation, triedKey(request), state));
  const tryProbe = useMutation({
    mutationFn: (request: ProbeRequest) => probe(client, request),
    onMutate: (request) => record(request, "busy"),
    onSuccess: (result, request) => record(request, result),
    onError: (error, request) => record(request, { error: failureOf(error).message }),
  });
  const tryHttp = (identity: ProcessIdentity, port: number) => tryProbe.mutate({ identity, port });

  const actions: RowActions = {
    inspect: (identity) => navigate({ view: "inspect", identity: identityKey(identity) }),
    stop: (identity) => navigate({ view: "stop", identity: identityKey(identity) }),
    customize: setCustomizing,
    tryHttp,
    tried,
    kossePid: data?.instancePid ?? null,
  };

  const inspected =
    route.view === "inspect" && observation !== null
      ? findRow(route.identity, observation, ended)
      : null;
  const missing =
    observation !== null && route.view === "inspect" && inspected === null ? route.identity : null;

  return (
    <div className="flex min-h-screen flex-col">
      <Header
        query={query}
        onQuery={setQuery}
        searchRef={searchRef}
        allListeners={allListeners}
        onAllListeners={setAllListeners}
        hidden={dashboard?.hidden ?? 0}
        connection={connection}
        freshness={freshness}
        refreshing={scan.isPending}
        onRefresh={() => {
          if (!scan.isPending) scan.mutate();
        }}
      />
      <main className="mx-auto w-full max-w-6xl flex-1 space-y-3 px-4 py-4">
        <ConnectionNotice connection={connection} hasData={data !== undefined} />
        {preference.problem === null ? null : (
          <Notice tone="failed" onDismiss={preference.dismiss}>
            {preference.problem}
          </Notice>
        )}
        {/* A failed refresh is news until a newer scan arrives. */}
        {scan.isError && (observation?.scannedAtMs ?? 0) < scan.submittedAt ? (
          <Notice tone="failed" onDismiss={() => scan.reset()}>
            Refresh failed: {failureOf(scan.error).message}.
          </Notice>
        ) : null}
        {missing === null ? null : (
          <Notice tone="failed" onDismiss={closeView}>
            No running process matches PID {missing.split(".")[0]}
            {missing.endsWith(".unknown")
              ? ""
              : ` started ${formatClock(Number(missing.split(".")[1]) * 1000)}`}
            . It may have exited or been restarted under a new PID.
          </Notice>
        )}
        {data === undefined ? (
          snapshot.isError ? (
            <Notice
              tone="failed"
              action={{ label: "Retry", onClick: () => void snapshot.refetch() }}
            >
              Could not load the scan: {failureOf(snapshot.error).message}.
            </Notice>
          ) : (
            <LoadingState />
          )
        ) : (
          <>
            <ScanProblem snapshot={data} freshness={freshness} />
            {dashboard === null ? null : dashboard.groups.length === 0 ? (
              <EmptyState
                query={query}
                hidden={dashboard.hidden}
                hiddenMatches={dashboard.hiddenMatches}
                onShowAll={() => setAllListeners(true)}
                onClear={() => setQuery("")}
              />
            ) : (
              <>
                {dashboard.groups.map((group) => (
                  <ProjectCard key={group.root ?? ""} group={group} actions={actions} />
                ))}
                {query !== "" && dashboard.hiddenMatches > 0 ? (
                  <p className="text-ink-muted">
                    {dashboard.hiddenMatches} more{" "}
                    {dashboard.hiddenMatches === 1 ? "match is" : "matches are"} hidden outside your
                    projects.{" "}
                    <button
                      type="button"
                      className="text-ink underline underline-offset-3"
                      onClick={() => setAllListeners(true)}
                    >
                      Show all listeners
                    </button>
                  </p>
                ) : null}
              </>
            )}
          </>
        )}
      </main>
      {inspected !== null && observation !== null ? (
        <InspectSheet
          key={identityKey(inspected.service.process.identity)}
          row={inspected}
          observation={observation}
          onClose={closeView}
          onStop={() =>
            navigate({ view: "stop", identity: identityKey(inspected.service.process.identity) })
          }
          onTryHttp={(port) => tryHttp(inspected.service.process.identity, port)}
          tried={(port) =>
            tried.get(triedKey({ identity: inspected.service.process.identity, port }))
          }
        />
      ) : null}
      {route.view === "stop" && data !== undefined ? (
        <StopDialog
          key={route.identity}
          identity={route.identity}
          snapshot={data}
          onClose={closeView}
        />
      ) : null}
      {customizing === null || observation === null ? null : (
        <CustomizeDialog
          root={customizing}
          title={
            dashboard?.groups.find((group) => group.root === customizing)?.title ?? customizing
          }
          metadata={observation.profiles.find((metadata) => metadata.root === customizing) ?? null}
          services={observation.services.filter((service) => service.project === customizing)}
          onClose={() => setCustomizing(null)}
        />
      )}
    </div>
  );
}

const NONE_ENDED: EndedService[] = [];

function withTried(
  current: ReadonlyMap<string, Tried>,
  observation: Observation | null,
  key: string,
  state: Tried,
): ReadonlyMap<string, Tried> {
  const live = new Set(
    observation?.services.map((service) => identityKey(service.process.identity)),
  );
  const next = new Map(
    [...current].filter(([entry]) => live.has(entry.slice(0, entry.lastIndexOf(":")))),
  );
  return next.set(key, state);
}

export function triedKey(request: ProbeRequest): string {
  return `${identityKey(request.identity)}:${request.port}`;
}

function findRow(
  key: string,
  observation: Observation,
  ended: readonly EndedService[],
): ServiceRow | null {
  const dashboard = buildDashboard(observation, ended, { terms: [], allListeners: true });
  for (const group of dashboard.groups) {
    for (const row of group.rows) {
      if (row.kind !== "configured" && identityKey(row.service.process.identity) === key)
        return row;
    }
  }
  return null;
}

/**
 * Current when the scanner reports no failure and the observation is younger than about two
 * intervals (the snapshot contract). One timer flips it at the deadline; nothing ticks.
 */
function useFreshness(snapshot: Snapshot | undefined): Freshness {
  const observation = snapshot?.observation ?? null;
  const interval = snapshot?.scanner.intervalMs ?? null;
  const deadline =
    observation === null || interval === null
      ? null
      : observation.scannedAtMs + 2 * interval + 1000;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (deadline === null) return;
    const timer = setTimeout(() => setNow(Date.now()), Math.max(0, deadline - Date.now()) + 20);
    return () => clearTimeout(timer);
  }, [deadline]);
  if (snapshot === undefined || observation === null) return { state: "none" };
  if (snapshot.scanner.failure !== null || (deadline !== null && now >= deadline)) {
    return { state: "stale", scannedAtMs: observation.scannedAtMs };
  }
  return { state: "fresh" };
}

function NotFound() {
  return (
    <main className="mx-auto max-w-6xl px-4 py-6 text-ink-muted">
      Nothing lives at this address.{" "}
      <a
        href="/"
        className="text-ink underline decoration-line underline-offset-4 hover:decoration-ink"
      >
        Back to the dashboard
      </a>
    </main>
  );
}
