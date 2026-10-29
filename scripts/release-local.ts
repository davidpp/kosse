// bun run release:local [--url <archive URL for the formula>]
//
// Builds the macOS arm64 release archive in artifacts/, publishing nothing:
//   kosse-<version>-aarch64-apple-darwin.tar.gz   kosse (embedded UI), LICENSE,
//                                                  THIRD_PARTY_NOTICES.md, INSTALL.md, MANIFEST.json
//   kosse-<version>-aarch64-apple-darwin.tar.gz.sha256
//   kosse.rb                                       Homebrew formula for that archive
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import {
  artifactName,
  artifactsDir,
  fail,
  output,
  parseArgs,
  repoRoot,
  run,
  script,
  sha256,
  target,
  workspaceVersion,
} from "./packaging.ts";

const usage = "bun run release:local [--url <archive URL>]";

await script(() => {
  const { values } = parseArgs(process.argv.slice(2), { values: ["--url"], flags: [] }, usage);

  const version = workspaceVersion();
  const name = artifactName(version);
  const tarball = `${name}.tar.gz`;
  const url = values.get("--url") ?? `REPLACE-WITH-THE-URL-OF/${tarball}`;

  // 0. about.toml renders exactly the license policy deny.toml enforces.
  assertSamePolicy();

  // 1. The web bundle (within budget) and the release binary that embeds it. cargo-auditable
  //    embeds the dependency list, so `cargo audit bin` and SBOM scanners can check the shipped file.
  run("bun", ["run", "--cwd", "web", "build"]);
  run("bun", ["run", "budget"]);
  run("cargo", ["auditable", "build", "--release", "--locked", "-p", "kosse"]);
  const binary = join(repoRoot, "target", "release", "kosse");
  const reported = output(binary, ["--version"]).trim();
  if (reported !== `kosse ${version}`) fail(`the binary reports ${reported}`);

  // 2. Third-party notices: Rust crates the binary links (cargo-about, the license policy of
  //    deny.toml restated in about.toml), then the JavaScript the dashboard bundles (Vite's
  //    build.license output).
  const rustNotices = output("cargo", [
    "about",
    "generate",
    "--locked",
    "--fail",
    "--manifest-path",
    "crates/app/Cargo.toml",
    "packaging/notices.hbs",
  ]);
  const webLicenses = readFileSync(join(repoRoot, "web", "dist", ".vite", "license.md"), "utf8");
  // Tailwind generates part of the shipped CSS (its preflight and utilities), which Vite's JS
  // license collection does not see.
  const tailwind = join(repoRoot, "web", "node_modules", "tailwindcss");
  const tailwindVersion = String(
    field(JSON.parse(readFileSync(join(tailwind, "package.json"), "utf8")), "version"),
  );
  const tailwindLicense = readFileSync(join(tailwind, "LICENSE"), "utf8").trim();
  const notices = `${rustNotices.trimEnd()}\n\n## Web dashboard (bundled JavaScript and generated CSS)\n\n${webLicenses
    .replace(/^# Licenses\s*\n/, "")
    .replace(/^## /gm, "### ")
    .replace(/^The app bundles dependencies which contain the following licenses:\s*\n/m, "")
    .trim()}\n\n### tailwindcss - ${tailwindVersion} (MIT)\n\n${tailwindLicense}\n`;
  // The notices must carry what the policy singles out, under the right license: deny.toml's
  // per-crate exceptions and the Apache-2.0-only GUI crates. A template or policy change that drops
  // one fails the release, not a later audit.
  const sections = licenseSections(rustNotices);
  for (const [license, crate] of [
    ["Mozilla Public License 2.0", "option-ext"],
    ["ISC License", "ring"],
    ["ISC License", "rustls-webpki"],
    ["ISC License", "untrusted"],
    ["Apache License 2.0", "tao"],
    ["Apache License 2.0", "wry"],
  ] as const) {
    if (!sections.some((section) => section.license === license && section.crates.has(crate))) {
      fail(`the notices do not list ${crate} under ${license}`);
    }
  }
  for (const required of ["### @tanstack/query-core", "### tailwindcss "]) {
    if (!notices.includes(required)) fail(`the notices lack ${JSON.stringify(required)}`);
  }

  // 3. The staged directory and its manifest.
  mkdirSync(artifactsDir, { recursive: true });
  const stage = join(artifactsDir, name);
  rmSync(stage, { recursive: true, force: true });
  mkdirSync(stage);
  cpSync(binary, join(stage, "kosse"));
  cpSync(join(repoRoot, "LICENSE"), join(stage, "LICENSE"));
  writeFileSync(join(stage, "THIRD_PARTY_NOTICES.md"), notices);
  const install = readFileSync(join(repoRoot, "packaging", "INSTALL.md"), "utf8");
  writeFileSync(join(stage, "INSTALL.md"), install.replaceAll("{{version}}", version));

  const commit = output("git", ["rev-parse", "HEAD"]).trim();
  const dirty = output("git", ["status", "--porcelain"]).trim() !== "";
  const files = ["kosse", "LICENSE", "THIRD_PARTY_NOTICES.md", "INSTALL.md"].map((path) => ({
    path,
    bytes: statSync(join(stage, path)).size,
    sha256: sha256(join(stage, path)),
  }));
  const manifest = {
    name: "kosse",
    version,
    target,
    commit,
    // Built from uncommitted changes: the commit alone does not reproduce it.
    dirty,
    rustc: output("rustc", ["--version"]).trim(),
    files,
  };
  writeFileSync(join(stage, "MANIFEST.json"), `${JSON.stringify(manifest, null, 2)}\n`);

  // 4. The archive (no AppleDouble files or extended attributes, numeric root ownership) and its
  //    checksum in `shasum -c` format.
  const archive = join(artifactsDir, tarball);
  rmSync(archive, { force: true });
  run(
    "/usr/bin/tar",
    [
      "-czf",
      archive,
      "--no-mac-metadata",
      "--no-xattrs",
      "--uid",
      "0",
      "--gid",
      "0",
      "-C",
      artifactsDir,
      name,
    ],
    { env: { ...process.env, COPYFILE_DISABLE: "1" } },
  );
  const digest = sha256(archive);
  writeFileSync(`${archive}.sha256`, `${digest}  ${tarball}\n`);
  run("/usr/bin/shasum", ["-a", "256", "-c", `${tarball}.sha256`], { cwd: artifactsDir });

  // 5. The Homebrew formula for exactly this archive (the release workflow commits it to the tap).
  const formula = readFileSync(join(repoRoot, "packaging", "homebrew", "kosse.rb.tmpl"), "utf8")
    .replaceAll("{{url}}", url)
    .replaceAll("{{version}}", version)
    .replaceAll("{{sha256}}", digest);
  writeFileSync(join(artifactsDir, "kosse.rb"), formula);

  rmSync(stage, { recursive: true });
  if (!existsSync(archive)) fail("no archive written");
  const size = statSync(archive).size;
  process.stdout.write(
    [
      "",
      `${archive}  ${size} B${dirty ? "  (built from uncommitted changes)" : ""}`,
      `${archive}.sha256  ${digest}`,
      `${join(artifactsDir, "kosse.rb")}  Homebrew formula${values.has("--url") ? "" : " (URL placeholder; pass --url)"}`,
      "",
    ].join("\n"),
  );
});

type Section = { license: string; crates: Set<string> };

/** The cargo-about output's `### <license>` sections with the crate names each lists. */
function licenseSections(markdown: string): Section[] {
  return markdown
    .split(/^### /m)
    .slice(1)
    .map((chunk) => {
      const [heading = "", ...lines] = chunk.split("\n");
      const crates = new Set<string>();
      for (const line of lines) {
        if (line.startsWith("```")) break;
        const crate = /^- (\S+) \d/.exec(line)?.[1];
        if (crate !== undefined) crates.add(crate);
      }
      return { license: heading.trim(), crates };
    });
}

function sorted(list: readonly string[]): string {
  return JSON.stringify(list.toSorted());
}

/** `value[key]` of a parsed TOML/JSON object, without trusting its shape. */
function field(value: unknown, key: string): unknown {
  return typeof value === "object" && value !== null && key in value
    ? Reflect.get(value, key)
    : undefined;
}

function strings(value: unknown, what: string): string[] {
  if (Array.isArray(value) && value.every((item) => typeof item === "string")) return value;
  fail(`${what} is not a list of strings`);
}

function assertSamePolicy(): void {
  const about: unknown = Bun.TOML.parse(readFileSync(join(repoRoot, "about.toml"), "utf8"));
  const deny: unknown = Bun.TOML.parse(readFileSync(join(repoRoot, "deny.toml"), "utf8"));
  const licenses = field(deny, "licenses");
  const accepted = strings(field(about, "accepted"), "about.toml accepted");
  if (sorted(accepted) !== sorted(strings(field(licenses, "allow"), "deny.toml allow"))) {
    fail("about.toml `accepted` differs from deny.toml [licenses] allow");
  }
  const aboutExceptions =
    typeof about === "object" && about !== null
      ? Object.keys(about)
          .filter((crate) => field(field(about, crate), "accepted") !== undefined)
          .map(
            (crate) => `${crate}=${sorted(strings(field(field(about, crate), "accepted"), crate))}`,
          )
          .toSorted()
      : [];
  const exceptions = field(licenses, "exceptions");
  const denyExceptions = (Array.isArray(exceptions) ? exceptions : [])
    .map((exception: unknown) => {
      const crate = field(exception, "crate");
      return `${String(crate)}=${sorted(strings(field(exception, "allow"), String(crate)))}`;
    })
    .toSorted();
  if (JSON.stringify(aboutExceptions) !== JSON.stringify(denyExceptions)) {
    fail(
      `about.toml per-crate licenses ${aboutExceptions.join(", ")} differ from deny.toml exceptions ${denyExceptions.join(", ")}`,
    );
  }
}
