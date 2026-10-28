import { CircleAlert } from "lucide-react";

import type { Marker as MarkerKind } from "@/model/probe";

/**
 * An observation marker (web/DESIGN.md, the Shape Rule): every state has its own shape, so none
 * depends on hue. Always next to a text label; the marker itself is decorative.
 */
export function Marker({ kind }: { kind: MarkerKind | "stale" }) {
  if (kind === "failed") {
    return <CircleAlert className="size-3.5 shrink-0 text-failed" aria-hidden />;
  }
  return (
    <span className="flex size-3.5 shrink-0 items-center justify-center" aria-hidden>
      {shape(kind)}
    </span>
  );
}

function shape(kind: Exclude<MarkerKind | "stale", "failed">) {
  switch (kind) {
    case "reachable":
      return <span className="size-2 rounded-full bg-reachable" />;
    case "listening":
      return (
        <span className="flex size-2.5 items-center justify-center rounded-full border border-listening">
          <span className="size-1 rounded-full bg-listening" />
        </span>
      );
    case "pending":
      return <span className="size-2.5 rounded-full border border-listening" />;
    case "unknown":
    case "configured":
      return <span className="size-2.5 rounded-full border border-dashed border-unknown" />;
    case "ended":
      return <span className="h-0.5 w-2.5 rounded-full bg-unknown" />;
    case "stale":
      return (
        <span className="flex size-2.5 overflow-hidden rounded-full border border-stale">
          <span className="h-full w-1/2 bg-stale" />
        </span>
      );
  }
}
