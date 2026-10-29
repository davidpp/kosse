// `bun run app -- <command>`: the real menu-bar app (tray + WKWebView window) on a fictional
// fixture world, driven through the window's debug-only control socket. See docs/agent-drive.md.
//
// `start` launches a detached host (`app.ts host`) that owns everything it starts: the fixture
// world (temporary Git projects and listeners, e2e/support/world.ts), the debug `kosse` app on
// this checkout's own port with `--probe-only`/`--act-only` scoped to those fixtures, an isolated
// KOSSE_CONFIG_DIR and a capturing opener. `stop` asks that host to release them by identity.
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, existsSync, openSync, statSync } from "node:fs";
import { chmod, copyFile, mkdir, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { dirname, join, resolve } from "node:path";

import { startKosseApp } from "../e2e/support/processes.ts";
import { createWorld, type World } from "../e2e/support/world.ts";

const root = resolve(import.meta.dirname, "..");
const varDir = join(root, "var", "app");
const sessionFile = join(varDir, "session.json");
const appLog = join(varDir, "kosse.log");
const hostLog = join(varDir, "host.log");
const openedLog = join(varDir, "opened.log");
const checkout = createHash("sha256").update(root).digest("hex").slice(0, 10);
/** This checkout's port: away from 47831 (installed default) and 47832/47833 (`bun run dev`). */
export const appPort = 48100 + (Number.parseInt(checkout.slice(0, 6), 16) % 800);
const uid = process.getuid?.() ?? 0;
/** A Unix socket path is limited to 104 bytes on macOS; a checkout path may be longer. */
const socketDir = `/tmp/kosse-app-${uid}`;
const socketPath = join(socketDir, `${checkout}.sock`);
const hostMarker = `kosse-app-host:${checkout}`;

const usage = `bun run app -- <command>
  start [--hidden]   Build what is stale, start the real app (tray + window) on a fixture world
                     on 127.0.0.1:${appPort}; prints status JSON once the window answers.
                     --hidden: invisible window (KOSSE_TEST_WINDOW=headless), for unattended runs.
  status             JSON: session, window URL/title/size/visibility (or why it does not answer).
  eval <js>          Run code in the real WKWebView; prints its JSON value. An expression, or a
                     body with return; await works. Elements come back as their HTML.
  shot [--out path]  PNG of the window's page (default var/app/shot.png).
  resize <w>x<h>     Set the window's content size in points.
  click <x> <y> | click <selector> [text]
                     A real AppKit click (NSEvent) at page coordinates or an element's centre.
  key <key>          A real key press: j, k, /, Enter, Escape, Tab, Backspace, ArrowDown,
                     ArrowUp, or one character.
  theme <light|dark|system>
                     The window's appearance (the page's prefers-color-scheme).
  show [route]       Ask the app for its window (POST /api/window): dashboard, inspect:<pid>.<start>
                     or stop:<pid>.<start> — the path the tray's Inspect and Stop… take.
                     <start> is the process's startedAtSecs (status lists the fixtures').
  logs [--follow]    The app's and window's output (var/app/kosse.log).
  opened             URLs the app handed to the browser (captured; no browser opens).
  stop               Quit the app and release the fixture world.`;

export type Session = {
  host: { pid: number; marker: string };
  /** `started` is `ps`'s start time, so a stale session is released by identity only. */
  app: { pid: number; started: string; url: string; port: number };
  socket: string;
  world: string;
  configDir: string;
  hidden: boolean;
  fixtures: { name: string; pid: number; ports: number[] }[];
  logs: { app: string; host: string; opened: string };
};

function print(value: unknown) {
  process.stdout.write(`${JSON.stringify(value, null, 2)}\n`);
}

const delay = (ms: number) => new Promise((done) => setTimeout(done, ms));

export async function readSession(): Promise<Session | null> {
  try {
    return JSON.parse(await readFile(sessionFile, "utf8")) as Session;
  } catch {
    return null;
  }
}

/** Whether `pid` is this checkout's host (by its command line), never just any live PID. */
function isHost(pid: number): boolean {
  const ps = spawnSync("/bin/ps", ["-o", "command=", "-p", String(pid)], { encoding: "utf8" });
  return ps.status === 0 && ps.stdout.includes(hostMarker);
}

/** Whether the console session's screen is locked: every window is then occluded, so WebKit
 * treats the page as hidden (timers and rendering throttled, evaluations unanswered). */
export function screenLocked(): boolean {
  const ioreg = spawnSync("/usr/sbin/ioreg", ["-n", "Root", "-d1"], { encoding: "utf8" });
  return /"CGSSessionScreenIsLocked"=Yes/.test(ioreg.stdout ?? "");
}

/** `ps`'s start time of `pid` (`null` when it is gone): with the PID, a process identity. */
function startOf(pid: number): string | null {
  const ps = spawnSync("/bin/ps", ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8" });
  return ps.status === 0 && ps.stdout.trim() !== "" ? ps.stdout.trim() : null;
}

/** The capturing opener (`KOSSE_TEST_OPENER`): appends each URL to `$KOSSE_APP_OPENED`. The
 * log's path travels in the environment, so no checkout path is interpolated into the script. */
export async function writeOpener(path: string) {
  await writeFile(path, `#!/bin/sh\nprintf '%s\\n' "$1" >> "$KOSSE_APP_OPENED"\n`);
  await chmod(path, 0o700);
}

/** One request to the window's control socket (this checkout's session unless `path`). */
export function control(
  request: Record<string, unknown>,
  path = socketPath,
): Promise<Record<string, unknown>> {
  return new Promise((done, fail) => {
    const socket = createConnection({ path });
    let data = "";
    socket.setTimeout(15_000, () => {
      socket.destroy();
      fail(new Error("the window did not answer within 15 s"));
    });
    socket.on("connect", () => socket.write(`${JSON.stringify(request)}\n`));
    socket.on("data", (chunk) => (data += chunk.toString()));
    socket.on("error", (error: NodeJS.ErrnoException) =>
      fail(
        new Error(
          error.code === "ENOENT" || error.code === "ECONNREFUSED"
            ? "the window is not running (closed, or the app is stopped): `bun run app -- show` reopens it"
            : error.message,
        ),
      ),
    );
    socket.on("end", () => {
      try {
        const reply = JSON.parse(data) as Record<string, unknown>;
        if (typeof reply.error === "string") fail(new Error(reply.error));
        else done(reply);
      } catch {
        fail(new Error(`unexpected reply: ${data}`));
      }
    });
  });
}

// --- eval ------------------------------------------------------------------------------------

const AsyncFunction = (async () => {}).constructor as new (body: string) => unknown;
/** Throws a `SyntaxError` unless `body` parses as an async function body (it is never run). */
const parses = (body: string) => typeof new AsyncFunction(body) === "function";

/** `code` as an async function body: an expression's value is returned, statements run as is. */
function asyncBody(code: string): string {
  const expression = `return (${code}\n);`;
  try {
    parses(expression);
    return expression;
  } catch {
    try {
      parses(code);
    } catch (error) {
      throw new Error(`not valid JavaScript: ${(error as Error).message}`, { cause: error });
    }
    return code;
  }
}

/** Runs `code` in the window. WebKit returns no promise's value, so the result is parked in a
 * page global and read back until it settles. */
export async function evaluate(
  code: string,
  { timeoutMs = 30_000, socket = socketPath } = {},
): Promise<unknown> {
  const id = `e${Date.now()}${Math.random().toString(36).slice(2, 8)}`;
  const key = JSON.stringify(id);
  await control(
    {
      op: "eval",
      js: `(() => {
  const box = (globalThis.__kosseAgent ??= {});
  box[${key}] = { pending: true };
  (async () => { ${asyncBody(code)} })().then(
    (value) => { box[${key}] = { value: value instanceof Element ? value.outerHTML.slice(0, 4000) : value === undefined ? null : value }; },
    (error) => { box[${key}] = { error: error instanceof Error ? error.name + ": " + error.message : String(error) }; },
  );
  return true;
})()`,
    },
    socket,
  );
  const read = `(() => { const box = globalThis.__kosseAgent || {}; const r = box[${key}]; if (r && !r.pending) delete box[${key}]; return JSON.stringify(r ?? { error: "the page reloaded before the code finished" }); })()`;
  for (let waited = 0; waited < timeoutMs; waited += 50) {
    const reply = await control({ op: "eval", js: read }, socket);
    const result = JSON.parse(String(reply.value)) as {
      pending?: boolean;
      value?: unknown;
      error?: string;
    };
    if (result.pending) {
      await delay(50);
      continue;
    }
    if (result.error !== undefined) throw new Error(result.error);
    return result.value;
  }
  throw new Error(`the code did not finish within ${timeoutMs / 1000} s`);
}

// --- start / host / stop ---------------------------------------------------------------------

async function newest(dir: string): Promise<number> {
  let latest = 0;
  for (const entry of await readdir(dir, { withFileTypes: true, recursive: true })) {
    if (entry.isFile())
      latest = Math.max(latest, (await stat(join(entry.parentPath, entry.name))).mtimeMs);
  }
  return latest;
}

/** The web bundle when `web/` changed since, then the debug `kosse` and fixture (cargo decides). */
export async function build() {
  const dist = join(root, "web", "dist", "index.html");
  const sources = Math.max(
    await newest(join(root, "web", "src")),
    statSync(join(root, "web", "index.html")).mtimeMs,
  );
  if (!existsSync(dist) || statSync(dist).mtimeMs < sources) {
    run(["bun", "run", "--cwd", "web", "build"]);
  }
  run(["cargo", "build", "--locked", "-p", "kosse", "-p", "kosse-fixtures", "--bins"]);
}

function run(command: string[]) {
  const [program = "", ...args] = command;
  const result = spawnSync(program, args, { cwd: root, stdio: ["ignore", "inherit", "inherit"] });
  if (result.status !== 0) throw new Error(`${command.join(" ")} failed`);
}

async function start(args: string[]) {
  const existing = await readSession();
  if (existing !== null && isHost(existing.host.pid)) {
    print({ ...(await status()), note: "already running; `bun run app -- stop` first to restart" });
    return;
  }
  if (screenLocked()) {
    throw new Error("the screen is locked: the window would be hidden to WebKit; unlock it first");
  }
  await build();
  await mkdir(varDir, { recursive: true });
  await rm(sessionFile, { force: true });
  const log = openSync(hostLog, "w");
  const child = spawn(
    "bun",
    [
      join(root, "scripts", "app.ts"),
      "host",
      hostMarker,
      ...(args.includes("--hidden") ? ["--hidden"] : []),
    ],
    { cwd: root, detached: true, stdio: ["ignore", log, log] },
  );
  child.unref();
  for (let waited = 0; waited < 60_000; waited += 250) {
    await delay(250);
    if (child.exitCode !== null) break;
    const session = await readSession();
    if (session === null) continue;
    try {
      await control({ op: "status" });
      print({
        ...(await status()),
        next: "bun run app -- eval <js> | shot | resize <w>x<h> | logs | stop",
      });
      return;
    } catch {
      // The window is still starting.
    }
  }
  // The host owns the world and the app; stopping it releases both.
  if (child.exitCode === null) child.kill("SIGTERM");
  throw new Error(`the app did not start; see ${hostLog} and ${appLog}`);
}

/** The detached owner of the world and the app; ends both on SIGTERM/SIGINT. */
async function host(args: string[]) {
  const hidden = args.includes("--hidden");
  await mkdir(socketDir, { recursive: true, mode: 0o700 });
  await chmod(socketDir, 0o700);
  await writeFile(appLog, "");
  await writeFile(openedLog, "");
  const opener = join(varDir, "opener.sh");
  await writeOpener(opener);
  let world: World | null = null;
  let ending = false;
  const end = async () => {
    if (ending) return;
    ending = true;
    await rm(sessionFile, { force: true });
    if (world !== null) await world.dispose();
    process.exit(0);
  };
  process.on("SIGTERM", () => void end());
  process.on("SIGINT", () => void end());

  const env: Record<string, string> = {
    KOSSE_WINDOW_CONTROL: "1",
    KOSSE_WINDOW_CONTROL_SOCKET: socketPath,
    KOSSE_WINDOW_CONTROL_DIR: varDir,
    KOSSE_TEST_OPENER: opener,
    KOSSE_APP_OPENED: openedLog,
  };
  if (hidden) env.KOSSE_TEST_WINDOW = "headless";
  world = await createWorld({
    showcase: true,
    start: (options) =>
      startKosseApp({
        ...options,
        port: options.port ?? appPort,
        env,
        log: (line) => appendFileSync(appLog, `${line}\n`),
      }),
    fixtureLifetimeSecs: 6 * 3600,
  });
  const session: Session = {
    host: { pid: process.pid, marker: hostMarker },
    app: {
      pid: world.server.pid,
      started: startOf(world.server.pid) ?? "",
      url: world.server.url,
      port: world.server.port,
    },
    socket: socketPath,
    world: world.dir,
    configDir: world.configDir,
    hidden,
    fixtures: Object.values(world.fixtures).map(({ name, pid, ports }) => ({ name, pid, ports })),
    logs: { app: appLog, host: hostLog, opened: openedLog },
  };
  await writeFile(sessionFile, `${JSON.stringify(session, null, 2)}\n`);
  // The app ending on its own (Quit from the tray) ends the session too.
  await world.server.exited;
  await end();
}

async function stop() {
  const session = await readSession();
  if (session === null || !isHost(session.host.pid)) {
    // A host that died uncleanly leaves its app behind (its fixtures exit with their stdin):
    // release it only if PID, start time and executable still match the session.
    const app = session?.app;
    const orphan =
      app !== undefined &&
      app.started !== "" &&
      startOf(app.pid) === app.started &&
      spawnSync("/bin/ps", ["-o", "command=", "-p", String(app.pid)], {
        encoding: "utf8",
      }).stdout.startsWith(join(root, "target", "debug", "kosse"));
    if (orphan) process.kill(app.pid, "SIGTERM");
    await rm(sessionFile, { force: true });
    print(
      orphan
        ? { stopped: true, note: `the host was gone; stopped its app (PID ${app.pid})` }
        : { stopped: false, reason: "no app is running for this checkout" },
    );
    return;
  }
  process.kill(session.host.pid, "SIGTERM");
  for (let waited = 0; waited < 20_000; waited += 200) {
    await delay(200);
    if (!isHost(session.host.pid)) {
      print({ stopped: true });
      return;
    }
  }
  throw new Error(`the host (PID ${session.host.pid}) did not stop within 20 s; see ${hostLog}`);
}

// --- reading ---------------------------------------------------------------------------------

export async function status(): Promise<Record<string, unknown>> {
  const session = await readSession();
  if (session === null || !isHost(session.host.pid)) {
    return { running: false, hint: "no app is running for this checkout: `bun run app -- start`" };
  }
  let window: unknown;
  try {
    window = await control({ op: "status" });
  } catch (error) {
    window = { error: (error as Error).message };
  }
  // `<start>` for `show inspect:<pid>.<start>`: each fixture's startedAtSecs from the snapshot.
  const started = new Map<number, number>();
  try {
    const snapshot = (await (await fetch(`${session.app.url}/api/snapshot`)).json()) as {
      observation: {
        services: { process: { identity: { pid: number; startedAtSecs: number } } }[];
      } | null;
    };
    for (const { process: proc } of snapshot.observation?.services ?? []) {
      started.set(proc.identity.pid, proc.identity.startedAtSecs);
    }
  } catch {
    // The window status still says what it can.
  }
  const fixtures = session.fixtures.map((fixture) => ({
    ...fixture,
    startedAtSecs: started.get(fixture.pid) ?? null,
  }));
  return { running: true, ...session, fixtures, window };
}

async function shot(args: string[]): Promise<string> {
  const index = args.indexOf("--out");
  const path = resolve(
    index >= 0 && args[index + 1] !== undefined
      ? (args[index + 1] ?? "")
      : join(varDir, "shot.png"),
  );
  // The window writes only under var/app (or the temporary directory); copy elsewhere after.
  const taken = join(varDir, "shot.png");
  await control({ op: "snapshot", path: taken });
  if (path !== taken) {
    await mkdir(dirname(path), { recursive: true });
    await copyFile(taken, path);
  }
  return path;
}

export async function resize(size: string, socket = socketPath): Promise<Record<string, unknown>> {
  const match = /^(\d+)x(\d+)$/.exec(size);
  if (match === null) throw new Error("give the size as <width>x<height>, e.g. 560x420");
  return control({ op: "resize", width: Number(match[1]), height: Number(match[2]) }, socket);
}

/** A real click at `x y` (page CSS pixels), or at the centre of the first element matching a
 * CSS selector (optionally the one whose text is exactly `text`). */
export async function click(
  target: string[],
  socket = socketPath,
): Promise<Record<string, unknown>> {
  const [first = "", ...rest] = target;
  let point: { x: number; y: number };
  if (/^\d+(\.\d+)?$/.test(first) && rest.length === 1) {
    point = { x: Number(first), y: Number(rest[0]) };
  } else {
    const text = rest.join(" ");
    const found = await evaluate(
      `(() => {
  const all = [...document.querySelectorAll(${JSON.stringify(first)})];
  const el = ${JSON.stringify(text)} === "" ? all[0] : all.find((e) => e.textContent.trim() === ${JSON.stringify(text)});
  if (!el) return null;
  el.scrollIntoView({ block: "center", inline: "nearest" });
  const r = el.getBoundingClientRect();
  return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
})()`,
      { socket },
    );
    if (found === null || typeof found !== "object")
      throw new Error(`nothing matches ${target.join(" ")}`);
    point = found as { x: number; y: number };
  }
  return control({ op: "click", ...point }, socket);
}

/** `POST /api/window`, the request a second launch or the tray's Inspect/Stop… path makes. */
export async function show(route = "dashboard") {
  const session = await readSession();
  if (session === null) {
    throw new Error("no app is running for this checkout: `bun run app -- start`");
  }
  const [kind, id] = route.split(":");
  const [pid, startedAtSecs] = (id ?? "").split(".").map(Number);
  const body = {
    route: kind === "dashboard" ? { kind: "dashboard" } : { kind, pid, startedAtSecs },
  };
  const origin = session.app.url;
  const token = ((await (await fetch(`${origin}/api/session`)).json()) as { token: string }).token;
  const response = await fetch(`${origin}/api/window`, {
    method: "POST",
    headers: { Origin: origin, "Content-Type": "application/json", "X-Kosse-Token": token },
    body: JSON.stringify(body),
  });
  if (response.status !== 204)
    throw new Error(`POST /api/window: ${response.status} ${await response.text()}`);
}

async function logs(args: string[]) {
  if (!args.includes("--follow")) {
    process.stdout.write(await readFile(appLog, "utf8").catch(() => ""));
    return;
  }
  spawn("/usr/bin/tail", ["-n", "50", "-F", appLog], { stdio: "inherit" });
}

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  switch (command) {
    case "start":
      return start(rest);
    case "host":
      return host(rest);
    case "status":
      return print(await status());
    case "eval":
      return print(await evaluate(rest.join(" ")));
    case "shot":
      return print({ path: await shot(rest) });
    case "resize":
      return print(await resize(rest[0] ?? ""));
    case "click":
      return print(await click(rest));
    case "key":
      return print(await control({ op: "key", key: rest[0] ?? "" }));
    case "theme":
      return print(await control({ op: "theme", theme: rest[0] ?? "" }));
    case "show":
      await show(rest[0]);
      return print({ shown: rest[0] ?? "dashboard" });
    case "logs":
      return logs(rest);
    case "opened":
      return process.stdout.write(await readFile(openedLog, "utf8").catch(() => ""));
    case "stop":
      return stop();
    default:
      process.stdout.write(`${usage}\n`);
      if (command !== undefined && command !== "help" && command !== "--help") process.exitCode = 2;
  }
}

if (import.meta.main) {
  main().catch((error: unknown) => {
    process.stderr.write(`app: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
