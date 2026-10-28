import { queryOptions } from "@tanstack/react-query";

import type { Health } from "@/generated/contracts";

export const healthQuery = queryOptions({
  queryKey: ["health"],
  queryFn: async ({ signal }): Promise<Health> => {
    const response = await fetch("/api/health", { signal });
    if (!response.ok) {
      throw new Error(`Health check failed with HTTP ${response.status}`);
    }
    const body: unknown = await response.json();
    if (!isHealth(body)) {
      throw new Error("Health check returned an unexpected payload");
    }
    return body;
  },
});

// Types are generated from Rust, but a response can still come from a different build (e.g. a stale
// dev backend). Each endpoint keeps a small guard over the fields the UI reads, next to its query.
function isHealth(value: unknown): value is Health {
  return (
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    typeof value.name === "string" &&
    "version" in value &&
    typeof value.version === "string"
  );
}
