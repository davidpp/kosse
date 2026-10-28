import { CircleAlert, LoaderCircle, RotateCw } from "lucide-react";

import type { Health } from "@/generated/contracts";

/** The subset of a TanStack Query result this view depends on. */
export type HealthQueryState =
  | { status: "pending" }
  | { status: "error"; error: Error; isFetching: boolean }
  | { status: "success"; data: Health };

export function ServerStatus({ query, onRetry }: { query: HealthQueryState; onRetry: () => void }) {
  switch (query.status) {
    case "pending":
      return (
        <output className="flex items-center gap-2 text-ink-muted">
          <LoaderCircle className="size-4 animate-spin" aria-hidden />
          Connecting to the Stackscan server…
        </output>
      );
    case "error":
      return (
        <div
          className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-panel border border-failed/35 bg-failed-wash px-3 py-2.5"
          role="alert"
        >
          <CircleAlert className="size-4 shrink-0 text-failed" aria-hidden />
          <span className="min-w-0 flex-1">Server unreachable: {query.error.message}</span>
          {/* aria-disabled, not disabled: the button keeps keyboard focus while a retry runs. */}
          <button
            type="button"
            className="inline-flex h-7 items-center gap-1.5 rounded-control border border-line-strong bg-canvas px-2.5 font-medium transition-colors duration-150 ease-out not-aria-disabled:hover:bg-surface not-aria-disabled:active:translate-y-px aria-disabled:cursor-default aria-disabled:opacity-60"
            aria-disabled={query.isFetching}
            onClick={() => {
              if (!query.isFetching) onRetry();
            }}
          >
            <RotateCw
              className={query.isFetching ? "size-3.5 animate-spin" : "size-3.5"}
              aria-hidden
            />
            {query.isFetching ? "Retrying…" : "Retry"}
          </button>
        </div>
      );
    case "success":
      return (
        <output className="flex items-center gap-2">
          <span className="size-2 rounded-full bg-reachable" aria-hidden />
          <span>
            Connected to {query.data.name}{" "}
            <span className="font-mono text-data text-ink-muted tabular-nums">
              {query.data.version}
            </span>
          </span>
        </output>
      );
  }
}
