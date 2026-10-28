// Curated screenshots of the dashboard with fixture data only, through `fixtureView` (no
// listener from the machine running the suite can appear). `bun run screenshots` writes them to
// docs/screenshots/; inside `test:e2e` they go to the test output directory, so the suite never
// rewrites tracked files.
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

import { fixtureView, type FixtureView } from "./support/fixture-view.ts";
import { repoRoot } from "./support/processes.ts";
import { createWorld, type World } from "./support/world.ts";

test.describe.configure({ mode: "serial" });
// One engine is enough for images; WebKit is the dashboard window's engine.
test.skip(({ browserName }) => browserName !== "webkit", "screenshots use WebKit only");

let world: World;
let view: FixtureView;

test.beforeAll(async () => {
  world = await createWorld({ showcase: true });
  view = await fixtureView(world.server.port, world.pids);
});

test.afterAll(async () => {
  await view?.close();
  await world?.dispose();
});

function target(name: string, outputPath: (name: string) => string): string {
  return process.env.KOSSE_SCREENSHOTS === "1"
    ? join(repoRoot, "docs", "screenshots", name)
    : outputPath(name);
}

async function settled(page: Page) {
  await page.goto(`${view.url}/`);
  const { web, gateway } = world.fixtures;
  await expect(page.locator("li[data-row]", { hasText: `:${web.ports[0]}` })).toContainText(
    "HTTP 200",
    { timeout: 30_000 },
  );
  await expect(page.locator("li[data-row]", { hasText: `:${gateway.ports[0]}` })).toContainText(
    "TLS",
    { timeout: 30_000 },
  );
  await expect(page.locator("li[data-row]", { hasText: "Storefront API" })).toBeVisible();
  await expect(
    page.locator("li[data-row]", { hasText: "queue-worker" }).getByText("not HTTP"),
  ).toBeVisible({ timeout: 30_000 });
  // CPU needs two scans of a process; the busy fixture shows a sampled, non-zero value.
  await expect(page.getByText("warming up")).toHaveCount(0, { timeout: 15_000 });
  await expect(
    page.locator("li[data-row]", { hasText: "next-server" }).getByText(/^(?!0\.0%)\d+(\.\d)?%$/),
  ).toBeVisible();
}

test("dashboard", async ({ browser }, testInfo) => {
  // A central profile, so the images show a named service and a declared link.
  const setup = await browser.newPage();
  await setup.goto(`${world.server.url}/`);
  const session: unknown = await (
    await setup.request.get(`${world.server.url}/api/session`)
  ).json();
  const token =
    typeof session === "object" &&
    session !== null &&
    "token" in session &&
    typeof session.token === "string"
      ? session.token
      : "";
  const saved = await setup.request.post(`${world.server.url}/api/profile`, {
    headers: { Origin: world.server.url, "X-Kosse-Token": token },
    data: {
      root: world.storefront,
      profile: {
        version: 1,
        services: {
          api: {
            name: "Storefront API",
            match: { cwd: "apps/api" },
            protocol: "http",
            links: [{ label: "API docs", url: "https://api.storefront.test/docs" }],
          },
        },
      },
    },
  });
  expect(saved.ok()).toBe(true);
  await setup.close();

  const capture = async (
    name: string,
    width: number,
    height: number,
    colorScheme: "light" | "dark",
  ) => {
    const context = await browser.newContext({
      viewport: { width, height },
      colorScheme,
      deviceScaleFactor: 2,
    });
    const page = await context.newPage();
    await settled(page);
    await page.screenshot({ path: target(name, (file) => testInfo.outputPath(file)) });
    await context.close();
  };
  await capture("dashboard-light.png", 1000, 640, "light");
  await capture("dashboard-dark.png", 1000, 640, "dark");
  await capture("dashboard-narrow.png", 390, 760, "light");
});

test("stop dialog and inspect sheet", async ({ browser }, testInfo) => {
  const { api, web } = world.fixtures;
  const context = await browser.newContext({
    viewport: { width: 1000, height: 600 },
    colorScheme: "light",
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  await settled(page);
  await page
    .locator("li[data-row]", { hasText: `:${api.ports[0]}` })
    .getByRole("button", { name: "Stop" })
    .click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.screenshot({ path: target("stop-dialog.png", (file) => testInfo.outputPath(file)) });
  await page.keyboard.press("Escape");
  await context.close();

  const dark = await browser.newContext({
    viewport: { width: 1000, height: 600 },
    colorScheme: "dark",
    deviceScaleFactor: 1,
  });
  const sheet = await dark.newPage();
  await settled(sheet);
  await sheet
    .locator("li[data-row]", { hasText: `:${web.ports[0]}` })
    .getByRole("button", { name: "Inspect" })
    .click();
  await expect(sheet.getByRole("dialog")).toBeVisible();
  await sheet.screenshot({
    path: target("inspect-sheet.png", (file) => testInfo.outputPath(file)),
  });
  await dark.close();
});
