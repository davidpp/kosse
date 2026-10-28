// Server state for the dashboard: one snapshot in the TanStack Query cache, loaded once with
// GET /api/snapshot and then kept current by the SSE stream. A snapshot replaces the cached one
// only when it is newer (`isNewer`), so a slow GET or an out-of-order event never rolls the page
// back. There is no polling. The cache entry also carries the services that ended recently,
// derived from consecutive snapshots as they arrive.
import { queryOptions, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import type { Snapshot } from "@/generated/contracts";
import { trackEnded, type EndedService } from "@/model/services";

import { fetchSnapshot, isSnapshot, sessionQuery } from "./client";

const SNAPSHOT_KEY = ["snapshot"] as const;

/**
 * Within one instance, a higher revision is newer. Another instance (a restart: its revisions
 * start again at 1) is newer when its scanner attempted a scan no earlier, so a late event from
 * a stopped instance cannot replace its successor's snapshot.
 */
export function isNewer(current: Snapshot | undefined, next: Snapshot): boolean {
  if (current === undefined) return true;
  if (next.instance !== current.instance) {
    return next.scanner.lastAttemptAtMs >= current.scanner.lastAttemptAtMs;
  }
  return next.revision > current.revision;
}

export type LiveState = { snapshot: Snapshot; ended: EndedService[] };

/** The state after `next`, or `current` unchanged when `next` is not newer. */
export function advance(current: LiveState | undefined, next: Snapshot): LiveState {
  if (current !== undefined && !isNewer(current.snapshot, next)) return current;
  const previous = current?.snapshot.observation ?? null;
  const ended = current?.ended ?? [];
  return {
    snapshot: next,
    ended: next.observation === null ? ended : trackEnded(ended, previous, next.observation),
  };
}

export const snapshotQuery = queryOptions({
  queryKey: SNAPSHOT_KEY,
  queryFn: async ({ signal, client }): Promise<LiveState> => {
    const fetched = await fetchSnapshot(signal);
    // Read the cache only now: an event may have delivered a newer snapshot during the GET.
    return advance(client.getQueryData<LiveState>(SNAPSHOT_KEY), fetched);
  },
  // The event stream keeps it fresh; refetching on focus would only race it.
  staleTime: Number.POSITIVE_INFINITY,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  retry: 2,
});

/** Writes a snapshot from any source (event, refresh) into the cache if it is newer. */
export function offerSnapshot(client: QueryClient, next: Snapshot): void {
  const current = client.getQueryData<LiveState>(SNAPSHOT_KEY);
  const updated = advance(current, next);
  if (updated === current) return;
  if (current !== undefined && current.snapshot.instance !== next.instance) {
    // A restarted Kossé has a new action token.
    client.removeQueries({ queryKey: sessionQuery.queryKey });
  }
  client.setQueryData(SNAPSHOT_KEY, updated);
}

export type Connection =
  | { state: "connecting" }
  | { state: "live" }
  /** The stream dropped; the next attempt starts at `retryAtMs`. */
  | { state: "reconnecting"; sinceMs: number; retryAtMs: number };

/** Waits before reconnect attempts, in order; the last one repeats. */
const BACKOFF_MS = [500, 1000, 2000, 4000, 8000];

/**
 * Subscribes to GET /api/events for as long as the component is mounted. On a drop it
 * reconnects with backoff; the first event of every connection is the current snapshot, which
 * is how a reconnect (or a restarted server) refreshes the page.
 */
export function useLiveSnapshot(): Connection {
  const client = useQueryClient();
  const [connection, setConnection] = useState<Connection>({ state: "connecting" });

  useEffect(() => {
    let source: EventSource | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    let droppedAtMs = 0;
    let closed = false;

    const connect = () => {
      source = new EventSource("/api/events");
      source.addEventListener("snapshot", (event) => {
        let data: unknown;
        try {
          data = JSON.parse(event.data);
        } catch {
          return;
        }
        if (!isSnapshot(data)) return;
        failures = 0;
        offerSnapshot(client, data);
        setConnection((current) => (current.state === "live" ? current : { state: "live" }));
      });
      source.addEventListener("error", () => {
        // EventSource would retry on its own at a fixed pace; Kossé backs off instead.
        source?.close();
        if (closed) return;
        if (failures === 0) droppedAtMs = Date.now();
        const delay = BACKOFF_MS[Math.min(failures, BACKOFF_MS.length - 1)] ?? 8000;
        failures += 1;
        setConnection({
          state: "reconnecting",
          sinceMs: droppedAtMs,
          retryAtMs: Date.now() + delay,
        });
        timer = setTimeout(connect, delay);
      });
    };
    connect();

    return () => {
      closed = true;
      clearTimeout(timer);
      source?.close();
    };
  }, [client]);

  return connection;
}
