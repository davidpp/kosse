// Temporary projects for the browser suite: Git repositories built with the developer-directory
// `git` Kossé itself runs, ignoring the user's Git configuration.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";

const developerDir = execFileSync("/usr/bin/xcode-select", ["-p"], { encoding: "utf8" }).trim();
const gitBinary = join(developerDir, "usr", "bin", "git");

export function git(cwd: string, ...args: string[]): string {
  const env: NodeJS.ProcessEnv = {
    PATH: process.env.PATH,
    HOME: process.env.HOME,
    GIT_CONFIG_GLOBAL: "/dev/null",
    GIT_CONFIG_NOSYSTEM: "1",
  };
  return execFileSync(
    gitBinary,
    [
      "-c",
      "user.name=Fixture",
      "-c",
      "user.email=fixture@example.invalid",
      "-c",
      "init.defaultBranch=main",
      ...args,
    ],
    {
      cwd,
      env,
      encoding: "utf8",
    },
  );
}

/** A repository with packages under `apps/` (each with a package.json) and one commit. */
export async function createRepo(dir: string, packages: string[]): Promise<void> {
  await mkdir(dir, { recursive: true });
  git(dir, "init", "-q");
  await writeFile(join(dir, "README.md"), "Fixture repository for the Kossé browser suite.\n");
  await Promise.all(
    packages.map(async (name) => {
      await mkdir(join(dir, "apps", name), { recursive: true });
      await writeFile(
        join(dir, "apps", name, "package.json"),
        `${JSON.stringify({ name, private: true })}\n`,
      );
    }),
  );
  git(dir, "add", "-A");
  git(dir, "commit", "-q", "-m", "fixture");
}

/** Every file below `dir` (except `.git` internals) with a content hash. */
export async function fingerprint(dir: string): Promise<Record<string, string>> {
  const entries = await readdir(dir, { recursive: true, withFileTypes: true });
  const files = entries
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name))
    .filter((path) => !relative(dir, path).startsWith(".git/"));
  const hashes = await Promise.all(
    files.map(
      async (path) =>
        [
          relative(dir, path),
          createHash("sha256")
            .update(await readFile(path))
            .digest("hex"),
        ] as const,
    ),
  );
  return Object.fromEntries(hashes);
}
