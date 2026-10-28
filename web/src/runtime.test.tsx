import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { CircleCheck } from "lucide-react";
import { StrictMode, type JSX } from "react";
import { describe, expect, it, vi } from "vitest";

// Tests must exercise the runtime that ships. Development React renders every component twice
// under StrictMode; Redact documents that StrictMode renders children once. Two renders mean
// Testing Library (or the code under test) reached real React instead of Redact.
describe("test runtime", () => {
  it("renders through Redact, like the shipped bundle", () => {
    const body = vi.fn<() => JSX.Element>(() => <p>probe</p>);
    function Probe() {
      return body();
    }

    render(
      <StrictMode>
        <Probe />
      </StrictMode>,
    );

    expect(screen.getByText("probe")).toBeInTheDocument();
    expect(body).toHaveBeenCalledOnce();
  });

  // A dependency loaded with a second copy of React (real React) calls its hooks against a
  // dispatcher Redact never installs, so rendering it throws. This drives the hook-using
  // dependencies the app ships: TanStack Query (context, useSyncExternalStore) and Lucide (context).
  it("runs dependencies' hooks on the same Redact copy", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <StrictMode>
        <QueryClientProvider client={queryClient}>
          <DependencyProbe />
        </QueryClientProvider>
      </StrictMode>,
    );

    expect(await screen.findByText("ready")).toBeInTheDocument();
    expect(screen.getByTestId("icon").tagName.toLowerCase()).toBe("svg");
    queryClient.clear();
  });
});

function DependencyProbe() {
  const query = useQuery({ queryKey: ["probe"], queryFn: () => Promise.resolve("ready") });
  return (
    <p>
      <CircleCheck aria-hidden data-testid="icon" />
      {query.data ?? "pending"}
    </p>
  );
}
