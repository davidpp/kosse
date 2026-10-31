// bun run release <X.Y.Z>
//
// Cuts a release from an up-to-date, clean `main`: moves CHANGELOG's Unreleased entries under the
// version, bumps Cargo and the README status through a PR, merges it once CI is green, tags the
// merge and follows release.yml, which publishes. A failure stops where it is and says so; nothing
// is published before the tag.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { fail, repoRoot, script, workspaceVersion } from "./packaging.ts";

const POLL_MS = 5000;
const POLLS = 24;

await script(async () => {
  const version = process.argv[2];
  if (process.argv.length !== 3 || version === undefined || !/^\d+\.\d+\.\d+$/.test(version)) {
    fail("usage: bun run release <X.Y.Z>");
  }
  const current = workspaceVersion();
  if (Bun.semver.order(version, current) !== 1) {
    fail(`${version} is not newer than ${current}`);
  }
  const tag = `v${version}`;

  output("git", "fetch", "--tags", "origin");
  if (output("git", "branch", "--show-current") !== "main") fail("run this on main");
  if (output("git", "status", "--porcelain") !== "") fail("the working tree is not clean");
  if (output("git", "rev-parse", "HEAD") !== output("git", "rev-parse", "origin/main")) {
    fail("main is not at origin/main");
  }
  if (output("git", "tag", "--list", tag) !== "") fail(`${tag} already exists`);

  const date = new Date().toLocaleDateString("en-CA");
  edit("CHANGELOG.md", (text) => {
    const unreleased = /^## Unreleased\n([\s\S]*?)(?=^## )/m.exec(text);
    const entries = unreleased?.[1];
    if (unreleased === null || entries === undefined || entries.trim() === "") {
      fail("CHANGELOG.md has nothing under ## Unreleased");
    }
    return text.replace(
      unreleased[0],
      () => `## Unreleased

## ${version} - ${date}
${entries}`,
    );
  });
  edit("Cargo.toml", (text) => text.replace(`version = "${current}"`, `version = "${version}"`));
  edit("README.md", (text) => text.replace(`Status: v${current}`, `Status: v${version}`));
  run("cargo", "update", "--workspace");

  const branch = `release-${version}`;
  run("git", "switch", "-c", branch);
  run("git", "add", "CHANGELOG.md", "Cargo.toml", "Cargo.lock", "README.md");
  run("git", "commit", "-m", `chore: release ${version}`);
  run("git", "push", "-u", "origin", branch);
  const title = `chore: release ${version}`;
  const url = output("gh", "pr", "create", "--base", "main", "--title", title, "--body", title);

  await eventually(`no CI checks appeared on ${url}`, () => attempt("gh", "pr", "checks", url));
  run("gh", "pr", "checks", url, "--watch", "--fail-fast", "--interval", "20");
  run("gh", "pr", "merge", url, "--squash", "--delete-branch");

  run("git", "switch", "main");
  run("git", "pull", "--ff-only", "origin", "main");
  if (workspaceVersion() !== version) fail(`main is not at ${version} after the merge`);
  run("git", "tag", "-a", tag, "-m", `Kossé ${version}`);
  run("git", "push", "origin", tag);

  const list = ["run", "list", "--workflow", "release.yml", "--branch", tag, "--limit", "1"];
  const runId = await eventually(`no release workflow started for ${tag}`, () => {
    const id = output("gh", ...list, "--json", "databaseId", "--jq", ".[0].databaseId // empty");
    return id === "" ? null : id;
  });
  run("gh", "run", "watch", runId, "--exit-status", "--interval", "30");
  run("gh", "release", "view", tag, "--json", "url", "--jq", ".url");
});

function edit(path: string, change: (text: string) => string): void {
  const file = join(repoRoot, path);
  const before = readFileSync(file, "utf8");
  const after = change(before);
  if (after === before) fail(`${path} did not change`);
  writeFileSync(file, after);
}

function run(...command: string[]): void {
  const { exitCode } = Bun.spawnSync(command, {
    cwd: repoRoot,
    stdio: ["inherit", "inherit", "inherit"],
  });
  if (exitCode !== 0) fail(`${command.slice(0, 3).join(" ")} failed; the release stopped here`);
}

function attempt(...command: string[]): string | null {
  const { exitCode, stdout } = Bun.spawnSync(command, { cwd: repoRoot, stderr: "ignore" });
  // `gh pr checks` exits 8 while checks are pending.
  return exitCode === 0 || exitCode === 8 ? stdout.toString().trim() : null;
}

function output(...command: string[]): string {
  const result = attempt(...command);
  if (result === null) fail(`${command.slice(0, 3).join(" ")} failed; the release stopped here`);
  return result;
}

async function eventually<T>(failure: string, read: () => T | null): Promise<T> {
  for (let poll = 0; poll < POLLS; poll += 1) {
    const value = read();
    if (value !== null) return value;
    // oxlint-disable-next-line eslint/no-await-in-loop -- polling is sequential by nature.
    await Bun.sleep(POLL_MS);
  }
  fail(failure);
}
