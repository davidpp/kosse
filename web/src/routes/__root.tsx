import type { QueryClient } from "@tanstack/react-query";
import { Link, Outlet, createRootRouteWithContext } from "@tanstack/react-router";

export interface RouterContext {
  queryClient: QueryClient;
}

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
  notFoundComponent: NotFound,
});

function RootLayout() {
  return (
    <main className="mx-auto flex min-h-screen max-w-3xl flex-col gap-8 px-6 py-12">
      <header>
        <h1 className="font-heading text-2xl font-semibold tracking-tight">Stackscan</h1>
        <p className="text-sm text-muted-foreground">
          Development services running on this machine.
        </p>
      </header>
      <Outlet />
    </main>
  );
}

function NotFound() {
  return (
    <p className="text-sm text-muted-foreground">
      Nothing lives at this address.{" "}
      <Link to="/" className="text-foreground underline underline-offset-4">
        Back to the dashboard
      </Link>
    </p>
  );
}
