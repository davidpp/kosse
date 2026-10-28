import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ServerStatus } from "./server-status";

describe("ServerStatus", () => {
  it("shows the connected server version", () => {
    render(
      <ServerStatus
        query={{
          status: "success",
          data: { name: "stackscan", version: "1.2.3" },
        }}
        onRetry={vi.fn<() => void>()}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent("Connected to stackscan 1.2.3");
  });

  it("reports an unreachable server and retries on request", async () => {
    const retry = vi.fn<() => void>();
    render(
      <ServerStatus
        query={{
          status: "error",
          error: new Error("Health check failed with HTTP 502"),
          isFetching: false,
        }}
        onRetry={retry}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("Health check failed with HTTP 502");
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retry).toHaveBeenCalledOnce();
  });

  it("blocks a second retry while one is in flight", async () => {
    const retry = vi.fn<() => void>();
    render(
      <ServerStatus
        query={{ status: "error", error: new Error("connection refused"), isFetching: true }}
        onRetry={retry}
      />,
    );

    const button = screen.getByRole("button", { name: "Retrying…" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    // It stays focusable, so keyboard focus is not dropped while the retry runs.
    button.focus();
    expect(button).toHaveFocus();
    await userEvent.click(button);
    await userEvent.keyboard("{Enter}");
    expect(retry).not.toHaveBeenCalled();
  });
});
