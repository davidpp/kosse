import { defineConfig, devices } from "@playwright/test";

// The browser suite runs against the real `kosse` debug binary (built by `bun run test:e2e`)
// with fixture processes it owns. WebKit is the engine of Kossé's own window (WKWebView);
// Chromium covers the other common browser engine.
export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.ts",
  outputDir: "../target/playwright",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  timeout: 90_000,
  expect: { timeout: 20_000 },
  reporter: [["list"]],
  use: { trace: "retain-on-failure", screenshot: "only-on-failure" },
  projects: [
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
});
