// Processes the browser suite starts and owns: the real `kosse` debug binary and
// `kosse-fixture` listeners (crates/fixtures). Each is signalled only through the handle of the
// child this module spawned (Node never signals a reaped child), never by name or bare PID.
import { spawn, type ChildProcess } from "node:child_process";
import { copyFile, mkdir } from "node:fs/promises";
import { join } from "node:path";
import { createInterface } from "node:readline";

export const repoRoot = join(import.meta.dirname, "..", "..");
const targetDir = join(repoRoot, "target", "debug");

export type Owned = { child: ChildProcess; pid: number; exited: Promise<void> };

function own(child: ChildProcess): Owned {
  if (child.pid === undefined) throw new Error("spawn failed");
  const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
  return { child, pid: child.pid, exited };
}

export function isRunning(owned: Owned): boolean {
  return owned.child.exitCode === null && owned.child.signalCode === null;
}

/** Stops an owned process: closes stdin (fixtures exit on EOF), then SIGTERM, then SIGKILL. */
export async function release(owned: Owned, graceMs = 3000): Promise<void> {
  if (!isRunning(owned)) return;
  owned.child.stdin?.end();
  if (await settles(owned.exited, graceMs)) return;
  if (isRunning(owned)) owned.child.kill("SIGTERM");
  if (await settles(owned.exited, graceMs)) return;
  if (isRunning(owned)) owned.child.kill("SIGKILL");
  await owned.exited;
}

function settles(promise: Promise<void>, ms: number): Promise<boolean> {
  return Promise.race([
    promise.then(() => true),
    new Promise<boolean>((resolve) => setTimeout(() => resolve(false), ms)),
  ]);
}

function firstLine(
  child: ChildProcess,
  stream: "stdout" | "stderr",
  match: RegExp,
  timeoutMs: number,
): Promise<string> {
  const source = child[stream];
  if (source === null) return Promise.reject(new Error(`no ${stream}`));
  return new Promise((resolve, reject) => {
    const lines = createInterface({ input: source });
    const timer = setTimeout(() => reject(new Error(`timed out waiting for ${match}`)), timeoutMs);
    lines.on("line", (line) => {
      if (!match.test(line)) return;
      clearTimeout(timer);
      // Keep draining so the child never blocks on a full pipe.
      lines.removeAllListeners("line");
      lines.on("line", () => undefined);
      resolve(line);
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`exited (${code}) before ${match}`));
    });
  });
}

export type FixtureOptions = {
  /** Process name others observe: the fixture binary is copied under this name. */
  name: string;
  cwd: string;
  serve?: "banner" | "title" | "not-found" | "error" | "hang" | "tls";
  /** One entry per port; each entry is a comma-separated address group. */
  listen?: string[];
  /** Preferred ports, one per `listen` group; ephemeral ones when any is taken. */
  ports?: number[];
  /** Page title for HTML answers (the default carries an entity and markup). */
  title?: string;
  /** Keep one core busy, so CPU is non-zero. */
  busy?: boolean;
  ignoreTerm?: boolean;
  supervise?: boolean;
  /** Command-line arguments the fixture ignores (what `ps` and Kossé show). */
  args?: string[];
  /** Exits after this long even if its owner never closes stdin (default 900 s). */
  maxLifetimeSecs?: number;
};

export type Fixture = Owned & { name: string; ports: number[] };

export async function startFixture(binDir: string, options: FixtureOptions): Promise<Fixture> {
  await mkdir(binDir, { recursive: true });
  const executable = join(binDir, options.name);
  await copyFile(join(targetDir, "kosse-fixture"), executable).catch(
    (error: NodeJS.ErrnoException) => {
      if (error.code !== "EEXIST") throw error;
    },
  );
  try {
    return await spawnFixture(executable, options, options.ports ?? []);
  } catch (error) {
    if (options.ports === undefined) throw error;
    return spawnFixture(executable, options, []);
  }
}

async function spawnFixture(
  executable: string,
  options: FixtureOptions,
  fixed: number[],
): Promise<Fixture> {
  const config = (options.listen ?? ["127.0.0.1"]).flatMap((group) => ["--listen", group]);
  config.push(
    "--serve",
    options.serve ?? "banner",
    "--max-lifetime-secs",
    String(options.maxLifetimeSecs ?? 900),
  );
  for (const port of fixed) config.push("--port", String(port));
  if (options.title !== undefined) config.push("--title", options.title);
  if (options.busy) config.push("--busy");
  if (options.ignoreTerm) config.push("--ignore-term");
  if (options.supervise) config.push("--supervise");
  // Options travel in the environment, so the command line others observe is only `args`.
  const child = spawn(executable, options.args ?? [], {
    cwd: options.cwd,
    env: { ...process.env, KOSSE_FIXTURE_ARGS: JSON.stringify(config) },
    stdio: ["pipe", "pipe", "ignore"],
  });
  const owned = own(child);
  let line: string;
  try {
    line = await firstLine(child, "stdout", /^\{/, 10_000);
  } catch (error) {
    await release(owned);
    throw error;
  }
  const ready: unknown = JSON.parse(line);
  if (
    typeof ready !== "object" ||
    ready === null ||
    !("listeners" in ready) ||
    !Array.isArray(ready.listeners)
  ) {
    throw new Error("unexpected fixture ready line");
  }
  const ports = ready.listeners.map((address: unknown) =>
    Number(String(address).split(":").at(-1)),
  );
  return { ...owned, name: options.name, ports };
}

export type Server = Owned & { url: string; port: number };

export type ServerOptions = {
  cwd: string;
  configDir: string;
  probe: number[];
  act: number[];
  port?: number;
};

/** `kosse serve` scoped to the given fixtures: nothing else is probed or can be signalled. */
export async function startKosse(options: ServerOptions): Promise<Server> {
  const child = spawn(join(targetDir, "kosse"), ["serve", "--no-open", ...serverArgs(options)], {
    cwd: options.cwd,
    env: { ...process.env, KOSSE_CONFIG_DIR: options.configDir },
    stdio: ["ignore", "ignore", "pipe"],
  });
  return listening(own(child));
}

function serverArgs(options: ServerOptions): string[] {
  return [
    "--port",
    String(options.port ?? 0),
    `--probe-only=${options.probe.join(",")}`,
    `--act-only=${options.act.join(",")}`,
  ];
}

/**
 * The menu-bar app (tray + dashboard window) scoped like `startKosse`, for `bun run app` and the
 * real-window suite. `env` adds the window hooks; every output line goes to `log`.
 */
export async function startKosseApp(
  options: ServerOptions & { env: Record<string, string>; log: (line: string) => void },
): Promise<Server> {
  const child = spawn(join(targetDir, "kosse"), serverArgs(options), {
    cwd: options.cwd,
    env: { ...process.env, ...options.env, KOSSE_CONFIG_DIR: options.configDir },
    stdio: ["ignore", "pipe", "pipe"],
  });
  for (const stream of [child.stdout, child.stderr]) {
    if (stream !== null) createInterface({ input: stream }).on("line", options.log);
  }
  return listening(own(child));
}

async function listening(owned: Owned): Promise<Server> {
  const child = owned.child;
  const line = await firstLine(child, "stderr", /listening on http:\/\//, 30_000).catch(
    async (error: unknown) => {
      await release(owned);
      throw error;
    },
  );
  const url = /listening on (http:\/\/\S+?)\/?$/.exec(line)?.[1];
  if (url === undefined) throw new Error(`unexpected kosse output: ${line}`);
  return { ...owned, url, port: Number(new URL(url).port) };
}
