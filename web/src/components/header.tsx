import { RotateCw, Search } from "lucide-react";
import type { RefObject } from "react";

import type { Connection } from "@/api/snapshot";
import { formatClock } from "@/model/format";

import { Marker } from "./marker";
import { button } from "./ui";

export type Freshness =
  | { state: "none" }
  | { state: "fresh" }
  | { state: "stale"; scannedAtMs: number };

export function Header({
  query,
  onQuery,
  searchRef,
  allListeners,
  onAllListeners,
  hidden,
  connection,
  freshness,
  refreshing,
  onRefresh,
}: {
  query: string;
  onQuery: (value: string) => void;
  searchRef: RefObject<HTMLInputElement | null>;
  allListeners: boolean;
  onAllListeners: (value: boolean) => void;
  hidden: number;
  connection: Connection;
  freshness: Freshness;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  return (
    <header className="sticky top-0 z-10 border-b border-line bg-surface">
      <div className="mx-auto flex max-w-6xl flex-wrap items-center gap-x-3 gap-y-2 px-4 py-2 sm:h-12 sm:flex-nowrap sm:py-0">
        <div className="flex items-center gap-2">
          <KosseMark />
          <h1>
            <KosseWordmark />
            <span className="sr-only">Kossé</span>
          </h1>
        </div>
        <div className="order-last flex w-full items-center gap-3 sm:order-none sm:ml-4 sm:w-auto sm:flex-1">
          <label className="relative block min-w-0 flex-1 sm:max-w-md">
            <span className="sr-only">Search projects, packages, commands and ports</span>
            <Search
              className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-ink-muted"
              aria-hidden
            />
            <input
              ref={searchRef}
              type="search"
              value={query}
              onInput={(event) => onQuery(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key !== "Escape") return;
                event.preventDefault();
                if (query !== "") onQuery("");
                else event.currentTarget.blur();
              }}
              placeholder="Search, or :port"
              autoComplete="off"
              spellCheck={false}
              className="h-8 w-full rounded-control border border-line-strong bg-canvas pr-8 pl-8 text-ink placeholder:text-ink-muted [&::-webkit-search-cancel-button]:hidden"
            />
            <kbd className="pointer-events-none absolute top-1/2 right-2 hidden -translate-y-1/2 rounded-[4px] border border-line px-1.5 font-mono text-small text-ink-muted sm:block">
              /
            </kbd>
          </label>
          <label className="flex shrink-0 cursor-pointer items-center gap-2 text-ink select-none">
            <input
              type="checkbox"
              checked={allListeners}
              onChange={(event) => onAllListeners(event.currentTarget.checked)}
              className="size-3.5"
            />
            All listeners
            {!allListeners && hidden > 0 ? (
              <span
                className="font-mono text-data text-ink-muted tabular-nums"
                title="Listeners outside a project or owned by another user"
              >
                +{hidden}
              </span>
            ) : null}
          </label>
        </div>
        <div className="ml-auto flex items-center gap-3">
          <LiveStatus connection={connection} freshness={freshness} />
          <button
            type="button"
            className={button}
            onClick={onRefresh}
            aria-disabled={refreshing}
            title="Scan now"
          >
            <RotateCw className={refreshing ? "size-3.5 animate-spin" : "size-3.5"} aria-hidden />
            <span className="sr-only sm:not-sr-only">Refresh</span>
          </button>
        </div>
      </div>
    </header>
  );
}

function LiveStatus({ connection, freshness }: { connection: Connection; freshness: Freshness }) {
  let content: { marker: "stale" | "failed" | null; text: string };
  if (connection.state === "reconnecting") content = { marker: "failed", text: "Reconnecting…" };
  else if (connection.state === "connecting") content = { marker: null, text: "Connecting…" };
  else if (freshness.state === "stale")
    content = { marker: "stale", text: `Stale · ${formatClock(freshness.scannedAtMs)}` };
  else content = { marker: null, text: "Live" };
  return (
    <output className="flex items-center gap-1.5 text-ink-muted" aria-live="polite">
      {content.marker === null ? null : <Marker kind={content.marker} />}
      <span className="whitespace-nowrap">{content.text}</span>
    </output>
  );
}

/** The Relay mark at its 16-point optical size (brand/BRAND.md, brand/tray/tray-16.svg). */
function KosseMark() {
  return (
    <svg viewBox="0 0 16 16" className="size-4 text-accent" aria-hidden>
      <rect x="2" y="2" width="3" height="12" rx="1" fill="currentColor" />
      <path
        d="M12.5 3 7 8 12.5 13"
        fill="none"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** brand/kosse-wordmark.svg (Kibernetyk Mono SemiBold outlines) as a relative path; no font ships. */
function KosseWordmark() {
  return (
    <svg viewBox="11 -182.2 698 183.2" className="h-3.5 w-auto text-ink" aria-hidden>
      <path
        fill="currentColor"
        d="M36.8 0h-24.8v-160h24.8v67.6h32.4l38-37.8v-29.8h24.8v40l-40 40l40 40v40h-24.8v-29.8l-38-37.8h-32.4v67.6zm119.2-30v-60l30-30h60l30 30v60l-30 30h-60l-30-30zm24.8-49.8v32.2v7.4l15.4 15.4h39.6l15.4-15.4v-39.6l-15.4-15.4h-39.6l-15.4 15.4zm119.2 42.2h17.6l12.6 12.8h59.6l5.4-5.4v-12l-5.4-5.4h-69.8l-20-20v-32.4l20-20h80l20 20v17.6h-17.6l-12.6-12.8h-59.6l-5.4 5.4v12l5.4 5.4h69.8l20 20v32.4l-20 20h-80l-20-20v-17.6zm144 0h17.6l12.6 12.8h59.6l5.4-5.4v-12l-5.4-5.4h-69.8l-20-20v-32.4l20-20h80l20 20v17.6h-17.6l-12.6-12.8h-59.6l-5.4 5.4v12l5.4 5.4h69.8l20 20v32.4l-20 20h-80l-20-20v-17.6zm264 0v17.6l-20 20h-80l-20-20v-80l20-20h80l20 20v52.4h-95.2v17.4l5.4 5.4h59.6l12.6-12.8h17.6zm-95.2-52.2v17.4h70.4v-17.4l-5.4-5.4h-59.6l-5.4 5.4zm24-49v-15.8l26.6-26.6h15.8v15.8l-26.6 26.6h-15.8z"
      />
    </svg>
  );
}
