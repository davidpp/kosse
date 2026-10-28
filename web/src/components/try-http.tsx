import type { Endpoint, HttpProbe } from "@/generated/contracts";

import { small } from "./ui";

/** A "Try HTTP" request for one endpoint: running, its result, or why it failed. */
export type Tried = HttpProbe | "busy" | { error: string };

/** What to show for an endpoint: the snapshot's probe, or a requested one it does not have yet. */
export function probeFor(endpoint: Endpoint, tried: Tried | undefined): HttpProbe {
  return typeof tried === "object" && "state" in tried && endpoint.http.state === "skipped"
    ? tried
    : endpoint.http;
}

/** The "Try HTTP" control of a skipped endpoint, shared by the row and the Inspect sheet. */
export function TryHttp({ tried, onTry }: { tried: Tried | undefined; onTry: () => void }) {
  if (tried === "busy") return <span className="shrink-0 text-ink-muted">trying…</span>;
  return (
    <>
      <button type="button" className={small} onClick={onTry}>
        {typeof tried === "object" && "error" in tried ? "Try again" : "Try HTTP"}
      </button>
      {typeof tried === "object" && "error" in tried ? (
        <span className="min-w-0 truncate text-ink-muted" title={tried.error}>
          {tried.error}
        </span>
      ) : null}
    </>
  );
}
