// Runs the Rust backend and the Vite dev server together. The browser talks only to Vite, which
// proxies /api to the backend (same origin). Every child is stopped by its handle when any child
// exits, when a later step fails, or when this process receives SIGINT/SIGTERM.
import { existsSync } from "node:fs";
import { join } from "node:path";

import type { Subprocess } from "bun";

import { devPorts } from "../web/dev-ports.ts";

const root = join(import.meta.dir, "..");
const web = join(root, "web");
const vite = join(web, "node_modules", ".bin", "vite");
const shutdownGrace = 5_000;

const children: Subprocess[] = [];
let stopping: Promise<void> | undefined;

function stopAll(): Promise<void> {
  stopping ??= Promise.all(
    children.map(async (child) => {
      if (child.exitCode !== null || child.signalCode !== null) return;
      child.kill("SIGTERM");
      const exited = await Promise.race([
        child.exited.then(() => true),
        Bun.sleep(shutdownGrace).then(() => false),
      ]);
      if (!exited) {
        console.error(`dev: pid ${child.pid} ignored SIGTERM; sending SIGKILL`);
        child.kill("SIGKILL");
        await child.exited;
      }
    }),
  ).then(() => undefined);
  return stopping;
}

async function fail(message: string): Promise<never> {
  console.error(`dev: ${message}`);
  await stopAll();
  process.exit(1);
}

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    void stopAll().then(() => process.exit(0));
  });
}

/** Spawns and records a child; a spawn failure stops the children already running. */
async function spawn(
  command: string[],
  cwd: string,
  stdio: ["inherit" | "ignore", "inherit" | "pipe", "inherit"],
): Promise<Subprocess<"inherit" | "ignore", "inherit" | "pipe", "inherit">> {
  if (stopping) {
    await stopping;
    process.exit(0);
  }
  try {
    const child = Bun.spawn(command, { cwd, stdio });
    children.push(child);
    return child;
  } catch (error) {
    return fail(`could not start ${command[0]}: ${String(error)}`);
  }
}

/** Builds the backend and returns the executable cargo reports, so any target-dir config holds. */
async function buildBackend(): Promise<string> {
  const build = await spawn(
    ["cargo", "build", "--locked", "-p", "stackscan", "--message-format=json-render-diagnostics"],
    root,
    ["ignore", "pipe", "inherit"],
  );
  const output =
    build.stdout instanceof ReadableStream ? await new Response(build.stdout).text() : "";
  if ((await build.exited) !== 0) {
    return fail("cargo build failed");
  }
  for (const line of output.split("\n")) {
    if (!line.startsWith("{")) continue;
    const message: unknown = JSON.parse(line);
    if (
      typeof message === "object" &&
      message !== null &&
      "reason" in message &&
      message.reason === "compiler-artifact" &&
      "executable" in message &&
      typeof message.executable === "string"
    ) {
      return message.executable;
    }
  }
  return fail("cargo build did not report the stackscan executable");
}

if (!existsSync(vite)) {
  await fail(`${vite} not found; run \`bun install\` first`);
}

const executable = await buildBackend();
const backend = await spawn(
  // --dev (debug builds only) admits the Vite authority in the backend's Host/Origin policy.
  [executable, "serve", "--no-open", "--dev", "--port", String(devPorts.backend)],
  root,
  ["ignore", "inherit", "inherit"],
);
const frontend = await spawn([vite], web, ["inherit", "inherit", "inherit"]);

const first = await Promise.race(
  [backend, frontend].map((child) => child.exited.then(() => child)),
);
if (!stopping) {
  console.error(`dev: pid ${first.pid} exited with code ${first.exitCode}; stopping the rest`);
  await stopAll();
  process.exit(first.exitCode === 0 ? 1 : (first.exitCode ?? 1));
}
