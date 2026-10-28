import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

import { devPorts } from "./dev-ports.ts";

export default defineConfig({
  resolve: { tsconfigPaths: true },
  plugins: [
    // The router plugin must run before the React plugin.
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
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
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
