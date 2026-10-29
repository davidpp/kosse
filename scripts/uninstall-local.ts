// bun run uninstall:local [--prefix <dir>] [--purge] [--force]
//
// Removes exactly what install:local recorded under the prefix (default ~/.local), then the
// directories it created if they are empty. A recorded file whose SHA-256 changed since install
// is no longer ours: it is kept (and stays in the record) unless --force. Kossé's configuration
// (saved profiles, at `kosse config path`) stays unless --purge. Refuses while login startup runs
// this executable, or when the startup agent cannot be read: `kosse startup disable` first.
import { existsSync, readdirSync, renameSync, rmdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

import {
  fail,
  output,
  parseArgs,
  prefixFrom,
  readRecord,
  recordPath,
  sameFile,
  script,
  sha256,
  startupAgent,
} from "./packaging.ts";

const usage = "bun run uninstall:local [--prefix <dir>] [--purge] [--force]";

await script(() => {
  const { values, flags } = parseArgs(
    process.argv.slice(2),
    { values: ["--prefix"], flags: ["--purge", "--force"] },
    usage,
  );
  const prefix = prefixFrom(values);
  const binary = join(prefix, "bin", "kosse");
  const record = readRecord(prefix);
  if (record === undefined) {
    if (existsSync(binary)) {
      fail(`${binary} was not installed by install:local (no ${recordPath}); leaving it alone`);
    }
    process.stdout.write(`kosse: nothing installed under ${prefix}\n`);
    return;
  }

  // Login startup pointing at this executable would fail at every login once it is gone.
  const agent = startupAgent(homedir());
  if (agent.kind === "unreadable") {
    fail(
      `cannot tell which kosse login startup runs (${agent.reason}); inspect it with \`kosse startup status\` or run \`kosse startup disable\` first`,
    );
  }
  if (agent.kind === "program" && sameFile(agent.path, binary)) {
    fail(`login startup runs ${binary}; run \`${binary} startup disable\` first`);
  }

  // The configuration file's location comes from the installed binary itself (it honours
  // KOSSE_CONFIG_DIR), before that binary is removed.
  const configFile =
    flags.has("--purge") && existsSync(binary)
      ? output(binary, ["config", "path"]).trim()
      : undefined;

  const kept: typeof record.files = [];
  let removed = 0;
  for (const file of record.files) {
    const path = join(prefix, file.path);
    if (!existsSync(path)) continue;
    if (sha256(path) !== file.sha256 && !flags.has("--force")) {
      kept.push(file);
      continue;
    }
    rmSync(path, { force: true });
    removed++;
  }
  const recordFile = join(prefix, recordPath);
  if (kept.length > 0) {
    // Still recorded, so a later `--force` run (or a reinstall's ownership check) knows them.
    const staged = `${recordFile}.uninstall-${process.pid}`;
    writeFileSync(staged, `${JSON.stringify({ ...record, files: kept }, null, 2)}\n`);
    renameSync(staged, recordFile);
  } else {
    rmSync(recordFile, { force: true });
  }
  for (const dir of record.createdDirs) {
    const path = join(prefix, dir);
    if (existsSync(path) && readdirSync(path).length === 0) rmdirSync(path);
  }

  const lines = [`kosse: removed ${record.version} from ${prefix} (${removed} files)`];
  if (kept.length > 0) {
    lines.push(
      `kept, because they changed since install (\`--force\` removes them): ${kept
        .map((file) => join(prefix, file.path))
        .join(", ")}`,
    );
  }
  if (configFile !== undefined) {
    // Only Kossé's own files: the configuration and its lock, then the directory if now empty.
    const dir = dirname(configFile);
    for (const name of [configFile, join(dir, "config.lock")]) rmSync(name, { force: true });
    if (existsSync(dir) && readdirSync(dir).length === 0) rmdirSync(dir);
    lines.push(`purged the configuration ${configFile}`);
  } else if (flags.has("--purge")) {
    lines.push(
      "note: --purge: no installed kosse to locate the configuration with; nothing purged",
    );
  } else {
    lines.push("kept the configuration (saved profiles); `--purge` removes it");
  }
  process.stdout.write(`${lines.join("\n")}\n`);
});
