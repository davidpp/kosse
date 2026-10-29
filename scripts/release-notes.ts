// bun scripts/release-notes.ts [--tag <tag>] [--out <file>]
//
// The release workflow's preflight: the tag (when given) names exactly the workspace version, and
// CHANGELOG.md has a dated section for that version. Writes the section's body as the GitHub
// Release notes (default artifacts/RELEASE_NOTES.md) and prints the version.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { artifactsDir, fail, parseArgs, repoRoot, script, workspaceVersion } from "./packaging.ts";

const usage = "bun scripts/release-notes.ts [--tag <tag>] [--out <file>]";

await script(() => {
  const { values } = parseArgs(
    process.argv.slice(2),
    { values: ["--tag", "--out"], flags: [] },
    usage,
  );
  const version = workspaceVersion();
  const tag = values.get("--tag");
  if (tag !== undefined && tag !== `v${version}`) {
    fail(`tag ${tag} does not match the workspace version ${version} (expected v${version})`);
  }

  const changelog = readFileSync(join(repoRoot, "CHANGELOG.md"), "utf8");
  const heading = new RegExp(`^## ${version.replaceAll(".", "\\.")} - \\d{4}-\\d{2}-\\d{2}$`, "m");
  const start = heading.exec(changelog);
  if (start === null) fail(`CHANGELOG.md has no "## ${version} - YYYY-MM-DD" section`);
  const rest = changelog.slice(start.index + start[0].length);
  // The section ends at the next version heading or the link references at the bottom.
  const end = rest.search(/^(## |\[[^\]]+\]: )/m);
  const body = (end === -1 ? rest : rest.slice(0, end)).trim();
  if (body === "") fail(`CHANGELOG.md's ${version} section is empty`);

  const out = values.get("--out") ?? join(artifactsDir, "RELEASE_NOTES.md");
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, `${body}\n`);
  process.stdout.write(`${version}\n`);
});
