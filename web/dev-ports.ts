// Development-only loopback ports, shared by vite.config.ts and scripts/dev.ts.
// They sit next to the production default (47831) so a running install is never
// shadowed by a development session.
export const devPorts = {
  backend: 47832,
  vite: 47833,
} as const;
