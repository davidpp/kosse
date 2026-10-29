// bun run test:package — the release archive through its user-facing path, in a sandbox.
//
// Uses the archive in artifacts/ (release:local builds it first), a temporary prefix, HOME and
// KOSSE_CONFIG_DIR, and a PATH without Bun or Node. Installs it with install:local, runs the
// installed `kosse` (--version, doctor, startup status, serve, the menu-bar app), discovers an
// owned fixture, stops it through the API, quits, downloads the archive over HTTP with curl
// (no quarantine attribute), upgrades to a rebuilt archive keeping the configuration, and
// uninstalls, leaving nothing it installed.
//
// The release binary has no debug-only --probe-only, so it would probe the machine's own project
// services automatically. Every installed `kosse` that scans runs under a seatbelt profile
// (sandbox-exec) that refuses outbound IP connections except to the fixture's port: nothing here
// reaches a real service. Login startup is never enabled (no real LaunchAgent, no launchctl
// bootstrap); `startup status` only reads.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { createInterface } from "node:readline";

import { type Fixture, release, startFixture } from "../e2e/support/processes.ts";
import { createRepo } from "../e2e/support/repos.ts";
import type { ActionResult, Health, Session, Snapshot } from "../web/src/generated/contracts.ts";
import {
  defaultTarball,
  output,
  recordPath,
  repoRoot,
  sha256,
  workspaceVersion,
} from "./packaging.ts";

const systemPath = "/usr/bin:/bin:/usr/sbin:/sbin";
const expectedVersion = workspaceVersion();
const owned: ChildProcess[] = [];
const fixtures: Fixture[] = [];
let passed = 0;

function step(name: string): void {
  passed++;
  process.stdout.write(`ok ${passed} - ${name}\n`);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(`assertion failed: ${message}`);
}

const root = mkdtempSync("/tmp/kosse-package-");
const prefix = join(root, "prefix");
const home = join(root, "home");
const config = join(root, "config");
const binary = join(prefix, "bin", "kosse");
mkdirSync(home);
const env = {
  PATH: `${join(prefix, "bin")}:${systemPath}`,
  HOME: home,
  KOSSE_CONFIG_DIR: config,
};

try {
  await main();
  process.stdout.write(`\n${passed} checks passed\n`);
} finally {
  await Promise.all(fixtures.map((fixture) => release(fixture)));
  for (const child of owned) {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }
  rmSync(root, { recursive: true, force: true });
}

async function main(): Promise<void> {
  // The archive under test, kept aside: the upgrade step rebuilds artifacts/.
  const built = defaultTarball();
  assert(existsSync(built), `${built} exists (run bun run release:local)`);
  const first = join(root, "first", basename(built));
  mkdirSync(join(root, "first"));
  copyFileSync(built, first);
  copyFileSync(`${built}.sha256`, `${first}.sha256`);
  const checked = output("/usr/bin/shasum", ["-a", "256", "-c", `${basename(first)}.sha256`], {
    cwd: join(root, "first"),
  });
  assert(checked.trim().endsWith(": OK"), "the archive matches its .sha256");
  step("archive checksum verified with shasum -c");

  // A modified archive is refused before anything is installed.
  const tampered = join(root, "tampered", basename(built));
  mkdirSync(join(root, "tampered"));
  writeFileSync(tampered, Buffer.concat([readFileSync(first), Buffer.from([0])]));
  copyFileSync(`${first}.sha256`, `${tampered}.sha256`);
  const refused = installLocal(["--prefix", prefix, "--artifact", tampered]);
  assert(refused.status === 1 && /SHA-256/.test(refused.stderr), "a tampered archive is refused");
  assert(!existsSync(prefix), "nothing installed from a tampered archive");
  step("tampered archive refused, prefix untouched");

  const installed = installLocal(["--prefix", prefix, "--artifact", first]);
  assert(installed.status === 0, `install:local: ${installed.stderr}`);
  assert(
    JSON.stringify(listFiles(prefix)) ===
      JSON.stringify([
        "bin/kosse",
        "share/doc/kosse/INSTALL.md",
        "share/doc/kosse/LICENSE",
        "share/doc/kosse/MANIFEST.json",
        "share/doc/kosse/THIRD_PARTY_NOTICES.md",
        recordPath,
      ]),
    `installed files: ${listFiles(prefix).join(", ")}`,
  );
  step("install:local into a temporary prefix");

  for (const dir of systemPath.split(":")) {
    assert(
      !existsSync(join(dir, "bun")) && !existsSync(join(dir, "node")),
      `no bun/node in ${dir}`,
    );
  }
  const version = kosse(["--version"]);
  assert(
    version.status === 0 && version.stdout.trim() === `kosse ${expectedVersion}`,
    version.stdout,
  );
  step(`installed kosse --version without Bun or Node on PATH: ${version.stdout.trim()}`);

  const free = await freePort();
  const doctor = kosse(["doctor", "--json", "--port", String(free)]);
  const report: {
    ok: boolean;
    checks: { name: string; level: string; detail: string }[];
  } = JSON.parse(doctor.stdout);
  const levels = Object.fromEntries(report.checks.map((check) => [check.name, check.level]));
  assert(doctor.status === 0 && report.ok, `doctor: ${doctor.stdout}`);
  assert(levels.binary === "ok" && levels.platform === "ok" && levels.port === "ok", doctor.stdout);
  // HOME is temporary, so no agent file is seen; `startup` is `info`, or `warn` when the real
  // launchd domain (queried read-only) still has a Kossé agent loaded on this machine.
  assert(levels.config === "ok" && ["info", "warn"].includes(levels.startup ?? ""), doctor.stdout);
  step("installed kosse doctor: exit 0, binary on PATH, free port, config ok, startup off");

  const status = kosse(["startup", "status"]);
  // `disabled`, or `disabled, but launchd still has …` when a real agent is loaded (read-only).
  assert(status.status === 0 && status.stdout.startsWith("disabled"), status.stdout);
  assert(!existsSync(join(home, "Library", "LaunchAgents")), "no LaunchAgents directory written");
  step("startup status (read-only): disabled");

  // A project with an owned HTTP fixture.
  const project = join(root, "shop");
  await createRepo(project, []);
  const fixture = await startFixture(join(root, "bin"), {
    name: "shop-web",
    cwd: project,
    serve: "title",
    title: "Packaged shop",
  });
  fixtures.push(fixture);
  const fixturePort = fixture.ports[0];
  assert(fixturePort !== undefined, "fixture port");
  // Same project, same kind of server, but outside the seatbelt's allowance: it stands in for the
  // machine's real services, and its probe must fail to connect.
  const blocked = await startFixture(join(root, "bin"), {
    name: "shop-api",
    cwd: project,
    serve: "title",
  });
  fixtures.push(blocked);

  const port = await freePort();
  const serve = sandboxed(["serve", "--no-open", "--port", String(port)], fixturePort);
  await line(serve, /listening on http:\/\/127\.0\.0\.1:\d+\//);
  const base = `http://127.0.0.1:${port}`;
  const page = await fetch(`${base}/`);
  const html = await page.text();
  const script = /<script[^>]+src="(\/assets\/[^"]+\.js)"/.exec(html)?.[1];
  assert(page.status === 200 && script !== undefined, "the embedded dashboard is served");
  const asset = await fetch(`${base}${script}`);
  assert(asset.status === 200 && (await asset.arrayBuffer()).byteLength > 10_000, "bundle served");
  const health = await getJson<Health>(`${base}/api/health`);
  assert(health.mode === "headless" && health.version === expectedVersion, JSON.stringify(health));
  step("installed kosse serve: dashboard page and bundle served from the binary");

  const service = await waitFor("the fixture with an HTTP probe result", async () => {
    const snapshot = await getJson<Snapshot>(`${base}/api/snapshot`);
    const found = snapshot.observation?.services.find(
      (candidate) => candidate.process.identity.pid === fixture.pid,
    );
    const http = found?.endpoints[0]?.http;
    return found !== undefined && http?.state === "probed" ? { snapshot, found, http } : undefined;
  });
  assert(service.found.project === realpathSync(project), `project ${service.found.project}`);
  assert(
    service.http.state === "probed" &&
      service.http.result.kind === "response" &&
      service.http.result.title === "Packaged shop",
    JSON.stringify(service.http),
  );
  step("fixture discovered in its project, probed over HTTP (title read)");

  const denied = await waitFor("the blocked fixture's probe result", async () => {
    const snapshot = await getJson<Snapshot>(`${base}/api/snapshot`);
    const http = snapshot.observation?.services.find(
      (candidate) => candidate.process.identity.pid === blocked.pid,
    )?.endpoints[0]?.http;
    return http?.state === "probed" ? http : undefined;
  });
  assert(
    denied.state === "probed" && denied.result.kind !== "response",
    `the seatbelt blocked the other probe: ${JSON.stringify(denied)}`,
  );
  step(`seatbelt held: a probe to a port outside the allowance failed (${denied.result.kind})`);

  const token = (await getJson<Session>(`${base}/api/session`)).token;
  const post = (path: string, body: unknown): Promise<Response> =>
    fetch(`${base}${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Origin: base, "X-Kosse-Token": token },
      body: JSON.stringify(body),
    });
  const saved = await post("/api/profile", {
    root: service.found.project,
    profile: { version: 1, name: "Packaged shop" },
  });
  assert(saved.status === 200, `save profile: ${saved.status} ${await saved.text()}`);
  const configFile = join(config, "config.json");
  assert(existsSync(configFile), "the profile was written to KOSSE_CONFIG_DIR");
  const configDigest = sha256(configFile);
  step("profile saved through the API into the temporary configuration");

  const stop = await post("/api/stop", {
    instance: service.snapshot.instance,
    identity: service.found.process.identity,
    ports: service.found.endpoints.map((endpoint) => endpoint.port),
  });
  const result: ActionResult = JSON.parse(await stop.text());
  assert(stop.status === 200 && result.outcome === "exited", JSON.stringify(result));
  await fixture.exited;
  step("fixture stopped through POST /api/stop (exited)");

  serve.kill("SIGTERM");
  const [code] = await exit(serve);
  assert(code === 0, `serve exit ${code}`);
  assert(await portFree(port), "serve released its port");
  step("serve quit on SIGTERM (exit 0, port released)");

  // The menu-bar app from the bare executable (no window: --no-open). Its tray icon is real and
  // shows in the menu bar while this runs.
  const appPort = await freePort();
  const app = sandboxed(["--no-open", "--port", String(appPort)], undefined);
  await line(app, /listening on http:\/\/127\.0\.0\.1:\d+\//);
  const resident = await getJson<Health>(`http://127.0.0.1:${appPort}/api/health`);
  assert(resident.mode === "resident", JSON.stringify(resident));
  app.kill("SIGTERM");
  const [appCode] = await exit(app);
  assert(appCode === 0, `menu-bar app exit ${appCode}`);
  step("installed menu-bar app starts (resident) and quits cleanly");

  // Downloaded with curl over HTTP: no quarantine attribute, so the unsigned binary just runs.
  const httpPort = await freePort();
  const http = spawn(
    "/usr/bin/python3",
    ["-m", "http.server", String(httpPort), "--bind", "127.0.0.1"],
    {
      cwd: join(root, "first"),
      stdio: ["ignore", "ignore", "ignore"],
    },
  );
  owned.push(http);
  const download = join(root, "download");
  mkdirSync(download);
  const fetched = join(download, basename(first));
  await waitFor("the HTTP server", async () => {
    const curl = spawnSync("/usr/bin/curl", [
      "-fsS",
      "-o",
      fetched,
      `http://127.0.0.1:${httpPort}/${basename(first)}`,
    ]);
    return curl.status === 0 ? true : undefined;
  });
  http.kill("SIGTERM");
  assert(sha256(fetched) === sha256(first), "downloaded bytes match");
  assert(!quarantine(fetched), "curl download has no com.apple.quarantine");
  spawnSync("/usr/bin/tar", ["-xzf", fetched, "-C", download]);
  const unpacked = join(download, basename(first).replace(/\.tar\.gz$/, ""), "kosse");
  assert(!quarantine(unpacked), "extracted kosse has no com.apple.quarantine");
  const ran = spawnSync(unpacked, ["--version"], { encoding: "utf8", env: { PATH: systemPath } });
  assert(
    ran.status === 0 && ran.stdout.trim() === `kosse ${expectedVersion}`,
    "downloaded kosse runs",
  );
  step("curl-downloaded archive: no quarantine attribute, extracted kosse runs");

  // Upgrade to a rebuilt archive: the configuration is untouched and still applies.
  const rebuilt = spawnSync("bun", ["scripts/release-local.ts"], {
    cwd: repoRoot,
    stdio: ["ignore", "ignore", "inherit"],
  });
  assert(rebuilt.status === 0, "release:local rebuilt the archive");
  const second = defaultTarball();
  const before = readRecord();
  const upgraded = installLocal(["--prefix", prefix, "--artifact", second]);
  assert(
    upgraded.status === 0 && /upgraded 0\.1\.0 → 0\.1\.0/.test(upgraded.stdout),
    upgraded.stdout,
  );
  const after = readRecord();
  assert(after.artifactSha256 === sha256(second), "the record names the rebuilt archive");
  assert(before.artifactSha256 !== after.artifactSha256, "a different archive was installed");
  assert(sha256(configFile) === configDigest, "the configuration file is byte-identical");
  const listed: Snapshot = JSON.parse(kosse(["list", "--json", "--port", "0"]).stdout);
  assert(
    listed.observation?.profiles.some(
      (profile) => profile.root === service.found.project && profile.name === "Packaged shop",
    ),
    "the upgraded kosse reads the saved profile",
  );
  step("upgrade to a rebuilt archive preserves and reads the configuration");

  // A recorded file changed since install is no longer ours: install refuses to replace it,
  // uninstall keeps it; --force overrides both.
  const notes = join(prefix, "share", "doc", "kosse", "INSTALL.md");
  writeFileSync(notes, "my own notes\n");
  const scratchBefore = installScratch();
  const changed = installLocal(["--prefix", prefix, "--artifact", second]);
  assert(
    changed.status === 1 && /changed since install:local installed it/.test(changed.stderr),
    `install over a changed file: ${changed.stderr}`,
  );
  assert(!/\n\s+at /.test(changed.stderr), "an expected failure prints no stack trace");
  assert(readFileSync(notes, "utf8") === "my own notes\n", "the changed file is untouched");
  assert(
    JSON.stringify(installScratch()) === JSON.stringify(scratchBefore),
    "a refused install leaves no kosse-install-* directory behind",
  );
  const keep = run("bun", ["scripts/uninstall-local.ts", "--prefix", prefix]);
  assert(keep.status === 0 && /kept, because they changed/.test(keep.stdout), keep.stdout);
  assert(readFileSync(notes, "utf8") === "my own notes\n", "uninstall kept the changed file");
  assert(!existsSync(binary), "uninstall removed the unchanged files");
  assert(
    JSON.stringify(listFiles(prefix)) ===
      JSON.stringify(["share/doc/kosse/INSTALL.md", recordPath]),
    `left after uninstall: ${listFiles(prefix).join(", ")}`,
  );
  const forced = installLocal(["--prefix", prefix, "--artifact", second, "--force"]);
  assert(forced.status === 0, `install --force: ${forced.stderr}`);
  assert(readFileSync(notes, "utf8") !== "my own notes\n", "--force replaced the changed file");
  writeFileSync(notes, "my own notes\n");
  const forcedOut = run("bun", ["scripts/uninstall-local.ts", "--prefix", prefix, "--force"]);
  assert(forcedOut.status === 0 && !existsSync(prefix), forcedOut.stdout);
  assert(installLocal(["--prefix", prefix, "--artifact", second]).status === 0, "reinstall");
  step("a changed recorded file is neither replaced nor removed without --force");

  // A broken record is a clear refusal, not a crash.
  const record = join(prefix, recordPath);
  const intact = readFileSync(record);
  writeFileSync(record, "{ not json");
  const broken = run("bun", ["scripts/uninstall-local.ts", "--prefix", prefix]);
  assert(
    broken.status === 1 &&
      /is not valid JSON/.test(broken.stderr) &&
      !/\n\s+at /.test(broken.stderr),
    broken.stderr,
  );
  writeFileSync(
    record,
    JSON.stringify({ ...JSON.parse(intact.toString()), createdDirs: ["../x"] }),
  );
  const escaping = run("bun", ["scripts/uninstall-local.ts", "--prefix", prefix]);
  assert(
    escaping.status === 1 && /not an install:local record/.test(escaping.stderr),
    escaping.stderr,
  );
  writeFileSync(record, intact);
  assert(existsSync(binary), "nothing removed by a refused uninstall");
  step("a malformed or escaping install record is refused with a message");

  const removed = run("bun", ["scripts/uninstall-local.ts", "--prefix", prefix]);
  assert(removed.status === 0, `uninstall: ${removed.stderr}`);
  assert(!existsSync(prefix), `prefix left behind: ${listFiles(prefix).join(", ")}`);
  assert(sha256(configFile) === configDigest, "uninstall kept the configuration");
  const again = run("bun", ["scripts/uninstall-local.ts", "--prefix", prefix]);
  assert(again.status === 0 && /nothing installed/.test(again.stdout), again.stdout);
  step("uninstall:local removes every installed file and directory, keeps the configuration");

  assert(installLocal(["--prefix", prefix, "--artifact", second]).status === 0, "reinstall");
  const purged = run("bun", ["scripts/uninstall-local.ts", "--prefix", prefix, "--purge"]);
  assert(purged.status === 0 && !existsSync(config) && !existsSync(prefix), purged.stdout);
  step("uninstall:local --purge also removes the configuration");

  // A file install:local did not create is never overwritten or removed.
  const foreign = join(root, "foreign");
  mkdirSync(join(foreign, "bin"), { recursive: true });
  writeFileSync(join(foreign, "bin", "kosse"), "not ours\n");
  const scratchBeforeClash = installScratch();
  const clash = installLocal(["--prefix", foreign, "--artifact", second]);
  assert(
    JSON.stringify(installScratch()) === JSON.stringify(scratchBeforeClash),
    "no kosse-install-* directory leaked",
  );
  assert(
    clash.status === 1 && /was not installed by install:local/.test(clash.stderr),
    clash.stderr,
  );
  const foreignRemove = run("bun", ["scripts/uninstall-local.ts", "--prefix", foreign]);
  assert(foreignRemove.status === 1, "uninstall refuses a kosse it did not install");
  assert(readFileSync(join(foreign, "bin", "kosse"), "utf8") === "not ours\n", "untouched");
  step("a kosse install:local did not install is neither replaced nor removed");
}

async function getJson<T>(url: string): Promise<T> {
  const value: T = JSON.parse(await (await fetch(url)).text());
  return value;
}

/** `kosse-install-*` scratch directories in the temporary directory install:local uses. */
function installScratch(): string[] {
  return readdirSync(tmpdir()).filter((name) => name.startsWith("kosse-install-"));
}

function quarantine(path: string): boolean {
  return spawnSync("/usr/bin/xattr", ["-p", "com.apple.quarantine", path]).status === 0;
}

type Ran = { status: number | null; stdout: string; stderr: string };

function run(program: string, args: string[], extraEnv: Record<string, string> = {}): Ran {
  const result = spawnSync(program, args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: { ...process.env, HOME: home, KOSSE_CONFIG_DIR: config, ...extraEnv },
  });
  return { status: result.status, stdout: result.stdout, stderr: result.stderr };
}

function installLocal(args: string[]): Ran {
  return run("bun", ["scripts/install-local.ts", ...args]);
}

/** The installed kosse with only the sandbox environment (no Bun or Node on PATH). */
function kosse(args: string[]): Ran {
  const result = spawnSync(binary, args, { encoding: "utf8", env });
  return { status: result.status, stdout: result.stdout, stderr: result.stderr };
}

/** The installed kosse under a seatbelt profile: outbound IP only to `allowPort` on loopback. */
function sandboxed(args: string[], allowPort: number | undefined): ChildProcess {
  const allow =
    allowPort === undefined ? "" : `(allow network-outbound (remote ip "localhost:${allowPort}"))`;
  const profile = `(version 1)(allow default)(deny network-outbound (remote ip "*:*"))${allow}`;
  const child = spawn("/usr/bin/sandbox-exec", ["-p", profile, binary, ...args], {
    env,
    stdio: ["ignore", "ignore", "pipe"],
  });
  owned.push(child);
  return child;
}

function line(child: ChildProcess, match: RegExp, timeoutMs = 30_000): Promise<string> {
  const source = child.stderr;
  if (source === null) return Promise.reject(new Error("no stderr"));
  return new Promise((resolve, reject) => {
    const lines = createInterface({ input: source });
    const seen: string[] = [];
    const timer = setTimeout(
      () => reject(new Error(`timed out waiting for ${match}: ${seen.join(" | ")}`)),
      timeoutMs,
    );
    lines.on("line", (text) => {
      seen.push(text);
      if (match.test(text)) {
        clearTimeout(timer);
        resolve(text);
      }
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`exited (${code}) before ${match}: ${seen.join(" | ")}`));
    });
  });
}

function exit(child: ChildProcess): Promise<[number | null, NodeJS.Signals | null]> {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve([child.exitCode, child.signalCode]);
  }
  return new Promise((resolve) => child.once("exit", (code, signal) => resolve([code, signal])));
}

async function waitFor<T>(
  what: string,
  attempt: () => Promise<T | undefined>,
  timeoutMs = 30_000,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  const once = async (): Promise<T> => {
    const value = await attempt().catch(() => undefined);
    if (value !== undefined) return value;
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 250));
    return once();
  };
  return once();
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address !== null ? address.port : 0;
      server.close(() => resolve(port));
    });
  });
}

function portFree(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const server = createServer();
    server.once("error", () => resolve(false));
    server.listen(port, "127.0.0.1", () => server.close(() => resolve(true)));
  });
}

function readRecord(): { artifactSha256: string } {
  const record: { artifactSha256: string } = JSON.parse(
    readFileSync(join(prefix, recordPath), "utf8"),
  );
  return record;
}

function listFiles(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name).slice(dir.length + 1))
    .toSorted();
}
