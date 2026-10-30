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
  type ServerOptions,
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
  /**
   * A Vite + workerd stack with a central profile: the site's inspector and every workerd port
   * are details endpoints.
   */
  marketing: string;
  fixtures: Record<FixtureName, Fixture>;
  server: Server;
  /** Stops the server (by its handle) and starts a new instance on the same port. */
  restart: () => Promise<void>;
  stopServer: () => Promise<void>;
  startServer: () => Promise<void>;
  pids: () => Set<number>;
  dispose: () => Promise<void>;
};

export type FixtureName =
  | "web"
  | "api"
  | "docs"
  | "stubborn"
  | "worker"
  | "db"
  | "gateway"
  | "site"
  | "workerd";

/** A fake credential assembled at run time, so no secret-shaped literal sits in the source. */
const fakeToken = ["kosse", "fixture", "token", "0000"].join("-");

/**
 * `showcase` (README screenshots) gives the fixtures plausible page titles, the usual ports when
 * they are free, one busy process and a plain (unsupervised) worker. The functional suite keeps
 * the default title, whose entity and markup test escaping.
 */
export async function createWorld(
  options: {
    showcase?: boolean;
    /** How the instance starts (default `kosse serve`); `bun run app` starts the menu-bar app. */
    start?: (options: ServerOptions) => Promise<Server>;
    /** Fixture lifetime cap when their owner never closes their stdin (default 900 s). */
    fixtureLifetimeSecs?: number;
  } = {},
): Promise<World> {
  const showcase = options.showcase ?? false;
  const startServer = options.start ?? startKosse;
  // `/tmp`, not the per-user temporary directory, keeps machine-specific paths out of screenshots.
  const dir = await realpath(await mkdtemp(join("/tmp", "kosse-e2e-")));
  const configDir = join(dir, "config");
  const bin = join(dir, "bin");
  const storefront = join(dir, "work", "storefront");
  const worktree = join(dir, "work", "worktrees", "storefront");
  const billing = join(dir, "work", "billing-api");
  const marketing = join(dir, "work", "marketing-site");
  const started: Fixture[] = [];

  try {
    await createRepo(storefront, ["web", "api", "docs", "worker"]);
    await createRepo(marketing, []);
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
      // Vite with its inspector: the profile lists the site in the summary, the inspector in
      // details.
      site: {
        name: "vite",
        cwd: marketing,
        serve: "title",
        listen: ["127.0.0.1", "127.0.0.1"],
        args: ["dev"],
        ...look("Marketing site", [5180, 9229]),
      },
      // workerd's ports change on every start; `otherEndpoints` keeps them all in details. An
      // internal 500 is reachable HTTP, never a failure of the site.
      workerd: {
        name: "workerd",
        cwd: marketing,
        serve: "error",
        listen: ["127.0.0.1", "127.0.0.1"],
        args: ["serve", "--experimental"],
      },
    };
    const start = async (name: FixtureName) => {
      const fixture = await startFixture(bin, {
        ...specs[name],
        maxLifetimeSecs: options.fixtureLifetimeSecs,
      });
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
      site: await start("site"),
      workerd: await start("workerd"),
    };
    const [sitePort, inspectorPort] = fixtures.site.ports;
    await mkdir(configDir, { recursive: true });
    await writeFile(
      join(configDir, "config.json"),
      `${JSON.stringify({
        version: 1,
        projects: {
          [marketing]: {
            version: 1,
            services: {
              site: {
                name: "Marketing site",
                match: { cwd: ".", process: "vite" },
                endpoints: {
                  [String(sitePort)]: { name: "Site" },
                  [String(inspectorPort)]: { name: "Inspector", show: "details" },
                },
              },
              worker: {
                name: "Worker runtime",
                match: { cwd: ".", process: "workerd" },
                otherEndpoints: { show: "details" },
              },
            },
          },
        },
      })}\n`,
    );

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
    const probe = [
      fixtures.web,
      fixtures.api,
      fixtures.docs,
      fixtures.stubborn,
      fixtures.gateway,
      fixtures.site,
      fixtures.workerd,
    ];
    if (showcase) probe.push(fixtures.worker);
    const serve = (port?: number) =>
      startServer({
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
      marketing,
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
