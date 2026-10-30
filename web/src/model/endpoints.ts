// Endpoint display tiers (blueprint/project-metadata.md): the core resolves each endpoint's
// `display`; this mirrors the core's launch policy (`Service::launch_endpoints`) for the views.
import type { Endpoint, EndpointDisplay, Service } from "@/generated/contracts";

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
