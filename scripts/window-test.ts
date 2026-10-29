// Real Rune window + real backend + owned processes. Native events enter through AppKit.
import assert from "node:assert/strict";
import { chmod, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { createWorld } from "./support/world.ts";
import { startKosseApp } from "./support/processes.ts";
import { build, control, screenLocked, writeOpener } from "./app.ts";

type Element = {
  key: string | null;
  role: string;
  text: string | null;
  label: string | null;
  rect: number[];
  opacity: number;
  inert: boolean;
};
const socketDir = `/tmp/kosse-app-${process.getuid?.() ?? 0}`;
const socket = join(socketDir, `test-${process.pid}.sock`);
const scratch = join("/tmp", `kosse-rune-test-${process.pid}`);
const delay = (ms: number) => new Promise((done) => setTimeout(done, ms));
const call = (request: Record<string, unknown>) => control(request, socket);
function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}
async function tree(): Promise<Element[]> {
  const result = await call({ op: "tree" });
  assert(Array.isArray(result.elements), "the real rendered element tree is available");
  return result.elements.map((value: unknown): Element => {
    assert(record(value));
    const { key, role, text, label, rect, opacity, inert } = value;
    assert(key === null || typeof key === "string");
    assert(text === null || typeof text === "string");
    assert(label === null || typeof label === "string");
    assert(typeof role === "string" && typeof opacity === "number" && typeof inert === "boolean");
    assert(
      Array.isArray(rect) && rect.length === 4 && rect.every((n: unknown) => typeof n === "number"),
    );
    return { key, role, text, label, rect, opacity, inert };
  });
}
async function until<T>(
  read: () => Promise<T | undefined>,
  message: string,
  timeout = 15000,
): Promise<T> {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const found = await read().catch(() => undefined);
    if (found !== undefined) return found;
    await delay(100);
  }
  throw new Error(`Timed out: ${message}`);
}
async function element(key: string) {
  return until(async () => (await tree()).find((e) => e.key === key && e.opacity > 0.99), key);
}
async function click(key: string) {
  let previous = "";
  let stable = 0;
  const e = await until(async () => {
    const candidate = await element(key);
    const position = JSON.stringify(candidate.rect);
    stable = position === previous ? stable + 1 : 0;
    previous = position;
    return stable >= 2 ? candidate : undefined;
  }, `${key} settled after layout and scrolling`);
  const [x = 0, y = 0, w = 0, h = 0] = e.rect;
  const status = await call({ op: "status" });
  assert(
    typeof status.height === "number" && y >= 0 && y + h <= status.height,
    `${key} is reachable inside the actual window`,
  );
  await call({ op: "click", x: x + w / 2, y: y + h / 2 });
}
async function textShown(pattern: RegExp) {
  return until(async () => (await tree()).find((e) => pattern.test(e.text ?? "")), pattern.source);
}
async function pressKey(value: string) {
  await call({ op: "key", key: value });
}
async function search(query: string) {
  await pressKey("Escape");
  await pressKey("Escape");
  await pressKey("/");
  await until(
    async () => ((await call({ op: "tree" })).search === "" ? true : undefined),
    "search cleared",
  );
  await call({ op: "text", text: query });
  await until(
    async () => ((await call({ op: "tree" })).search === query ? true : undefined),
    "search updated",
  );
}

async function closeDialog() {
  await pressKey("Escape");
  await until(
    async () => ((await tree()).some((e) => e.role === "dialog") ? undefined : true),
    "dialog closes",
  );
}
async function dialog() {
  const status = await call({ op: "status" });
  let previous = "";
  let stable = 0;
  const e = await until(async () => {
    const elements = await tree();
    const root = elements.find((item) => item.key === "kosse");
    if (!root || root.rect[2] !== status.width || root.rect[3] !== status.height) return undefined;
    const candidate = elements.find((item) => item.role === "dialog" && item.opacity >= 0.99);
    if (!candidate) return undefined;
    const position = JSON.stringify(candidate.rect);
    stable = position === previous ? stable + 1 : 0;
    previous = position;
    return stable >= 2 ? candidate : undefined;
  }, "dialog rendered at current window size");
  const [x = 0, y = 0, w = 0, h = 0] = e.rect;
  assert(typeof status.width === "number" && typeof status.height === "number");
  assert(
    w > 200 &&
      h > 80 &&
      x >= 0 &&
      y >= 0 &&
      x + w <= status.width + 1 &&
      y + h <= status.height + 1,
    "dialog is fully inside the actual viewport",
  );
}

if (screenLocked()) throw new Error("Unlock the screen to test the real Rune window.");
await build();
await mkdir(socketDir, { recursive: true, mode: 0o700 });
await chmod(socketDir, 0o700);
await mkdir(scratch, { recursive: true });
const opened = join(scratch, "opened.log");
const opener = join(scratch, "opener.sh");
await writeFile(opened, "");
await writeOpener(opener);
const logs: string[] = [];
const world = await createWorld({
  start: (options) =>
    startKosseApp({
      ...options,
      env: {
        KOSSE_WINDOW_CONTROL: "1",
        KOSSE_WINDOW_CONTROL_SOCKET: socket,
        KOSSE_TEST_WINDOW: "transparent",
        KOSSE_TEST_OPENER: opener,
        KOSSE_APP_OPENED: opened,
      },
      log: (line) => logs.push(line),
    }),
});
try {
  await element(`inspect-${world.fixtures.web.pid}`);
  assert.equal((await call({ op: "status" })).renderer, "rune");
  assert.equal(
    (await call({ op: "status" })).alpha,
    0.01,
    "test window does not cover the desktop",
  );
  await click("search");
  await call({ op: "text", text: "storefront" });
  await until(
    async () => ((await call({ op: "tree" })).search === "storefront" ? true : undefined),
    "native text input",
  );
  await delay(3500);
  assert.equal((await call({ op: "tree" })).search, "storefront", "SSE refresh preserves search");
  await search(String(world.fixtures.api.ports[0]));
  for (const [width, height] of [
    [1180, 780],
    [640, 440],
    [800, 600],
  ]) {
    await call({ op: "resize", width, height });
    for (const theme of ["light", "dark"]) {
      await call({ op: "theme", theme });
      await until(async () => {
        const root = (await tree()).find((e) => e.key === "kosse");
        return root?.rect[2] === width ? root : undefined;
      }, "resized layout");
      for (const e of await tree()) {
        if (e.key?.startsWith("service-"))
          assert(
            (e.rect[0] ?? -1) >= 0 && (e.rect[0] ?? 0) + (e.rect[2] ?? 0) <= (width ?? 0) + 1,
            `service row fits ${width}`,
          );
      }
      await click(`stop-${world.fixtures.api.pid}`);
      await dialog();
      const ports = world.fixtures.api.ports;
      const confirmation = await textShown(/All affected ports:/);
      assert(
        ports.every((p) => confirmation.text?.includes(String(p))),
        "all process ports appear in confirmation",
      );
      await closeDialog();
      await click(`inspect-${world.fixtures.api.pid}`);
      await element("inspector");
      await textShown(/Working directory/);
      await pressKey("Escape");
      // Escape clears search; restore it through the actual input.
      await search(String(world.fixtures.api.ports[0]));
    }
  }
  await search("server.js");
  await element(`inspect-${world.fixtures.api.pid}`);
  const repositoryProfile = JSON.stringify({
    version: 1,
    services: { docs: { name: "Repository docs", match: { cwd: "apps/docs" }, protocol: "http" } },
  });
  await writeFile(join(world.storefront, "kosse.json"), repositoryProfile);
  await search(String(world.fixtures.docs.ports[0]));
  await textShown(/Repository docs/);
  await call({ op: "resize", width: 1180, height: 1000 });
  await search(String(world.fixtures.web.ports[0]));
  await click(`open-${world.fixtures.web.ports[0]}`);
  await until(
    async () =>
      (await readFile(opened, "utf8")).includes(`localhost:${world.fixtures.web.ports[0]}`)
        ? true
        : undefined,
    "native Open reaches captured system opener",
  );
  await click(`customize-${world.storefront}`);
  await dialog();
  await click("project-name");
  await call({ op: "text", text: "Rune test shop" });
  await click("add-service");
  await click("service-id");
  await call({ op: "text", text: "web" });
  await click("service-name");
  await call({ op: "text", text: "Shop frontend" });
  await click("match-override");
  await click("service-cwd");
  await call({ op: "text", text: "apps/web" });
  await call({ op: "scroll", x: 750, y: 450, delta: -200 });
  await click("service-port");
  await call({ op: "text", text: String(world.fixtures.web.ports[0]) });
  await click("save-profile");
  await until(
    async () => ((await tree()).some((e) => e.role === "dialog") ? undefined : true),
    "profile saved and editor closed",
  );
  await textShown(/Rune test shop/);
  const saved = await readFile(join(world.configDir, "config.json"), "utf8");
  assert(saved.includes("Shop frontend"), "service label saved through native fields");
  await textShown(/Shop frontend/);
  assert(saved.includes("apps/web"), "slash typed into the matching field stays in the editor");
  assert.equal(
    await readFile(join(world.storefront, "kosse.json"), "utf8"),
    repositoryProfile,
    "customization never edits the repository profile",
  );
  assert(
    !saved.includes("Repository docs"),
    "inherited metadata is not copied into central overrides",
  );
  await search("Rune test shop");
  await element(`inspect-${world.fixtures.web.pid}`);
  await click(`customize-${world.storefront}`);
  await click("edit-service-0");
  await call({ op: "resize", width: 640, height: 440 });
  await dialog();
  await textShown(/Repository name: Repository docs/);
  await closeDialog();
  await call({ op: "resize", width: 1180, height: 1000 });
  assert(saved.includes("Rune test shop"), "profile persisted in isolated central configuration");
  await search(String(world.fixtures.stubborn.ports[0]));
  await click(`stop-${world.fixtures.stubborn.pid}`);
  await dialog();
  await click("confirm-stop");
  await textShown(/Still running after graceful/);
  await click("offer-force");
  await dialog();
  await textShown(/All affected ports:/);
  await click("confirm-stop");
  await textShown(/Process exited/);
  await closeDialog();
  await search(String(world.fixtures.api.ports[0]));
  await click(`stop-${world.fixtures.api.pid}`);
  await click("confirm-stop");
  await textShown(/Process exited/);
  await closeDialog();
  process.stdout.write(
    "Rune window: native input, live updates, layouts, inspect, Open, profiles, Stop and explicit Force passed.\n",
  );
} catch (error) {
  process.stderr.write(`${logs.slice(-30).join("\n")}\n`);
  process.stderr.write(
    JSON.stringify(
      (await tree()).filter((e) => e.text || e.role === "dialog"),
      null,
      2,
    ),
  );
  throw error;
} finally {
  await world.dispose();
  await rm(scratch, { recursive: true, force: true });
  await rm(socket, { force: true });
}
