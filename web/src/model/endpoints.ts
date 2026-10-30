// Endpoint display tiers (blueprint/project-metadata.md): the core resolves each endpoint's
// `display`; this mirrors the core's launch policy (`Service::launch_endpoints`) for the views.
import type { Endpoint, EndpointDisplay, HttpProbe, Service } from "@/generated/contracts";

/** What a person opens: Open is offered only on these. */
export function isLaunchTarget(endpoint: Endpoint): boolean {
  return endpoint.display.show === "summary";
}

/** The service's summary endpoints and its details ones, each in port order. */
export function splitEndpoints(service: Service): { summary: Endpoint[]; details: Endpoint[] } {
  return {
    summary: service.endpoints.filter(isLaunchTarget),
    details: service.endpoints.filter((endpoint) => !isLaunchTarget(endpoint)),
  };
}

/** Why an endpoint is shown where it is, for Inspect. */
export function displayReason(display: EndpointDisplay): string {
  const where = display.source === "repository" ? "kosse.json" : "Kossé profile";
  switch (display.basis) {
    case "endpoint":
      return `${where}: endpoints`;
    case "otherEndpoints":
      return `${where}: other endpoints`;
    case "inspectorPort":
      return "inferred: default inspector port";
    case "default":
      return "default: nothing describes this endpoint";
  }
}

/**
 * The summary endpoint drawn first, as the row's open target: of those that answered HTTP
 * (automatically or on request, HTTP or HTTPS alike), a status below 400 with a page title, then
 * a status below 400, then a title, then any response; the lowest port breaks ties. When nothing
 * better answered, the target is a 4xx or 5xx, and its status stays on the line. Order and
 * emphasis only: every answering summary endpoint keeps its own Open, as in the tray.
 */
export function openTarget<T extends { endpoint: Endpoint; http: HttpProbe }>(
  summary: readonly T[],
): T | null {
  let best: T | null = null;
  let bestRank = Number.POSITIVE_INFINITY;
  for (const item of summary) {
    const rank = targetRank(item.http);
    if (
      rank < bestRank ||
      (rank === bestRank && best !== null && item.endpoint.port < best.endpoint.port)
    ) {
      best = item;
      bestRank = rank;
    }
  }
  return best;
}

function targetRank(http: HttpProbe): number {
  if (http.state !== "probed" || http.result.kind !== "response") return Number.POSITIVE_INFINITY;
  const ok = http.result.status < 400;
  const page = http.result.title !== null && http.result.title !== "";
  // A non-error status outranks a title: a titled 404 page never beats an untitled 200.
  if (ok) return page ? 0 : 1;
  return page ? 2 : 3;
}
