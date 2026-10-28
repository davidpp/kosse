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
          <h1 className="font-semibold tracking-tight">Kossé</h1>
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

/** The Kossé mark (brand/BRAND.md): one stem, two arms; geometry from brand/tools/mark.mjs. */
function KosseMark() {
  return (
    <svg viewBox="0 0 16 16" className="size-4 text-accent" aria-hidden>
      <path
        fill="currentColor"
        d="M1 1L5 1L5 15L1 15ZM4.9985 8.4977L3.0015 5.5023L9.7515 1.0023L11.7485 3.9977ZM3.019 8.5092L4.981 5.4908L14.981 11.9908L13.019 15.0092Z"
      />
    </svg>
  );
}
