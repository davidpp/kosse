import { X } from "lucide-react";
import type { ReactNode } from "react";

import type { Connection } from "@/api/snapshot";
import type { Snapshot } from "@/generated/contracts";
import { formatClock } from "@/model/format";

import type { Freshness } from "./header";
import { Marker } from "./marker";
import { button } from "./ui";

export function Notice({
  tone,
  children,
  action,
  onDismiss,
}: {
  tone: "failed" | "stale";
  children: ReactNode;
  action?: { label: string; onClick: () => void };
  onDismiss?: () => void;
}) {
  return (
    <div
      role={tone === "failed" ? "alert" : "status"}
      className={`flex flex-wrap items-center gap-x-3 gap-y-2 rounded-panel border px-3 py-2 ${
        tone === "failed" ? "border-failed bg-failed-wash" : "border-stale bg-stale-wash"
      }`}
    >
      <Marker kind={tone} />
      <span className="min-w-0 flex-1">{children}</span>
      {action === undefined ? null : (
        <button type="button" className={button} onClick={action.onClick}>
          {action.label}
        </button>
      )}
      {onDismiss === undefined ? null : (
        <button
          type="button"
          className="rounded-control p-1 text-ink-muted hover:text-ink"
          aria-label="Dismiss"
          onClick={onDismiss}
        >
          <X className="size-3.5" aria-hidden />
        </button>
      )}
    </div>
  );
}

/** Live updates lost: the page keeps the last data it had and says how old it is. */
export function ConnectionNotice({
  connection,
  hasData,
}: {
  connection: Connection;
  hasData: boolean;
}) {
  if (connection.state !== "reconnecting") return null;
  return (
    <Notice tone="failed">
      Live updates stopped at {formatClock(connection.sinceMs)}: Kossé is not answering.{" "}
      {hasData ? "The list below is from the last scan received. " : ""}Reconnecting automatically.
    </Notice>
  );
}

/** The scanner is failing (last good data kept) or has not published for too long. */
export function ScanProblem({ snapshot, freshness }: { snapshot: Snapshot; freshness: Freshness }) {
  const { scanner, observation } = snapshot;
  if (observation === null) {
    return (
      <Notice tone="failed">
        {scanner.failure === null
          ? "The first scan has not finished yet."
          : `Kossé could not scan yet: ${scanner.failure}`}
      </Notice>
    );
  }
  if (scanner.failure !== null) {
    return (
      <Notice tone="stale">
        Scanning is failing ({scanner.consecutiveFailures} in a row): {scanner.failure}. Showing the
        last good scan, from {formatClock(observation.scannedAtMs)}.
      </Notice>
    );
  }
  if (freshness.state === "stale") {
    return (
      <Notice tone="stale">
        No new scan since {formatClock(freshness.scannedAtMs)}. The list may be out of date.
      </Notice>
    );
  }
  return null;
}

export function EmptyState({
  query,
  hidden,
  hiddenMatches,
  onShowAll,
  onClear,
}: {
  query: string;
  hidden: number;
  hiddenMatches: number;
  onShowAll: () => void;
  onClear: () => void;
}) {
  const link = "text-ink underline decoration-line underline-offset-3 hover:decoration-ink";
  if (query !== "") {
    return (
      <div className="rounded-panel border border-line bg-surface px-4 py-6 text-center">
        <p>Nothing matches “{query}”.</p>
        <p className="mt-1 text-ink-muted">
          {hiddenMatches > 0 ? (
            <>
              {hiddenMatches} hidden {hiddenMatches === 1 ? "listener matches" : "listeners match"}.{" "}
              <button type="button" className={link} onClick={onShowAll}>
                Show all listeners
              </button>{" "}
              or{" "}
            </>
          ) : null}
          <button type="button" className={link} onClick={onClear}>
            clear the search
          </button>
          .
        </p>
      </div>
    );
  }
  return (
    <div className="rounded-panel border border-line bg-surface px-4 py-6 text-center">
      <p>No services running in your projects.</p>
      <p className="mt-1 text-ink-muted">
        Start a local service from a project directory and it appears here.
        {hidden > 0 ? (
          <>
            {" "}
            {hidden} other {hidden === 1 ? "listener is" : "listeners are"} hidden.{" "}
            <button type="button" className={link} onClick={onShowAll}>
              Show all listeners
            </button>
          </>
        ) : null}
      </p>
    </div>
  );
}

/** Placeholder cards in the shape of the real ones while the first scan loads. */
export function LoadingState() {
  return (
    <div className="space-y-3" aria-busy="true" aria-label="Loading the scan">
      {[
        { id: "first", rows: ["a", "b", "c"] },
        { id: "second", rows: ["a", "b"] },
      ].map((card) => (
        <div key={card.id} className="rounded-panel border border-line bg-surface">
          <div className="border-b border-hairline px-4 py-2.5">
            <div className="h-3.5 w-40 rounded bg-hairline" />
          </div>
          {card.rows.map((row) => (
            <div
              key={row}
              className="flex gap-4 border-b border-hairline px-4 py-3 last:border-b-0"
            >
              <div className="h-3 w-32 rounded bg-hairline" />
              <div className="h-3 w-48 rounded bg-hairline" />
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
