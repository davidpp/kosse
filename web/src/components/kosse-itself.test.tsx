import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";

import { buildDashboard } from "@/model/services";
import { service, snapshot } from "@/test/snapshots";

import { ProjectCard, type RowActions } from "./project-card";
import { StopDialog } from "./stop-dialog";

const kosse = service(7, { name: "kosse", ports: [47831] });
const other = service(8, { name: "vite", ports: [5173] });

function actions(kossePid: number | null): RowActions {
  return {
    inspect: noop,
    stop: noop,
    customize: noop,
    tryHttp: noop,
    tried: new Map(),
    kossePid,
  };
}

const noop = () => undefined;

// jsdom has no modal dialogs; the real ones are checked in the shipped window by `test:window`.
function showModal(this: HTMLDialogElement) {
  this.setAttribute("open", "");
}
function close(this: HTMLDialogElement) {
  this.removeAttribute("open");
}
beforeAll(() => {
  if (typeof HTMLDialogElement.prototype.showModal !== "function") {
    HTMLDialogElement.prototype.showModal = showModal;
    HTMLDialogElement.prototype.close = close;
  }
});

describe("Kossé's own row", () => {
  it("has a disabled Stop with the reason; other rows keep theirs", () => {
    const current = snapshot(1, "a", [kosse, other]);
    if (current.observation === null) throw new Error("no observation");
    const [group] = buildDashboard(current.observation, [], {
      terms: [],
      allListeners: true,
    }).groups;
    if (group === undefined) throw new Error("no group");
    render(<ProjectCard group={group} actions={actions(7)} />);

    const own = screen.getByRole("listitem", { name: /^kosse, port 47831/ });
    expect(within(own).getByText("this Kossé")).toBeInTheDocument();
    const stop = within(own).getByRole("button", { name: /^Stop/ });
    expect(stop).toBeDisabled();
    expect(stop).toHaveAttribute("title", expect.stringMatching(/Kossé itself/));

    const vite = screen.getByRole("listitem", { name: /^vite, port 5173/ });
    expect(within(vite).getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("a Stop deep link to it explains instead of confirming", () => {
    const current = { ...snapshot(1, "a", [kosse, other]), instancePid: 7 };
    render(
      <QueryClientProvider client={new QueryClient()}>
        <StopDialog identity={`7.${1_700_000_007}`} snapshot={current} onClose={noop} />
      </QueryClientProvider>,
    );

    expect(screen.getByRole("heading", { name: "Kossé cannot stop itself" })).toBeInTheDocument();
    expect(screen.getByText(/Nothing was sent/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Stop process/ })).not.toBeInTheDocument();
  });
});
