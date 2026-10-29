// `bun run test:window`: the dashboard in the REAL Kossé window (WKWebView, the menu-bar app's
// own `kosse window` child), driven through its debug-only control socket with real AppKit
// clicks and key presses. Playwright drives its own WebKit build; this suite exists because the
// shipped window rendered dialogs Playwright saw fine as 2px slivers (docs/evidence/S11.md).
//
// It owns everything it starts: a fixture world (e2e/support/world.ts), the debug app with
// `--probe-only`/`--act-only` scoped to it on a free port, an isolated config, a capturing opener.
// The window is shown but fully transparent and click-through (`KOSSE_TEST_WINDOW=transparent`),
// so the person at the desktop sees and hits nothing beyond the app's menu-bar icon, while
// rendering, hit-testing and events are WebKit's and AppKit's own (an invisible window ignores
// clicks and throttles rendering; an offscreen one is pulled back onto a screen by AppKit).
import { chmod, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import { startKosseApp } from "../e2e/support/processes.ts";
import { createWorld, type World } from "../e2e/support/world.ts";
import { build, control, evaluate, resize, screenLocked, writeOpener } from "./app.ts";

const DEFAULT = "1180x780";
/** A project name no other world on the machine has saved. */
const NAME = `Shop${process.pid}`;
const MIN = "560x420";
const uid = process.getuid?.() ?? 0;
const socketDir = `/tmp/kosse-app-${uid}`;
const socket = join(socketDir, `test-${process.pid}.sock`);

let failures = 0;
let passes = 0;

function check(name: string, ok: boolean, detail?: unknown) {
  if (ok) {
    passes += 1;
    process.stdout.write(`  ok    ${name}\n`);
  } else {
    failures += 1;
    process.stdout.write(
      `  FAIL  ${name}${detail === undefined ? "" : `: ${JSON.stringify(detail)}`}\n`,
    );
  }
}

const delay = (ms: number) => new Promise((done) => setTimeout(done, ms));
const page = (js: string) => evaluate(js, { socket, timeoutMs: 10_000 });

/** Polls a synchronous page expression until it is truthy (hidden windows throttle page timers,
 * so waiting happens out here). */
async function until(js: string, ms = 10_000): Promise<unknown> {
  let last: unknown = null;
  for (let waited = 0; waited < ms; waited += 100) {
    // A page that is still loading may not answer an evaluation yet.
    last = await page(js).catch(() => null);
    if (last) return last;
    await delay(100);
  }
  return last;
}

/** A real click at the element's centre, once that centre holds still: a dialog that just
 * opened (or a scrolled-into-view target) may still be moving, and a native click aimed at its
 * first position can miss on a slower machine. */
async function click(selector: string, text = "") {
  let point = await locate(selector, text);
  for (let waited = 0; waited < 1000; waited += 100) {
    await delay(100);
    const again = await locate(selector, text);
    if (JSON.stringify(again) === JSON.stringify(point)) break;
    point = again;
  }
  if (point === null || typeof point !== "object")
    throw new Error(`nothing to click: ${selector} ${text}`);
  await control({ op: "click", ...point }, socket);
}

async function locate(selector: string, text: string): Promise<unknown> {
  return page(`(() => {
  const all = [...document.querySelectorAll(${JSON.stringify(selector)})];
  const el = ${JSON.stringify(text)} === "" ? all[0] : all.find((e) => e.textContent.trim() === ${JSON.stringify(text)});
  if (!el) return null;
  el.scrollIntoView({ block: "center", inline: "nearest" });
  const r = el.getBoundingClientRect();
  return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
})()`);
}

const key = (name: string) => control({ op: "key", key: name }, socket);

async function type(text: string) {
  for (const char of text) await key(char);
}

/** The open dialog, judged as a person would see it: inside the viewport, a real size, opaque,
 * and the topmost element at its centre. */
const DIALOG = `(() => {
  const d = document.querySelector("dialog[open]");
  if (!d) return { open: false };
  const r = d.getBoundingClientRect();
  let opacity = 1;
  for (let e = d; e; e = e.parentElement) opacity *= Number(getComputedStyle(e).opacity);
  const hit = document.elementFromPoint(r.x + r.width / 2, r.y + Math.min(r.height / 2, 30));
  return {
    open: true,
    heading: d.querySelector("h2")?.textContent ?? null,
    rect: [r.x, r.y, r.width, r.height].map(Math.round),
    inside: r.x >= 0 && r.y >= 0 && r.right <= innerWidth + 0.5 && r.bottom <= innerHeight + 0.5,
    sized: r.width >= 200 && r.height >= 80,
    opacity,
    onTop: hit !== null && d.contains(hit),
    // Only the body scrolls, never the dialog box itself.
    ownScroll: d.scrollHeight > d.clientHeight + 1,
    scroll: [scrollX, scrollY],
  };
})()`;

type DialogState = {
  open: boolean;
  heading?: string;
  inside?: boolean;
  sized?: boolean;
  opacity?: number;
  onTop?: boolean;
  ownScroll?: boolean;
};

async function dialogVisible(name: string, heading: RegExp) {
  const state = (await until(
    `(() => { const s = ${DIALOG}; return s.open && s.rect[3] > 2 && new RegExp(${JSON.stringify(heading.source)}).test(s.heading ?? "") ? s : null; })()`,
    5000,
  )) as DialogState | null;
  const shown = state ?? ((await page(DIALOG)) as DialogState);
  check(
    `${name}: dialog visible`,
    shown.open &&
      heading.test(shown.heading ?? "") &&
      shown.inside === true &&
      shown.sized === true &&
      shown.opacity === 1 &&
      shown.onTop === true &&
      shown.ownScroll === false,
    shown,
  );
}

async function closeDialog(name: string) {
  await key("Escape");
  check(
    `${name}: Escape closes it`,
    (await until(`!document.querySelector("dialog[open]")`, 3000)) === true,
  );
}

async function fits(name: string) {
  const fit = (await page(
    `({ scrollWidth: document.documentElement.scrollWidth, innerWidth, scrollX })`,
  )) as { scrollWidth: number; innerWidth: number; scrollX: number };
  check(
    `${name}: no horizontal scroll`,
    fit.scrollWidth <= fit.innerWidth && fit.scrollX === 0,
    fit,
  );
}

/** This run's fixtures by process name: other worlds (a `bun run app` session) and the
 * machine's own services show in the same dashboard, so rows are picked by their port. */
const firstPort = new Map<string, number>();
const row = (name: string) =>
  `li[data-row][aria-label^="${name}, port ${firstPort.get(name) ?? 0}"]`;

async function setAllListeners(on: boolean) {
  const checked = await page(`document.querySelector('input[type=checkbox]').checked`);
  if (checked !== on) await click("input[type=checkbox]");
  await until(`document.querySelector('input[type=checkbox]').checked === ${on}`);
}

async function sweep(size: string) {
  process.stdout.write(`window ${size}\n`);
  await resize(size, socket);
  await until(`document.querySelector(${JSON.stringify(row("postgres"))}) !== null`);
  await fits(`${size} dashboard`);
  await setAllListeners(true);
  await fits(`${size} all listeners`);

  // Kossé's own row: no Stop, and it says why. It appears with the all-listeners render.
  await until(
    `[...document.querySelectorAll("li[data-row]")].some((e) => e.textContent.includes("this Kossé"))`,
  );
  const own = (await page(`(() => {
    const r = [...document.querySelectorAll("li[data-row]")].find((e) => e.textContent.includes("this Kossé"));
    if (!r) return null;
    const stop = [...r.querySelectorAll("button")].find((b) => b.textContent.startsWith("Stop"));
    return { disabled: stop?.disabled ?? null, title: stop?.title ?? null };
  })()`)) as { disabled: boolean | null; title: string | null } | null;
  check(
    `${size} Kossé's own row: Stop disabled with a reason`,
    own?.disabled === true && /Kossé itself/.test(own.title ?? ""),
    own,
  );
  await setAllListeners(false);

  for (const theme of ["light", "dark"]) {
    await control({ op: "theme", theme }, socket);
    const scheme = await until(`matchMedia("(prefers-color-scheme: ${theme})").matches`, 3000);
    check(`${size} ${theme} appearance applies`, scheme === true);
    await fits(`${size} ${theme}`);
  }
  await control({ op: "theme", theme: "system" }, socket);

  await click(`${row("postgres")} button`, "Stop");
  await dialogVisible(`${size} Stop`, /^Stop postgres \(PID \d+\)\?$/);
  await closeDialog(`${size} Stop`);

  await click(`${row("vite")} button[data-inspect]`);
  await dialogVisible(`${size} Inspect sheet`, /^vite · PID \d+$/);
  await closeDialog(`${size} Inspect sheet`);

  await click(`section:has(${row("vite")}) button`, "Customize");
  await dialogVisible(`${size} Customize`, /^Customize /);
  await closeDialog(`${size} Customize`);

  // Keyboard: `/` focuses search; j/k move between rows; Enter inspects.
  await click("main");
  await key("/");
  check(
    `${size} / focuses search`,
    (await page(`document.activeElement?.type === "search"`)) === true,
  );
  await type(`:${firstPort.get("postgres") ?? 0}`);
  const filtered = await until(`document.querySelectorAll("li[data-row]").length === 1`, 3000);
  check(`${size} search filters to one row`, filtered === true);
  // Clear it as a person would: select the text and delete it.
  await page(`(() => { document.querySelector("input[type=search]").select(); return true })()`);
  await key("Backspace");
  const cleared = await until(`document.querySelectorAll("li[data-row]").length > 1`, 3000);
  check(`${size} clearing search shows every row again`, cleared === true);
}

async function main() {
  if (screenLocked()) {
    throw new Error(
      "the screen is locked: macOS occludes every window, so WebKit treats the page as hidden and the suite cannot judge it; unlock and rerun",
    );
  }
  await build();
  await mkdir(socketDir, { recursive: true, mode: 0o700 });
  await chmod(socketDir, 0o700);
  const scratch = join("/tmp", `kosse-window-test-${process.pid}`);
  await mkdir(scratch, { recursive: true });
  const opened = join(scratch, "opened.log");
  const opener = join(scratch, "opener.sh");
  await writeFile(opened, "");
  await writeOpener(opener);
  const log: string[] = [];
  const world: World = await createWorld({
    start: (options) =>
      startKosseApp({
        ...options,
        env: {
          KOSSE_WINDOW_CONTROL: "1",
          KOSSE_WINDOW_CONTROL_SOCKET: socket,
          KOSSE_TEST_OPENER: opener,
          KOSSE_APP_OPENED: opened,
          KOSSE_TEST_WINDOW: "transparent",
        },
        log: (line) => log.push(line),
      }),
  });
  for (const fixture of Object.values(world.fixtures)) {
    if (fixture.ports[0] !== undefined) firstPort.set(fixture.name, fixture.ports[0]);
  }
  try {
    // The window process starts after the app listens; its socket appears when it is ready.
    for (let waited = 0; ; waited += 200) {
      const ready = await control({ op: "status" }, socket).then(
        () => true,
        () => false,
      );
      if (ready) break;
      if (waited > 30_000) throw new Error("the window's control socket never answered");
      await delay(200);
    }
    if ((await until(`document.readyState === "complete"`, 60_000)) !== true) {
      throw new Error("the dashboard never loaded in the window");
    }
    const placed = await control({ op: "status" }, socket);
    check(
      "the test window is shown but transparent and click-through",
      placed.visible === true &&
        (placed.alpha as number) < 0.02 &&
        (await until(`document.visibilityState === "visible"`, 5000)) === true,
      placed,
    );
    for (const size of [DEFAULT, MIN]) await sweep(size);
    // In between and wide: the row grid changes columns at 768 px, where rows overflowed (S11).
    await setAllListeners(true);
    for (const size of ["780x600", "800x600", "900x600", "1024x700", "1600x1000"]) {
      await resize(size, socket);
      await until(`innerWidth === ${size.split("x")[0]}`, 3000);
      await fits(`${size} all listeners`);
    }
    await setAllListeners(false);
    await flows(world, opened);
  } finally {
    await world.dispose();
    await rm(scratch, { recursive: true, force: true });
    await rm(socket, { force: true });
  }
  process.stdout.write(`\n${passes} passed, ${failures} failed\n`);
  if (failures > 0) {
    process.stdout.write(`app output:\n${log.slice(-40).join("\n")}\n`);
    process.exitCode = 1;
  }
}

/** Flows that change state, once, at the default size. */
async function flows(world: World, opened: string) {
  process.stdout.write(`flows ${DEFAULT}\n`);
  await resize(DEFAULT, socket);
  const snapshot = (await (await fetch(`${world.server.url}/api/snapshot`)).json()) as {
    observation: { services: { process: { identity: { pid: number; startedAtSecs: number } } }[] };
  };
  const identityOf = (pid: number) =>
    snapshot.observation.services.find((service) => service.process.identity.pid === pid)?.process
      .identity;

  // Open: the link goes to the (captured) browser, never a window navigation.
  await until(
    `[...document.querySelectorAll(${JSON.stringify(`${row("vite")} a span`)})].some((a) => a.textContent.trim() === "Open")`,
    30_000,
  );
  await click(`${row("vite")} a span`, "Open");
  const url = `http://localhost:${world.fixtures.web.ports[0]}/`;
  for (
    let waited = 0;
    waited < 5000 && !(await readFile(opened, "utf8")).includes(url);
    waited += 100
  )
    await delay(100);
  check(
    "Open hands the service URL to the browser",
    (await readFile(opened, "utf8")).includes(url),
  );
  check("the window stays on the dashboard", (await page(`location.pathname`)) === "/");

  // Try HTTP on a non-HTTP port: a requested probe, its result in the row.
  await click(`${row("postgres")} button`, "Try HTTP");
  const tried = await until(
    `(() => { const t = document.querySelector(${JSON.stringify(row("postgres"))}).textContent; return /trying|not HTTP|no HTTP|answered|refused|HTTP \\d|closed/i.test(t) && !t.includes("Try HTTP") ? t : null; })()`,
    8000,
  );
  check("Try HTTP shows a result", typeof tried === "string", tried);

  // Customize: save a name, see it, then remove the profile through its confirmation.
  await click(`section:has(${row("vite")}) button`, "Customize");
  await dialogVisible("Customize storefront", /^Customize /);
  await click("dialog[open] input");
  await page(`(() => { document.querySelector("dialog[open] input").select(); return true })()`);
  await type(NAME);
  // WebKit queues key and mouse events separately: let the keys land before the click.
  await until(
    `document.querySelector("dialog[open] input").value === ${JSON.stringify(NAME)}`,
    3000,
  );
  await click("dialog[open] button", "Save");
  const renamed = await until(
    `!document.querySelector("dialog[open]") && [...document.querySelectorAll("section h2")].some((h) => h.textContent === ${JSON.stringify(NAME)})`,
    8000,
  );
  check(
    "Customize saves the project name",
    renamed === true,
    await page(`[...document.querySelectorAll("section h2")].map((h) => h.textContent)`),
  );
  // Profile writes are limited to one per second (api.rs WRITE_MIN_INTERVAL); a person never
  // removes within a second of saving, and a faster runner would only test the refusal.
  await delay(1100);
  await click(`section:has(${row("vite")}) button`, "Customize");
  await dialogVisible("Customize (saved)", /^Customize /);
  await click("dialog[open] button", "Remove saved profile");
  const confirm = await until(
    `[...document.querySelectorAll("dialog[open] button")].some((b) => b.textContent.trim() === "Remove")`,
    3000,
  );
  check("Remove saved profile asks to confirm", confirm === true);
  await dialogVisible("Customize remove confirmation", /^Customize /);
  await click("dialog[open] button", "Remove");
  const removed = await until(
    `!document.querySelector("dialog[open]") && ![...document.querySelectorAll("section h2")].some((h) => h.textContent === ${JSON.stringify(NAME)})`,
    8000,
  );
  check(
    "Remove deletes the saved profile",
    removed === true,
    await page(
      `({ dialog: document.querySelector("dialog[open]")?.textContent?.slice(0, 200) ?? null, headings: [...document.querySelectorAll("section h2")].map((h) => h.textContent) })`,
    ),
  );

  // Deep links: the tray's Inspect and Stop… take POST /api/window to the same window.
  const web = identityOf(world.fixtures.web.pid);
  if (web === undefined) throw new Error("web fixture missing from the snapshot");
  await showRoute(world, { kind: "inspect", pid: web.pid, startedAtSecs: web.startedAtSecs });
  await dialogVisible("deep link Inspect", /^vite · PID \d+$/);
  await showRoute(world, { kind: "stop", pid: web.pid, startedAtSecs: web.startedAtSecs });
  await dialogVisible("deep link Stop", /^Stop vite \(PID \d+\)\?$/);
  await click("dialog[open] button", "Cancel");
  check(
    "Cancel closes the deep-linked Stop",
    (await until(`!document.querySelector("dialog[open]")`, 3000)) === true,
  );

  // Stop → still running (the fixture ignores SIGTERM) → Force.
  await click(`${row("next-server")} button`, "Stop");
  await dialogVisible("Stop next-server", /^Stop next-server/);
  await click("dialog[open] button", "Stop process");
  const still = await until(
    `/still running/i.test(document.querySelector("dialog[open]")?.textContent ?? "")`,
    15_000,
  );
  check("Stop reports still running", still === true);
  await dialogVisible("Stop result", /next-server/);
  await click("dialog[open] button", "Force stop…");
  await dialogVisible("Force confirmation", /^Force stop next-server/);
  await click("dialog[open] button", "Force stop");
  const forced = await until(
    `/exited|stopped/i.test(document.querySelector("dialog[open]")?.textContent ?? "")`,
    15_000,
  );
  check(
    "Force stop ends the process",
    forced === true,
    await page(`document.querySelector("dialog[open]")?.textContent`),
  );
  await key("Escape");

  // A supervised worker comes back: the dialog says it respawned.
  await until(`document.querySelector(${JSON.stringify(row("queue-worker"))}) !== null`);
  await click(`${row("queue-worker")} button`, "Stop");
  await dialogVisible("Stop queue-worker", /^Stop queue-worker/);
  await click("dialog[open] button", "Stop process");
  const respawned = await until(
    `/respawn|restarted|came back|listens again/i.test(document.querySelector("dialog[open]")?.textContent ?? "")`,
    15_000,
  );
  check(
    "Stop of a supervised worker reports the respawn",
    respawned === true,
    await page(`document.querySelector("dialog[open]")?.textContent`),
  );
  await dialogVisible("Respawn result", /queue-worker/);
  await key("Escape");

  // Keyboard on rows: j moves focus into the list, Enter inspects the focused row.
  await page(`(() => { document.activeElement?.blur(); return true })()`);
  await key("j");
  const focused = await page(`document.activeElement?.dataset.row !== undefined`);
  check("j focuses the first row", focused === true);
  await key("j");
  await key("k");
  await key("Enter");
  await dialogVisible("Enter inspects the focused row", / · PID \d+$/);
  await key("Escape");

  const end = await control({ op: "status" }, socket);
  check("the window stayed transparent", (end.alpha as number) < 0.02, end.alpha);
}

async function showRoute(world: World, route: Record<string, unknown>) {
  const origin = world.server.url;
  const { token } = (await (await fetch(`${origin}/api/session`)).json()) as { token: string };
  // The window endpoint allows one request per second.
  await delay(1100);
  const response = await fetch(`${origin}/api/window`, {
    method: "POST",
    headers: { Origin: origin, "Content-Type": "application/json", "X-Kosse-Token": token },
    body: JSON.stringify({ route }),
  });
  if (response.status !== 204) throw new Error(`POST /api/window: ${response.status}`);
  // The window loads the route's URL; wait for that page, not the previous one.
  const query = route.kind === "dashboard" ? "" : `?${String(route.kind)}=${String(route.pid)}.`;
  await until(
    `location.search.startsWith(${JSON.stringify(query)}) && document.readyState === "complete"`,
  );
}

await main();
