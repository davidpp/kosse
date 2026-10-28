import { useQuery } from "@tanstack/react-query";

import { healthQuery } from "@/api/health";
import { ServerStatus } from "@/components/server-status";

/**
 * The dashboard has one view, at `/` (and `/index.html`, which the server also serves). The
 * server answers every extensionless path with this page, so any other path gets an honest
 * not-found instead of a dashboard under a misleading URL.
 */
export function viewFor(pathname: string): "dashboard" | "not-found" {
  return pathname === "/" || pathname === "/index.html" ? "dashboard" : "not-found";
}

export function App({ pathname }: { pathname: string }) {
  return (
    <div className="flex min-h-screen flex-col">
      <header className="border-b border-line bg-surface">
        <div className="mx-auto flex h-11 w-full max-w-5xl items-center gap-3 px-4">
          <h1 className="font-semibold tracking-tight">Kossé</h1>
          <p className="hidden min-w-0 truncate text-ink-muted sm:block">
            Development services running on this machine.
          </p>
        </div>
      </header>
      <main className="mx-auto w-full max-w-5xl flex-1 px-4 py-5">
        {viewFor(pathname) === "dashboard" ? <Dashboard /> : <NotFound />}
      </main>
    </div>
  );
}

function Dashboard() {
  const health = useQuery(healthQuery);
  return <ServerStatus query={health} onRetry={() => void health.refetch()} />;
}

function NotFound() {
  return (
    <p className="text-ink-muted">
      Nothing lives at this address.{" "}
      <a
        href="/"
        className="text-ink underline decoration-line underline-offset-4 hover:decoration-ink"
      >
        Back to the dashboard
      </a>
    </p>
  );
}
