// The dashboard against the real Kossé server and fixture processes this file owns. Tests run in
// order and share one world per browser: later tests see what earlier ones stopped.
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

import { isRunning } from "./support/processes.ts";
import { fingerprint, git } from "./support/repos.ts";
import { createWorld, type World } from "./support/world.ts";

test.describe.configure({ mode: "serial" });

let world: World;

test.beforeAll(async () => {
  world = await createWorld();
});

test.afterAll(async () => {
  await world?.dispose();
});

test.beforeEach(async ({ page }) => {
  // Every CSP violation (an inline style or script, a remote asset) fails the test. WebKit's
  // automation (Playwright drives its Web Inspector) adds an empty `<style>` of its own; a
  // violation without a source file is discounted once per such injected element.
  await page.addInitScript(() => {
    const seen: { directive: string; source: string }[] = [];
    let automation = 0;
    Object.defineProperty(window, "cspReport", {
      value: () => seen.filter((item) => item.source !== "" || automation-- <= 0),
    });
    document.addEventListener("securitypolicyviolation", (event) =>
      seen.push({
        directive: `${event.violatedDirective} ${event.blockedURI}`,
        source: event.sourceFile,
      }),
    );
    new MutationObserver((records) => {
      for (const record of records) {
        for (const node of record.addedNodes) {
          if (node.nodeName === "STYLE" && (new Error().stack ?? "").includes("web-inspector://"))
            automation += 1;
        }
      }
    }).observe(document, { childList: true, subtree: true });
  });
});

test.afterEach(async ({ page }) => {
  expect(
    await page.evaluate(() => Reflect.apply(Reflect.get(window, "cspReport"), window, [])),
  ).toEqual([]);
});

function row(page: Page, port: number) {
  return page.locator("li[data-row]", { hasText: `:${port}` });
}

/** A project card by its root: names and branches repeat across real projects on this Mac. */
function card(page: Page, root: string | null) {
  return page.locator(`section[aria-labelledby="project-${root ?? "none"}"]`);
}

async function open(page: Page, path = "/") {
  await page.goto(`${world.server.url}${path}`);
  await expect(page.getByText("Live", { exact: true })).toBeVisible();
}

/** Resolves when Kossé has stored `allListeners`; a rate-limited attempt is retried by the page. */
async function preferenceSaved(page: Page, allListeners: boolean): Promise<void> {
  const response = await page.waitForResponse(
    (candidate) =>
      candidate.url().endsWith("/api/preferences") &&
      candidate.status() !== 429 &&
      candidate.request().method() === "POST" &&
      candidate.request().postDataJSON()?.allListeners === allListeners,
  );
  expect(response.status()).toBe(200);
  expect(await response.json()).toEqual({ allListeners });
}

/** The `instance` of the server's current snapshot. */
async function instance(page: Page): Promise<string> {
  const text = await (await page.request.get(`${world.server.url}/api/snapshot`)).text();
  const id = /"instance":"([^"]+)"/.exec(text)?.[1];
  if (id === undefined) throw new Error("no instance in the snapshot");
  return id;
}

/** `pid.startedAtSecs` of a running fixture, read from the server's own snapshot. */
async function identity(page: Page, pid: number): Promise<string> {
  const response = await page.request.get(`${world.server.url}/api/snapshot`);
  const text = await response.text();
  const started = new RegExp(`"identity":\\{"pid":${pid},"startedAtSecs":(\\d+)\\}`).exec(
    text,
  )?.[1];
  if (started === undefined) throw new Error(`PID ${pid} is not in the snapshot`);
  return `${pid}.${started}`;
}

test("groups fixtures by worktree and reports what HTTP probes saw", async ({ page }) => {
  const { web, api, stubborn, db, gateway } = world.fixtures;
  await open(page);
  await expect(page).toHaveTitle("Kossé");
  const icon = await page.locator('link[rel="icon"][type="image/svg+xml"]').getAttribute("href");
  const served = await page.request.get(`${world.server.url}${icon}`);
  expect(served.headers()["content-type"]).toBe("image/svg+xml");

  // Two checkouts named storefront stay separate: branch, worktree tag and path tell them apart.
  const main = card(page, world.storefront);
  const linked = card(page, world.worktree);
  await expect(main.getByRole("heading", { name: "storefront" })).toBeVisible();
  await expect(linked.getByRole("heading", { name: "storefront" })).toBeVisible();
  await expect(linked.getByText("worktree", { exact: true })).toBeVisible();
  await expect(main.getByText("…/work/storefront")).toBeVisible();
  await expect(linked.getByText("…/worktrees/storefront")).toBeVisible();
  await expect(linked.locator("li[data-row]", { hasText: `:${stubborn.ports[0]}` })).toBeVisible();

  // One process with two ports is one row with two endpoints; a 404 is reachable HTTP.
  const apiRow = row(page, api.ports[0] ?? 0);
  await expect(apiRow).toHaveCount(1);
  await expect(apiRow).toContainText(`:${api.ports[1]}`);
  await expect(apiRow.getByText("HTTP 404").first()).toBeVisible({ timeout: 30_000 });
  await expect(row(page, web.ports[0] ?? 0)).toContainText("HTTP 200 · Fixture & <Title>", {
    timeout: 30_000,
  });

  // A self-signed certificate is a visible TLS failure; a database gets no Open link.
  await expect(row(page, gateway.ports[0] ?? 0)).toContainText("TLS certificate not trusted", {
    timeout: 30_000,
  });
  const dbRow = row(page, db.ports[0] ?? 0);
  await expect(dbRow).toContainText("not probed: non-HTTP server");
  await expect(dbRow.getByRole("link")).toHaveCount(0);

  // "Try HTTP" asks for one probe; the banner fixture speaks first, so it is not HTTP.
  await dbRow.getByRole("button", { name: "Try HTTP" }).click();
  await expect(dbRow).toContainText("not HTTP (on request)");
  await dbRow.getByRole("button", { name: "Inspect" }).click();
  await expect(page.getByRole("dialog")).toContainText("not HTTP (on request)");
  await page.keyboard.press("Escape");
});

test("search narrows by port, project and command; all listeners reveals the rest", async ({
  page,
}) => {
  const { web, stubborn } = world.fixtures;
  await open(page);
  const search = page.getByRole("searchbox");

  await page.keyboard.press("/");
  await expect(search).toBeFocused();
  await page.keyboard.type(`:${web.ports[0]}`);
  await expect(page.locator("li[data-row]")).toHaveCount(1);
  await expect(row(page, web.ports[0] ?? 0)).toBeVisible();

  await search.fill("checkout-redesign");
  await expect(row(page, stubborn.ports[0] ?? 0)).toBeVisible();
  await expect(card(page, world.storefront)).toHaveCount(0);

  await search.fill("api-token");
  await expect(page.locator("li[data-row]")).toHaveCount(1);

  // Kossé's own listener runs outside any project, so it is noise by default.
  await search.fill(`:${world.server.port}`);
  await expect(page.getByText("Nothing matches")).toBeVisible();
  await expect(page.getByText("1 hidden listener matches")).toBeVisible();
  await page.getByRole("button", { name: "Show all listeners" }).click();
  await expect(page.getByRole("checkbox", { name: /All listeners/ })).toBeChecked();
  await expect(card(page, null).getByText(`:${world.server.port}`)).toBeVisible();

  await search.press("Escape");
  await expect(search).toHaveValue("");

  // "Show all listeners" is the saved preference too; later tests start from the default.
  const saved = preferenceSaved(page, false);
  await page.getByRole("checkbox", { name: /All listeners/ }).uncheck();
  await saved;
});

test("Open goes to the probed loopback URL", async ({ page }) => {
  const { web } = world.fixtures;
  await open(page);
  const link = row(page, web.ports[0] ?? 0).getByRole("link", { name: /Open/ });
  await expect(link).toHaveAttribute("href", `http://localhost:${web.ports[0]}/`);
  const [popup] = await Promise.all([page.waitForEvent("popup"), link.click()]);
  await expect(popup).toHaveURL(`http://localhost:${web.ports[0]}/`);
  await popup.close();
});

test("keyboard: rows, Inspect sheet, deep link and Escape", async ({ page }) => {
  const { web } = world.fixtures;
  await open(page);
  await page.keyboard.press("/");
  await page.keyboard.type(`:${web.ports[0]}`);
  await page.keyboard.press("ArrowDown");
  const focused = row(page, web.ports[0] ?? 0);
  await expect(focused).toBeFocused();
  // Visible focus: a 2px accent (Québec blue) outline on the row.
  expect(await focused.evaluate((element) => getComputedStyle(element).outlineStyle)).toBe("solid");
  await page.keyboard.press("Enter");

  const sheet = page.getByRole("dialog");
  await expect(sheet).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`\\?inspect=${web.pid}\\.\\d+$`));
  await expect(sheet).toContainText(`PID ${web.pid}`);
  await expect(sheet).toContainText(join(world.storefront, "apps", "web"));
  await expect(sheet).toContainText("--api-token=[redacted]");
  await expect(sheet).not.toContainText("kosse-fixture-token");
  await expect(sheet).toContainText("The working directory is inside this Git worktree.");
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await expect(page).toHaveURL(`${world.server.url}/`);

  // The same sheet from a deep link, and an honest notice for an identity that is not running.
  await open(page, `/?inspect=${await identity(page, web.pid)}`);
  await expect(page.getByRole("dialog")).toContainText(`PID ${web.pid}`);
  await open(page, "/?inspect=999999.1");
  await expect(page.getByRole("alert")).toContainText("No running process matches PID 999999");
});

test("reduced motion collapses transitions", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open(page);
  const refresh = page.getByRole("button", { name: "Refresh" });
  expect(
    await refresh.evaluate((element) => parseFloat(getComputedStyle(element).transitionDuration)),
  ).toBeLessThan(0.001);
});

test("stop: confirm every port, then the row ends", async ({ page }) => {
  const { web } = world.fixtures;
  await open(page);
  await row(page, web.ports[0] ?? 0)
    .getByRole("button", { name: "Stop" })
    .click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading")).toHaveText(`Stop vite (PID ${web.pid})?`);
  await expect(dialog).toContainText(
    `vite in storefront listens on :${web.ports[0]}. Stop sends SIGTERM; that port is affected.`,
  );
  await expect(page).toHaveURL(new RegExp(`\\?stop=${web.pid}\\.\\d+$`));
  // Focus starts on Cancel, never on the destructive button.
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await dialog.getByRole("button", { name: "Stop process" }).click();
  await expect(dialog).toContainText("The process exited.");
  await expect(dialog).toContainText(`:${web.ports[0]} is free`);
  await web.exited;
  expect(isRunning(web)).toBe(false);
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(dialog).toBeHidden();

  // Ended rows stay for a minute with their last-seen time and no actions.
  const ended = row(page, web.ports[0] ?? 0);
  await expect(ended).toContainText("ended · last seen");
  await expect(ended.getByRole("button", { name: "Stop" })).toHaveCount(0);
  await expect(ended.getByRole("link")).toHaveCount(0);
});

test("a TERM-ignoring process needs a separate, expiring force confirmation", async ({ page }) => {
  const { stubborn } = world.fixtures;
  await open(page);
  await row(page, stubborn.ports[0] ?? 0)
    .getByRole("button", { name: "Stop" })
    .click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Stop process" }).click();
  // While the stop is being observed, nothing in the dialog pretends it can be taken back.
  await expect(dialog.getByRole("button", { name: "Stopping…" })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Cancel" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Still running.", { timeout: 15_000 });
  await expect(dialog).toContainText("Force stop (SIGKILL) is offered until");
  expect(isRunning(stubborn)).toBe(true);
  await dialog.getByRole("button", { name: "Force stop…" }).click();
  await expect(dialog.getByRole("heading")).toHaveText(
    `Force stop next-server (PID ${stubborn.pid})?`,
  );
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await dialog.getByRole("button", { name: "Force stop" }).click();
  await expect(dialog).toContainText("The process exited.");
  await stubborn.exited;
});

test("a supervised process that comes back is reported as respawned", async ({ page }) => {
  const { worker } = world.fixtures;
  await open(page);
  const port = worker.ports[0] ?? 0;
  const before = row(page, port);
  await expect(before).toHaveCount(1);
  await before.getByRole("button", { name: "Stop" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Stop process" }).click();
  await expect(dialog).toContainText("It exited, and a new process took its place.");
  await expect(dialog).toContainText(
    `A process with the same parent (PID ${worker.pid}) listens again`,
  );
  expect(isRunning(worker)).toBe(true);
  await dialog.getByRole("button", { name: "Close" }).last().click();
  // The new process is a new row on the same port; the old identity is an ended row.
  await expect(
    page.locator("li[data-row]", { hasText: `:${port}`, hasNotText: "ended" }),
  ).toHaveCount(1);
  await expect(
    page.locator("li[data-row]", { hasText: "ended · last seen" }).filter({ hasText: `:${port}` }),
  ).toHaveCount(1);
});

test("a refused stop is shown as refused and nothing is signalled", async ({ page }) => {
  const { docs } = world.fixtures;
  // Kossé accepts one stop per second; the previous test's stop may be that recent.
  await page.waitForTimeout(1100);
  await open(page);
  await row(page, docs.ports[0] ?? 0)
    .getByRole("button", { name: "Stop" })
    .click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Stop process" }).click();
  await expect(dialog.getByRole("alert")).toContainText("Kossé will not stop this process.");
  await expect(dialog).toContainText("outside the test scope");
  await expect(dialog).toContainText("Nothing was sent.");
  expect(isRunning(docs)).toBe(true);
});

test("a stop deep link opens the confirmation and never confirms it", async ({ page }) => {
  const { api } = world.fixtures;
  await open(page, `/?stop=${await identity(page, api.pid)}`);
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading")).toHaveText(`Stop node (PID ${api.pid})?`);
  await expect(dialog).toContainText(
    `listens on :${api.ports[0]} and :${api.ports[1]}. Stop sends SIGTERM; both ports are affected.`,
  );
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(isRunning(api)).toBe(true);

  await open(page, `/?stop=${api.pid}.1`);
  await expect(page.getByRole("dialog")).toContainText(`No running process matches PID ${api.pid}`);
});

test("customize a project: saved centrally, kept on reload, repository untouched", async ({
  page,
}) => {
  const { api } = world.fixtures;
  const files = await fingerprint(world.storefront);
  const status = git(world.storefront, "status", "--porcelain");

  await open(page);
  const main = card(page, world.storefront);
  await main.getByRole("button", { name: "Customize" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("Nothing is written to the project");
  await dialog.getByRole("button", { name: new RegExp(`Name :${api.ports[0]}`) }).click();
  await dialog.getByLabel("Name", { exact: true }).fill("Storefront API");
  await dialog.getByLabel("Protocol (hint)").selectOption("http");
  await dialog.getByRole("button", { name: "Add link" }).click();
  await dialog.getByLabel("Link label").fill("Docs");
  await dialog.getByLabel("Link URL").fill("javascript:alert(1)");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog.getByRole("alert")).toContainText("is not an http or https URL");
  await dialog.getByLabel("Link URL").fill("https://api.storefront.test/docs");
  await dialog.getByLabel("Probe path").fill("/health");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog).toBeHidden();

  const apiRow = row(page, api.ports[0] ?? 0);
  await expect(apiRow).toContainText("Storefront API");
  await expect(apiRow.getByText("Kossé profile")).toBeVisible();
  await expect(apiRow.getByRole("link", { name: /Docs/ })).toHaveAttribute(
    "href",
    "https://api.storefront.test/docs",
  );
  await expect(apiRow).toContainText("api.storefront.test");
  await expect(apiRow.getByText("declared, not verified")).toBeVisible();

  await page.reload();
  await expect(row(page, api.ports[0] ?? 0)).toContainText("Storefront API");

  // Written only to Kossé's own config file; the repository and its Git state are unchanged.
  expect(await readFile(join(world.configDir, "config.json"), "utf8")).toContain(
    '"name": "Storefront API"',
  );
  expect(await fingerprint(world.storefront)).toEqual(files);
  expect(git(world.storefront, "status", "--porcelain")).toBe(status);
});

test("All listeners is saved centrally and kept on reload, repository untouched", async ({
  page,
}) => {
  const files = await fingerprint(world.storefront);
  const status = git(world.storefront, "status", "--porcelain");
  const toggle = page.getByRole("checkbox", { name: /All listeners/ });
  const own = card(page, null).getByText(`:${world.server.port}`);

  await open(page);
  await expect(toggle).not.toBeChecked();
  await expect(own).toHaveCount(0);
  let saved = preferenceSaved(page, true);
  await toggle.check();
  await saved;

  await page.reload();
  await expect(page.getByText("Live", { exact: true })).toBeVisible();
  await expect(toggle).toBeChecked();
  await expect(own).toBeVisible();
  expect(await readFile(join(world.configDir, "config.json"), "utf8")).toContain(
    '"allListeners": true',
  );
  expect(await fingerprint(world.storefront)).toEqual(files);
  expect(git(world.storefront, "status", "--porcelain")).toBe(status);

  saved = preferenceSaved(page, false);
  await toggle.uncheck();
  await saved;
  await page.reload();
  await expect(page.getByText("Live", { exact: true })).toBeVisible();
  await expect(toggle).not.toBeChecked();
  await expect(own).toHaveCount(0);

  // A change still waiting for its save slot when the page reloads is saved by the next load.
  await page.waitForTimeout(1100);
  saved = preferenceSaved(page, true);
  await toggle.check();
  await saved;
  saved = preferenceSaved(page, false);
  await toggle.uncheck();
  await page.reload();
  await expect(toggle).not.toBeChecked();
  await saved;
  await page.reload();
  await expect(page.getByText("Live", { exact: true })).toBeVisible();
  await expect(toggle).not.toBeChecked();

  // A file Kossé will not overwrite: the toggle still works for this page, and says so.
  const config = join(world.configDir, "config.json");
  const good = await readFile(config, "utf8");
  const newer = '{"version": 9, "preferences": {"allListeners": true}}';
  await writeFile(config, newer);
  try {
    await page.reload();
    await expect(page.getByText("Saved preferences could not be read")).toBeVisible();
    await expect(toggle).not.toBeChecked();
    await toggle.check();
    await expect(
      page.getByText("“All listeners” applies to this page only; it was not saved"),
    ).toBeVisible();
    await expect(toggle).toBeChecked();
    await expect(own).toBeVisible();
    expect(await readFile(config, "utf8")).toBe(newer);
  } finally {
    await writeFile(config, good);
  }
});

test("details endpoints stay visible and muted, are never opened, and Inspect says why", async ({
  page,
}) => {
  const { site, workerd } = world.fixtures;
  const [sitePort = 0, inspectorPort = 0] = site.ports;
  await open(page);
  const project = card(page, world.marketing);

  // The site row: Open for the summary endpoint only; the inspector on the muted details line.
  const siteRow = project.locator("li[data-row]", { hasText: "Marketing site" });
  await expect(siteRow.getByText("HTTP 200").first()).toBeVisible({ timeout: 30_000 });
  await expect(siteRow).toContainText(`:${sitePort}Site`);
  const siteDetails = siteRow.locator("[data-details]");
  await expect(siteDetails).toContainText(`:${inspectorPort} Inspector`);
  await expect(siteDetails).toContainText("HTTP 200", { timeout: 30_000 });
  await expect(siteRow.getByRole("link")).toHaveCount(1);
  await expect(siteRow.getByRole("link")).toHaveAttribute("href", `http://localhost:${sitePort}/`);

  // workerd: matched by its process name, every (changing) port in details; its 500s are
  // reachable HTTP, not a failure, and there is nothing to open.
  const workerRow = project.locator("li[data-row]", { hasText: "Worker runtime" });
  const workerDetails = workerRow.locator("[data-details]");
  await expect(workerDetails).toContainText(
    workerd.ports.map((port) => `:${port}`).join(" HTTP 500 · "),
    {
      timeout: 30_000,
    },
  );
  await expect(workerDetails).toContainText("HTTP 500", { timeout: 30_000 });
  await expect(workerRow.getByRole("link")).toHaveCount(0);
  await expect(workerRow.locator("svg.text-failed")).toHaveCount(0);

  // Inspect lists every endpoint with its tier and where that came from.
  await siteRow.getByRole("button", { name: "Inspect" }).click();
  const sheet = page.getByRole("dialog");
  const inspector = sheet.locator(`[data-endpoint="${inspectorPort}"]`);
  await expect(inspector).toContainText("details");
  await expect(inspector).toContainText("never a launch target");
  await expect(inspector).toContainText("Kossé profile: endpoints");
  await expect(sheet.locator(`[data-endpoint="${sitePort}"]`)).toContainText("in the summary");
  await page.keyboard.press("Escape");
  await workerRow.getByRole("button", { name: "Inspect" }).click();
  await expect(sheet.locator(`[data-endpoint="${workerd.ports[0]}"]`)).toContainText(
    "Kossé profile: other endpoints",
  );
  await page.keyboard.press("Escape");
});

test("losing the server is visible; a restarted instance is picked up and old confirmations are stale", async ({
  page,
}) => {
  const { api, docs } = world.fixtures;
  await open(page);
  const before = await instance(page);
  await row(page, api.ports[0] ?? 0)
    .getByRole("button", { name: "Stop" })
    .click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("button", { name: "Stop process" })).toBeVisible();

  // Down: the page says so. Back: it picks the new instance up from the stream.
  await world.stopServer();
  await expect(page.getByText("Reconnecting…")).toBeVisible();
  await world.startServer();
  await expect(page.getByText("Live", { exact: true })).toBeVisible({ timeout: 30_000 });
  const after = await instance(page);
  expect(after).not.toBe(before);

  // The open dialog still holds the old instance's confirmation: Kossé refuses it as stale.
  await dialog.getByRole("button", { name: "Stop process" }).click();
  await expect(dialog.getByRole("alert")).toContainText("Nothing was sent.");
  await expect(dialog).toContainText("Kossé restarted after this view was loaded.");
  expect(isRunning(api)).toBe(true);
  await dialog.getByRole("button", { name: "Close" }).last().click();

  // A new confirmation acts on the new instance (refused here only by the test scope).
  await page.waitForTimeout(1100);
  await row(page, docs.ports[0] ?? 0)
    .getByRole("button", { name: "Stop" })
    .click();
  const [request] = await Promise.all([
    page.waitForRequest((candidate) => candidate.url().endsWith("/api/stop")),
    page.getByRole("dialog").getByRole("button", { name: "Stop process" }).click(),
  ]);
  expect(request.postDataJSON()).toMatchObject({ instance: after });
  await expect(page.getByRole("dialog")).toContainText("outside the test scope");
  await page.keyboard.press("Escape");

  // A dropped stream says so; a failed refresh says why; the last data stays on screen.
  await world.stopServer();
  await expect(page.getByRole("alert").filter({ hasText: "Live updates stopped" })).toBeVisible();
  await page.getByRole("button", { name: "Refresh" }).click();
  await expect(
    page.getByRole("alert").filter({ hasText: "Refresh failed: Kossé did not answer" }),
  ).toBeVisible();
  await expect(row(page, api.ports[0] ?? 0)).toBeVisible();

  await world.startServer();
  await expect(page.getByText("Live", { exact: true })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole("alert").filter({ hasText: "Live updates stopped" })).toBeHidden();
  // A newer scan clears the old refresh failure.
  await expect(page.getByRole("alert").filter({ hasText: "Refresh failed" })).toBeHidden();
});
