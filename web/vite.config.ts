import tailwindcss from "@tailwindcss/vite";
import { redact } from "@tanstack/redact/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

import { devPorts } from "./dev-ports.ts";

export default defineConfig({
  resolve: { tsconfigPaths: true },
  plugins: [
    // Fast Refresh in development; Redact supports the plugin's refresh transform.
    react(),
    // React APIs on the Redact runtime, in the build and in Vitest alike. Only features that
    // neither the app nor its dependencies use are dropped: the app is client-rendered with
    // `createRoot` (`hydrateRoot` would throw); nothing renders `<Activity>`, `<Suspense>`,
    // `lazy`, `memo`, portals, view transitions, Fragment refs or class components; and nothing
    // passes a ref to a Lucide icon (its `forwardRef` wrapper still renders). Context stays:
    // TanStack Query and Lucide use it.
    redact({
      features: {
        hydration: false,
        activity: false,
        fragmentRefs: false,
        suspense: false,
        lazy: false,
        memo: false,
        portal: false,
        viewTransitions: false,
        classComponents: false,
        forwardRef: false,
      },
    }),
    tailwindcss(),
  ],
  server: {
    host: "127.0.0.1",
    port: devPorts.vite,
    strictPort: true,
    // Same-origin proxy: the browser only ever talks to the Vite authority.
    proxy: {
      "/api": { target: `http://127.0.0.1:${devPorts.backend}` },
    },
  },
  build: {
    outDir: "dist",
    // Third-party license notices for the bundled JS, collected for release packaging.
    license: true,
    emptyOutDir: true,
    // One entry chunk and no dynamic imports, so there is nothing to preload: the polyfill would
    // only cost budget.
    modulePreload: { polyfill: false },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    // Tests must run the runtime that ships (src/runtime.test.tsx checks it). Vitest normally lets
    // Node load dependencies itself, and Node's `react` is real React; inlining sends every
    // dependency through Vite, where the Redact aliases apply as they do in the build.
    server: { deps: { inline: true } },
  },
  environments: {
    client: {
      // Vite's own client defaults, restated because Vitest clears `mainFields`. Without them
      // Testing Library resolves to its CommonJS build, whose `require("react")` skips the aliases.
      resolve: { mainFields: ["browser", "module", "jsnext:main", "jsnext"] },
    },
  },
});
