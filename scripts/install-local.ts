// bun run install:local [--prefix <dir>] [--artifact <kosse-…tar.gz>] [--force]
//
// Installs a release:local archive under a prefix (default ~/.local): <prefix>/bin/kosse and
// <prefix>/share/doc/kosse/, after checking the archive against its .sha256 file and every file
// against MANIFEST.json. Records what it installed in <prefix>/share/kosse/install-record.json so
// uninstall:local removes exactly that. A file is ours only while it is at a recorded path with
// the recorded SHA-256: an upgrade refuses to replace a file it did not install, or one changed
// since (unless --force, which replaces changed recorded files only). Never touches Kossé's
// configuration (`kosse config path`) or login startup.
//
// The record is rewritten after every change, so an install that stops halfway (full disk, a
// signal) still names every file it placed and uninstall:local or a new install can finish.
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative } from "node:path";

import {
  type InstallRecord,
  defaultTarball,
  fail,
  isFileEntry,
  output,
  parseArgs,
  prefixFrom,
  readRecord,
  recordPath,
  run,
  script,
  sha256,
} from "./packaging.ts";

const usage = "bun run install:local [--prefix <dir>] [--artifact <kosse-…tar.gz>] [--force]";

await script(() => {
  const { values, flags } = parseArgs(
    process.argv.slice(2),
    { values: ["--prefix", "--artifact"], flags: ["--force"] },
    usage,
  );
  const prefix = prefixFrom(values);
  const archive = values.get("--artifact") ?? defaultTarball();
  if (!existsSync(archive)) fail(`${archive} does not exist; run \`bun run release:local\` first`);

  // 1. The archive matches its checksum file.
  const sidecar = `${archive}.sha256`;
  if (!existsSync(sidecar)) fail(`${sidecar} is missing; refusing to install an unchecked archive`);
  const expected = readFileSync(sidecar, "utf8").trim().split(/\s+/)[0] ?? "";
  const actual = sha256(archive);
  if (actual !== expected) fail(`${basename(archive)}: SHA-256 ${actual}, expected ${expected}`);

  // 2. Its entries stay inside one top-level directory; extract into a private temporary one.
  const top = basename(archive).replace(/\.tar\.gz$/, "");
  for (const entry of output("/usr/bin/tar", ["-tzf", archive]).split("\n").filter(Boolean)) {
    const parts = entry.split("/");
    if (parts[0] !== top || parts.includes("..") || entry.startsWith("/")) {
      fail(`${basename(archive)}: unexpected entry ${JSON.stringify(entry)}`);
    }
  }
  const scratch = mkdtempSync(join(tmpdir(), "kosse-install-"));
  try {
    install(prefix, archive, actual, join(scratch, top), flags.has("--force"));
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});

function install(
  prefix: string,
  archive: string,
  archiveSha256: string,
  unpacked: string,
  force: boolean,
): void {
  run("/usr/bin/tar", ["-xzf", archive, "-C", dirname(unpacked)]);
  let manifest: unknown;
  try {
    manifest = JSON.parse(readFileSync(join(unpacked, "MANIFEST.json"), "utf8"));
  } catch (error) {
    fail(`MANIFEST.json is unreadable: ${String(error)}`);
  }
  if (!isManifest(manifest)) fail("MANIFEST.json is not a Kossé release manifest");
  for (const file of manifest.files) {
    const digest = sha256(join(unpacked, file.path));
    if (digest !== file.sha256) {
      fail(`${file.path}: SHA-256 ${digest}, MANIFEST.json says ${file.sha256}`);
    }
  }
  // The executable runs here (no Bun or Node needed) and is the version the manifest names.
  const reported = output(join(unpacked, "kosse"), ["--version"], { env: {} }).trim();
  if (reported !== `kosse ${manifest.version}`) fail(`the archive's kosse reports ${reported}`);

  const plan: { from: string; to: string; mode: number }[] = [
    { from: "kosse", to: "bin/kosse", mode: 0o755 },
    ...["LICENSE", "THIRD_PARTY_NOTICES.md", "INSTALL.md", "MANIFEST.json"].map((name) => ({
      from: name,
      to: `share/doc/kosse/${name}`,
      mode: 0o644,
    })),
  ];
  const previous = readRecord(prefix);
  const recorded = new Map(previous?.files.map((file) => [file.path, file.sha256]));
  for (const { to } of plan) {
    const path = join(prefix, to);
    if (!existsSync(path)) continue;
    const digest = recorded.get(to);
    if (digest === undefined) {
      fail(
        `${path} exists and was not installed by install:local; remove it or choose another --prefix`,
      );
    }
    if (sha256(path) !== digest && !force) {
      fail(
        `${path} changed since install:local installed it; move your copy away, or pass --force to replace it`,
      );
    }
  }

  // The record as it stands after each step: what is on disk now, with its hash.
  const state: InstallRecord = {
    name: "kosse",
    version: manifest.version,
    artifactSha256: archiveSha256,
    files: previous?.files ?? [],
    createdDirs: previous?.createdDirs ?? [],
  };
  const save = (): void => {
    const createdDirs = [...new Set(state.createdDirs)].toSorted((a, b) => depth(b) - depth(a));
    const path = join(prefix, recordPath);
    const staged = `${path}.install-${process.pid}`;
    writeFileSync(staged, `${JSON.stringify({ ...state, createdDirs }, null, 2)}\n`, {
      mode: 0o644,
    });
    renameSync(staged, path);
  };
  const setFile = (path: string, digest: string | undefined): void => {
    state.files = state.files.filter((file) => file.path !== path);
    if (digest !== undefined) state.files.push({ path, sha256: digest });
  };

  for (const dir of ["share/kosse", "bin", "share/doc/kosse"]) {
    // Recorded before the next directory is made, so none is ever left unrecorded.
    makeDirs(prefix, dir, (created) => {
      state.createdDirs.push(created);
      if (existsSync(join(prefix, "share/kosse"))) save();
    });
  }
  save();
  for (const { from, to, mode } of plan) {
    // A new file renamed over the old one: a running kosse keeps its own (unchanged) file, and
    // the kernel never sees a signed executable modified in place.
    const destination = join(prefix, to);
    const staged = `${destination}.install-${process.pid}`;
    try {
      copyFileSync(join(unpacked, from), staged);
      chmodSync(staged, mode);
      renameSync(staged, destination);
    } finally {
      rmSync(staged, { force: true });
    }
    setFile(to, sha256(destination));
    save();
  }
  const kept = new Set(plan.map((step) => step.to));
  for (const file of previous?.files ?? []) {
    if (kept.has(file.path)) continue;
    rmSync(join(prefix, file.path), { force: true });
    setFile(file.path, undefined);
    save();
  }

  const binary = join(prefix, "bin", "kosse");
  const verb = previous === undefined ? "installed" : `upgraded ${previous.version} →`;
  const lines = [`kosse: ${verb} ${manifest.version} at ${binary}`];
  const path = (process.env.PATH ?? "").split(":");
  if (!path.includes(join(prefix, "bin"))) {
    lines.push(`note: ${join(prefix, "bin")} is not on PATH; add it to run \`kosse\` by name`);
  }
  if (previous !== undefined) {
    lines.push("note: a Kossé that is already running keeps the previous build until it restarts");
  }
  process.stdout.write(`${lines.join("\n")}\n`);
}

/** The prefix itself (`.`) is the shallowest. */
function depth(dir: string): number {
  return dir === "." ? 0 : dir.split("/").length;
}

/** Creates `relativeDir` below `root`, reporting each directory it created (prefix-relative). */
function makeDirs(root: string, relativeDir: string, created: (dir: string) => void): void {
  let current = root;
  for (const part of ["", ...relativeDir.split("/")]) {
    current = part === "" ? root : join(current, part);
    if (!existsSync(current)) {
      mkdirSync(current);
      created(current === root ? "." : relative(root, current));
    } else if (!statSync(current).isDirectory()) {
      fail(`${current} is not a directory`);
    }
  }
}

type Manifest = { version: string; files: { path: string; sha256: string }[] };

function isManifest(value: unknown): value is Manifest {
  return (
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    value.name === "kosse" &&
    "version" in value &&
    typeof value.version === "string" &&
    "files" in value &&
    Array.isArray(value.files) &&
    value.files.every(isFileEntry)
  );
}
