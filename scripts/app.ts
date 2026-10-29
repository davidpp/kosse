// `bun run app -- <command>`: the real menu-bar app (tray + Rune window) on a fictional
// fixture world, driven through the window's debug-only control socket. See docs/agent-drive.md.
//
// `start` launches a detached host (`app.ts host`) that owns everything it starts: the fixture
// world (temporary Git projects and listeners, scripts/support/world.ts), the debug `kosse` app on
// this checkout's own port with `--probe-only`/`--act-only` scoped to those fixtures, an isolated
// KOSSE_CONFIG_DIR and a capturing opener. `stop` asks that host to release them by identity.
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFileSync, openSync } from "node:fs";
import { chmod, copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { dirname, join, resolve } from "node:path";

import { startKosseApp } from "./support/processes.ts";
import { delay, elements, isRecord } from "./support/window.ts";
import { createWorld, type World } from "./support/world.ts";

const root = resolve(import.meta.dirname, "..");
const varDir = join(root, "var", "app");
const sessionFile = join(varDir, "session.json");
const appLog = join(varDir, "kosse.log");
const hostLog = join(varDir, "host.log");
const openedLog = join(varDir, "opened.log");
const checkout = createHash("sha256").update(root).digest("hex").slice(0, 10);
/** This checkout's port stays separate from the default owner on 47831. */
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
  status             JSON: session, window title/size/visibility (or why it does not answer).
  tree               Read the rendered Rune element tree.
  text <text>        Type text into the focused native field.
  shot [--out path]  PNG of the native window (default var/app/shot.png).
  resize <w>x<h>     Set the window's content size in points.
  click <x> <y> | click <element-key>
                     A real AppKit click (NSEvent) at window coordinates or an element's centre.
  key <key>          A real key press: j, k, /, Enter, Escape, Tab, Backspace, ArrowDown,
                     ArrowUp, or one character.
  theme <light|dark|system>
                     The native window's appearance.
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

/** Enough of a session file to act on safely; anything else is treated as no session. */
function isSession(value: unknown): value is Session {
  if (!isRecord(value) || !isRecord(value.host) || !isRecord(value.app)) return false;
  const { host: owner, app } = value;
  return (
    typeof owner.pid === "number" &&
    typeof owner.marker === "string" &&
    typeof app.pid === "number" &&
    typeof app.started === "string" &&
    typeof app.url === "string" &&
    typeof app.port === "number" &&
    typeof value.socket === "string" &&
    Array.isArray(value.fixtures)
  );
}

export async function readSession(): Promise<Session | null> {
  try {
    const value: unknown = JSON.parse(await readFile(sessionFile, "utf8"));
    return isSession(value) ? value : null;
  } catch {
    return null;
  }
}

/** Whether `pid` is this checkout's host (by its command line), never just any live PID. */
function isHost(pid: number): boolean {
  const ps = spawnSync("/bin/ps", ["-o", "command=", "-p", String(pid)], { encoding: "utf8" });
  return ps.status === 0 && ps.stdout.includes(hostMarker);
}

/** Native rendering and input tests require an unlocked graphical session. */
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
        const reply: unknown = JSON.parse(data);
        if (!isRecord(reply)) fail(new Error(`unexpected reply: ${data}`));
        else if (typeof reply.error === "string") fail(new Error(reply.error));
        else done(reply);
      } catch {
        fail(new Error(`unexpected reply: ${data}`));
      }
    });
  });
}

/** Build the native app and owned fixture executable. */
export async function build() {
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
    throw new Error("the screen is locked; unlock it to render and drive the native window");
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
        next: "bun run app -- tree | shot | resize <w>x<h> | logs | stop",
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
    window = { error: error instanceof Error ? error.message : String(error) };
  }
  // `<start>` for `show inspect:<pid>.<start>`: each fixture's startedAtSecs from the snapshot.
  const started = new Map<number, number>();
  try {
    const snapshot: unknown = await (await fetch(`${session.app.url}/api/snapshot`)).json();
    const services =
      isRecord(snapshot) && isRecord(snapshot.observation) ? snapshot.observation.services : [];
    for (const service of Array.isArray(services) ? services : []) {
      const identity =
        isRecord(service) && isRecord(service.process) ? service.process.identity : null;
      if (
        isRecord(identity) &&
        typeof identity.pid === "number" &&
        typeof identity.startedAtSecs === "number"
      )
        started.set(identity.pid, identity.startedAtSecs);
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

/** Real AppKit input, addressed by coordinates or an exact rendered Rune element key. */
export async function click(
  target: string[],
  socket = socketPath,
): Promise<Record<string, unknown>> {
  const [first = "", second] = target;
  if (/^\d+(\.\d+)?$/.test(first) && second !== undefined) {
    return control({ op: "click", x: Number(first), y: Number(second) }, socket);
  }
  const found = elements(await control({ op: "tree" }, socket)).find((e) => e.key === first);
  if (found === undefined) throw new Error(`No rendered control with key ${first}`);
  const [x, y, width, height] = found.rect;
  return control({ op: "click", x: x + width / 2, y: y + height / 2 }, socket);
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
  const reply: unknown = await (await fetch(`${origin}/api/session`)).json();
  if (!isRecord(reply) || typeof reply.token !== "string")
    throw new Error("GET /api/session: no token");
  const token = reply.token;
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
    case "scroll":
      return print(
        await control({
          op: "scroll",
          x: Number(rest[0]),
          y: Number(rest[1]),
          delta: Number(rest[2]),
        }),
      );
    case "tree":
      return print(await control({ op: "tree" }));
    case "text":
      return print(await control({ op: "text", text: rest.join(" ") }));
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
