// Shared by release:local, install:local, uninstall:local and test:package: artifact naming,
// checksums and child processes. Only macOS arm64 is packaged (blueprint/architecture.md).
import { spawnSync, type SpawnSyncOptions } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, realpathSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";

export const repoRoot = join(import.meta.dirname, "..");
export const target = "aarch64-apple-darwin";
export const artifactsDir = join(repoRoot, "artifacts");
export const agentLabel = "com.davidpp.kosse";

/** The workspace version (Cargo.toml `[workspace.package]`), which `kosse --version` prints. */
export function workspaceVersion(): string {
  const manifest = readFileSync(join(repoRoot, "Cargo.toml"), "utf8");
  const section = manifest.split("[workspace.package]")[1] ?? "";
  const version = /^version\s*=\s*"([^"]+)"/m.exec(section)?.[1];
  if (version === undefined) throw new Error("no version in Cargo.toml [workspace.package]");
  return version;
}

export function artifactName(version: string): string {
  return `kosse-${version}-${target}`;
}

export function defaultTarball(): string {
  return join(artifactsDir, `${artifactName(workspaceVersion())}.tar.gz`);
}

export function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/** Runs a command to completion with inherited output; throws on failure. */
export function run(program: string, args: string[], options: SpawnSyncOptions = {}): void {
  const result = spawnSync(program, args, { stdio: "inherit", cwd: repoRoot, ...options });
  if (result.error !== undefined) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${program} ${args.join(" ")} exited with ${result.status ?? result.signal}`);
  }
}

/** Runs a command and returns its stdout; throws on failure with its stderr. */
export function output(program: string, args: string[], options: SpawnSyncOptions = {}): string {
  const result = spawnSync(program, args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    ...options,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error !== undefined) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${program} ${args.join(" ")} exited with ${result.status ?? result.signal}: ${String(result.stderr).trim()}`,
    );
  }
  return String(result.stdout);
}

/** `--name value` options and bare `--flag`s; anything else is a usage error. */
export function parseArgs(
  argv: string[],
  spec: { values: string[]; flags: string[] },
  usage: string,
): { values: Map<string, string>; flags: Set<string> } {
  const values = new Map<string, string>();
  const flags = new Set<string>();
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index] ?? "";
    const [name = "", inline] = arg.split(/=(.*)/s, 2);
    if (spec.flags.includes(name) && inline === undefined) {
      flags.add(name);
    } else if (spec.values.includes(name)) {
      const value = inline ?? argv[++index];
      if (value === undefined || value === "") usageError(usage, `${name} needs a value`);
      values.set(name, value);
    } else {
      usageError(usage, `unknown argument ${JSON.stringify(arg)}`);
    }
  }
  return { values, flags };
}

export function usageError(usage: string, message: string): never {
  throw new Refusal(`${message}\nusage: ${usage}`, 2);
}

/** `--prefix` (default `~/.local`), resolved to an absolute path. */
export function prefixFrom(values: Map<string, string>): string {
  const prefix = values.get("--prefix") ?? join(homedir(), ".local");
  return isAbsolute(prefix) ? prefix : resolve(prefix);
}

/** What install:local records under the prefix, so uninstall removes exactly these. */
export type InstallRecord = {
  name: "kosse";
  version: string;
  artifactSha256: string;
  /** Prefix-relative files, with the SHA-256 they were installed with. */
  files: { path: string; sha256: string }[];
  /** Prefix-relative directories install:local created, deepest first. */
  createdDirs: string[];
};

export const recordPath = "share/kosse/install-record.json";

/** An expected failure: its message is the whole report (no stack trace). */
export class Refusal extends Error {
  constructor(
    message: string,
    readonly exitCode = 1,
  ) {
    super(message);
  }
}

/** Stops the script with a message (exit 1) by throwing: `finally` cleanup still runs. */
export function fail(message: string): never {
  throw new Refusal(message);
}

/** Runs a script body: a [`Refusal`] prints its message and sets the exit code after every
 *  `finally` inside the body has run; anything else is a bug and keeps its stack trace. */
export async function script(body: () => void | Promise<void>): Promise<void> {
  try {
    await body();
  } catch (error) {
    if (!(error instanceof Refusal)) throw error;
    process.stderr.write(`${error.message}\n`);
    process.exitCode = error.exitCode;
  }
}

export function readRecord(root: string): InstallRecord | undefined {
  const path = join(root, recordPath);
  if (!existsSync(path)) return undefined;
  let record: unknown;
  try {
    record = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`${path} is not valid JSON (${String(error)}); fix or remove it`);
  }
  if (!isRecord(record)) fail(`${path} is not an install:local record; fix or remove it`);
  return record;
}

function isRecord(value: unknown): value is InstallRecord {
  return (
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    value.name === "kosse" &&
    "version" in value &&
    typeof value.version === "string" &&
    "files" in value &&
    Array.isArray(value.files) &&
    value.files.every(isFileEntry) &&
    "createdDirs" in value &&
    Array.isArray(value.createdDirs) &&
    value.createdDirs.every(
      (dir: unknown) => typeof dir === "string" && (dir === "." || isInside(dir)),
    )
  );
}

/** A relative path that stays below its root: not absolute, no `..`, no empty or `.` parts. */
function isInside(path: string): boolean {
  return (
    path !== "" &&
    !path.startsWith("/") &&
    path.split("/").every((part) => part !== "" && part !== "." && part !== "..")
  );
}

export function isFileEntry(value: unknown): value is { path: string; sha256: string } {
  return (
    typeof value === "object" &&
    value !== null &&
    "path" in value &&
    typeof value.path === "string" &&
    isInside(value.path) &&
    "sha256" in value &&
    typeof value.sha256 === "string"
  );
}

/** What the login-startup agent under `home` runs, read with `plutil`. */
export type Agent =
  | { kind: "none" }
  | { kind: "program"; path: string }
  | { kind: "unreadable"; reason: string };

export function startupAgent(home: string): Agent {
  const plist = join(home, "Library", "LaunchAgents", `${agentLabel}.plist`);
  if (!existsSync(plist)) return { kind: "none" };
  const result = spawnSync(
    "/usr/bin/plutil",
    ["-extract", "ProgramArguments.0", "raw", "-o", "-", plist],
    { encoding: "utf8" },
  );
  if (result.status !== 0) {
    return { kind: "unreadable", reason: `${plist}: ${String(result.stderr).trim()}` };
  }
  return { kind: "program", path: String(result.stdout).trim() };
}

/** Same file by device and inode (symbolic links and `..` resolved); false if either is missing. */
export function sameFile(a: string, b: string): boolean {
  try {
    const [first, second] = [statSync(realpathSync(a)), statSync(realpathSync(b))];
    return first.dev === second.dev && first.ino === second.ino;
  } catch {
    return false;
  }
}
