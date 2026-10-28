import type { Exposure, HttpProbe, ProbeSkip } from "@/generated/contracts";

/** Marker shapes (web/DESIGN.md, the Shape Rule). */
export type Marker =
  | "listening"
  | "reachable"
  | "failed"
  | "pending"
  | "unknown"
  | "ended"
  | "configured";

export type ProbeSummary = {
  marker: Marker;
  /** Short text for the row. */
  text: string;
  /** A loopback URL that answered over HTTP: the only automatic Open target. */
  url: string | null;
  /** The user may ask for one probe ("Try HTTP"). */
  canTry: boolean;
};

const SKIPPED: Record<ProbeSkip, { text: string; canTry: boolean }> = {
  notOnLoopback: { text: "not probed: not on loopback", canTry: false },
  otherUser: { text: "not probed: another user's process", canTry: false },
  knownNonHttpPort: { text: "not probed: non-HTTP port", canTry: true },
  knownNonHttpProcess: { text: "not probed: non-HTTP server", canTry: true },
  notAttributed: { text: "not probed: outside a project", canTry: true },
};

/**
 * What an HTTP probe saw, without health claims: any status (404 and 500 included) means the
 * endpoint is reachable over HTTP; a timeout, refusal or TLS failure is shown as that failure.
 */
export function summarizeProbe(http: HttpProbe): ProbeSummary {
  switch (http.state) {
    case "skipped":
      return { marker: "listening", url: null, ...SKIPPED[http.reason] };
    case "pending":
      return { marker: "pending", text: "HTTP not checked yet", url: null, canTry: false };
    case "probed": {
      const asked = http.requested ? " (on request)" : "";
      const { result } = http;
      switch (result.kind) {
        case "response": {
          const scheme = result.tls ? "HTTPS" : "HTTP";
          const redirect = result.location === null ? "" : ` → ${result.location}`;
          const title = result.title === null || result.title === "" ? "" : ` · ${result.title}`;
          return {
            marker: "reachable",
            text: `${scheme} ${result.status}${redirect}${title}${asked}`,
            url: result.url,
            canTry: false,
          };
        }
        case "timedOut":
          return { marker: "failed", text: `HTTP timed out${asked}`, url: null, canTry: true };
        case "refused":
          return { marker: "failed", text: `connection refused${asked}`, url: null, canTry: true };
        case "closed":
          return {
            marker: "failed",
            text: `closed without an HTTP answer${asked}`,
            url: null,
            canTry: true,
          };
        case "tlsFailed":
          return {
            marker: "failed",
            text:
              result.failure === "certificate"
                ? `TLS certificate not trusted${asked}`
                : `TLS handshake failed${asked}`,
            url: null,
            canTry: true,
          };
        case "notHttp":
          return { marker: "listening", text: `not HTTP${asked}`, url: null, canTry: false };
        case "invalidPath":
          return {
            marker: "failed",
            text: "probe path refused by the server's URL rules",
            url: null,
            canTry: false,
          };
      }
    }
  }
}

export const EXPOSURE: Record<Exposure, string> = {
  loopback: "this Mac only",
  interface: "one network interface",
  allInterfaces: "all interfaces",
};
