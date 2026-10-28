import { CircleAlert, CircleCheck, LoaderCircle, RotateCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { Health } from "@/generated/contracts";

/** The subset of a TanStack Query result this view depends on. */
export type HealthQueryState =
  | { status: "pending" }
  | { status: "error"; error: Error }
  | { status: "success"; data: Health };

export function ServerStatus({ query, onRetry }: { query: HealthQueryState; onRetry: () => void }) {
  switch (query.status) {
    case "pending":
      return (
        <output className="flex items-center gap-2 text-sm text-muted-foreground">
          <LoaderCircle className="size-4 animate-spin" aria-hidden />
          Connecting to the Stackscan server…
        </output>
      );
    case "error":
      return (
        <div
          className="flex flex-wrap items-center gap-3 rounded-lg border border-destructive/30 p-4 text-sm"
          role="alert"
        >
          <CircleAlert className="size-4 text-destructive" aria-hidden />
          <span>Server unreachable: {query.error.message}</span>
          <Button variant="outline" size="sm" onClick={onRetry}>
            <RotateCw aria-hidden />
            Retry
          </Button>
        </div>
      );
    case "success":
      return (
        <output className="flex items-center gap-2 text-sm">
          <CircleCheck className="size-4 text-emerald-600" aria-hidden />
          Connected to {query.data.name} {query.data.version}
        </output>
      );
  }
}
