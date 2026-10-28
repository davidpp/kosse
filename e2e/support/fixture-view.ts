// For README screenshots only: a loopback proxy in front of the real Kossé server that removes
// every service the suite did not start from the snapshot and the event stream. The dashboard
// still renders real backend data, but nothing from the machine running the suite (its own
// projects, process names or paths) can appear in a committed image. Functional tests talk to
// Kossé directly.
import { createServer, request, type IncomingMessage, type Server } from "node:http";

import type { Snapshot } from "../../web/src/generated/contracts.ts";

export type FixtureView = { url: string; close: () => Promise<void> };

export async function fixtureView(
  upstreamPort: number,
  owned: () => Set<number>,
): Promise<FixtureView> {
  const server: Server = createServer((incoming, outgoing) => {
    const headers = { ...incoming.headers, host: `127.0.0.1:${upstreamPort}` };
    if (headers.origin !== undefined) headers.origin = `http://127.0.0.1:${upstreamPort}`;
    const forward = request(
      {
        host: "127.0.0.1",
        port: upstreamPort,
        path: incoming.url,
        method: incoming.method,
        headers,
      },
      (answer: IncomingMessage) => {
        const path = incoming.url ?? "";
        if (path === "/api/snapshot" || path === "/api/refresh") {
          const chunks: Buffer[] = [];
          answer.on("data", (chunk: Buffer) => chunks.push(chunk));
          answer.on("end", () => {
            const body = filterJson(Buffer.concat(chunks).toString("utf8"), owned());
            const { "content-length": _, ...rest } = answer.headers;
            outgoing.writeHead(answer.statusCode ?? 502, rest);
            outgoing.end(body);
          });
        } else if (path === "/api/events") {
          outgoing.writeHead(answer.statusCode ?? 502, answer.headers);
          let pending = "";
          answer.on("data", (chunk: Buffer) => {
            pending += chunk.toString("utf8");
            let end = pending.indexOf("\n\n");
            while (end !== -1) {
              const event = pending.slice(0, end);
              pending = pending.slice(end + 2);
              outgoing.write(
                `${event.replace(/^data: (.*)$/m, (_, json: string) => `data: ${filterJson(json, owned())}`)}\n\n`,
              );
              end = pending.indexOf("\n\n");
            }
          });
          answer.on("end", () => outgoing.end());
        } else {
          outgoing.writeHead(answer.statusCode ?? 502, answer.headers);
          answer.pipe(outgoing);
        }
      },
    );
    forward.on("error", () => outgoing.destroy());
    incoming.pipe(forward);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("proxy did not bind");
  const { port } = address;
  return {
    url: `http://127.0.0.1:${port}`,
    close: () =>
      new Promise((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
}

function filterJson(text: string, owned: Set<number>): string {
  let snapshot: unknown;
  try {
    snapshot = JSON.parse(text);
  } catch {
    return text;
  }
  if (!hasObservation(snapshot)) return text;
  const { observation } = snapshot;
  const services = observation.services.filter(
    (service) =>
      owned.has(service.process.identity.pid) ||
      (service.process.parentPid !== null && owned.has(service.process.parentPid)),
  );
  const roots = new Set(services.map((service) => service.project));
  return JSON.stringify({
    ...snapshot,
    observation: {
      ...observation,
      services,
      projects: observation.projects.filter((project) => roots.has(project.root)),
      profiles: observation.profiles.filter((profile) => roots.has(profile.root)),
      warnings: observation.warnings.filter(
        (warning) => "pid" in warning && owned.has(warning.pid),
      ),
    },
  });
}

function hasObservation(
  value: unknown,
): value is Snapshot & { observation: NonNullable<Snapshot["observation"]> } {
  return (
    typeof value === "object" &&
    value !== null &&
    "observation" in value &&
    typeof value.observation === "object" &&
    value.observation !== null &&
    "services" in value.observation &&
    Array.isArray(value.observation.services)
  );
}
