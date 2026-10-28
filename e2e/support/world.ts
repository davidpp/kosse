// One disposable world per test file and browser: temporary Git projects, owned fixture
// listeners in them, an isolated KOSSE_CONFIG_DIR, and a `kosse serve` that may probe and signal
// only those fixtures.
import { mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import {
  release,
  startFixture,
  startKosse,
  type Fixture,
  type FixtureOptions,
  type Server,
} from "./processes.ts";
import { createRepo, git } from "./repos.ts";

export type World = {
  dir: string;
  configDir: string;
  /** The main checkout of `storefront` (branch main). */
  storefront: string;
  /** A linked worktree of the same repository, in another directory also named `storefront`. */
  worktree: string;
  /** A second project outside Git: a package directory. */
  billing: string;
  fixtures: Record<FixtureName, Fixture>;
  server: Server;
  /** Stops the server (by its handle) and starts a new instance on the same port. */
  restart: () => Promise<void>;
  stopServer: () => Promise<void>;
  startServer: () => Promise<void>;
  pids: () => Set<number>;
  dispose: () => Promise<void>;
};

export type FixtureName = "web" | "api" | "docs" | "stubborn" | "worker" | "db" | "gateway";

/** A fake credential assembled at run time, so no secret-shaped literal sits in the source. */
const fakeToken = ["kosse", "fixture", "token", "0000"].join("-");

/**
 * `showcase` (README screenshots) gives the fixtures plausible page titles, the usual ports when
 * they are free, one busy process and a plain (unsupervised) worker. The functional suite keeps
 * the default title, whose entity and markup test escaping.
 */
export async function createWorld(options: { showcase?: boolean } = {}): Promise<World> {
  const showcase = options.showcase ?? false;
  // `/tmp`, not the per-user temporary directory, keeps machine-specific paths out of screenshots.
  const dir = await realpath(await mkdtemp(join("/tmp", "kosse-e2e-")));
  const configDir = join(dir, "config");
  const bin = join(dir, "bin");
  const storefront = join(dir, "work", "storefront");
  const worktree = join(dir, "work", "worktrees", "storefront");
  const billing = join(dir, "work", "billing-api");
  const started: Fixture[] = [];

  try {
    await createRepo(storefront, ["web", "api", "docs", "worker"]);
    await mkdir(join(dir, "work", "worktrees"), { recursive: true });
    git(storefront, "worktree", "add", "-q", "-b", "checkout-redesign", worktree);
    await mkdir(billing, { recursive: true });
    await writeFile(
      join(billing, "package.json"),
      `${JSON.stringify({ name: "billing-api", private: true })}\n`,
    );

    const look = (title: string, ports: number[]) => (showcase ? { title, ports } : {});
    const specs: Record<FixtureName, FixtureOptions> = {
      web: {
        name: "vite",
        cwd: join(storefront, "apps", "web"),
        serve: "title",
        args: showcase ? [] : ["--port", "auto", `--api-token=${fakeToken}`],
        ...look("Storefront", [5173]),
      },
      api: {
        name: "node",
        cwd: join(storefront, "apps", "api"),
        serve: "not-found",
        listen: ["127.0.0.1", "127.0.0.1"],
        args: ["server.js"],
        ...look("", [3000, 3001]),
      },
      docs: {
        name: "astro",
        cwd: join(storefront, "apps", "docs"),
        serve: "title",
        args: ["dev"],
        ...look("Storefront Docs", [4321]),
      },
      stubborn: {
        name: "next-server",
        cwd: join(worktree, "apps", "web"),
        serve: "title",
        ignoreTerm: true,
        busy: showcase,
        args: ["dev"],
        ...look("Storefront (checkout redesign)", [3002]),
      },
      worker: {
        name: "queue-worker",
        cwd: join(storefront, "apps", "worker"),
        supervise: !showcase,
        args: ["--watch"],
      },
      db: { name: "postgres", cwd: billing, args: ["-D", "data"], ...look("", [5432]) },
      gateway: {
        name: "caddy",
        cwd: billing,
        serve: "tls",
        listen: ["127.0.0.1,::1"],
        args: ["run"],
        ...look("", [8443]),
      },
    };
    const start = async (name: FixtureName) => {
      const fixture = await startFixture(bin, specs[name]);
      started.push(fixture);
      return fixture;
    };
    const fixtures: Record<FixtureName, Fixture> = {
      web: await start("web"),
      api: await start("api"),
      docs: await start("docs"),
      stubborn: await start("stubborn"),
      worker: await start("worker"),
      db: await start("db"),
      gateway: await start("gateway"),
    };

    // `docs` is deliberately outside `--act-only`: stopping it must be refused. The supervised
    // worker's children are never probed (their PIDs are not known up front).
    const act = [
      fixtures.web,
      fixtures.api,
      fixtures.stubborn,
      fixtures.worker,
      fixtures.db,
      fixtures.gateway,
    ];
    const probe = [fixtures.web, fixtures.api, fixtures.docs, fixtures.stubborn, fixtures.gateway];
    if (showcase) probe.push(fixtures.worker);
    const serve = (port?: number) =>
      startKosse({
        cwd: dir,
        configDir,
        probe: probe.map((fixture) => fixture.pid),
        act: act.map((fixture) => fixture.pid),
        port,
      });
    const world: World = {
      dir,
      configDir,
      storefront,
      worktree,
      billing,
      fixtures,
      server: await serve(),
      restart: async () => {
        await world.stopServer();
        await world.startServer();
      },
      stopServer: () => release(world.server),
      startServer: async () => {
        world.server = await serve(world.server.port);
      },
      pids: () => new Set(started.map((fixture) => fixture.pid)),
      dispose: async () => {
        await release(world.server);
        await Promise.all(started.map((fixture) => release(fixture)));
        await rm(dir, { recursive: true, force: true });
      },
    };
    return world;
  } catch (error) {
    await Promise.all(started.map((fixture) => release(fixture)));
    await rm(dir, { recursive: true, force: true });
    throw error;
  }
}
