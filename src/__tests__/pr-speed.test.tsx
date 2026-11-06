import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { tauriCommandFor } from "../host/client";
import { prefetchPrDetail, prTarget } from "../views/prDetails";
import { PrWorkspaceView } from "../views/PrWorkspaceView";
import { PullRequestsView } from "../views/PullRequestsView";
import {
  workspaceFixture,
  workspacePr,
} from "../../tests/support/pr-workspace-fixture";
import type { HostClient } from "../host";

function client() {
  return {
    pullRequestDetail: vi.fn().mockResolvedValue(workspaceFixture),
    pullRequestAction: vi.fn().mockResolvedValue({}),
  };
}

describe("PR view speed", () => {
  it("sends GitHub-bound methods to the off-main-thread command", () => {
    expect(tauriCommandFor("pr/detail")).toBe("host_rpc_detached");
    expect(tauriCommandFor("pr/action")).toBe("host_rpc_detached");
    expect(tauriCommandFor("pr/list")).toBe("host_rpc");
  });

  it("reads a URL's host, and falls back to github.com", () => {
    expect(
      prTarget({
        repo: "o/r",
        number: 1,
        url: "https://ghe.example/o/r/pull/1",
      }),
    ).toEqual({ repo: "o/r", number: 1, host: "ghe.example" });
    expect(prTarget({ repo: "o/r", number: 1, url: "not a url" }).host).toBe(
      "github.com",
    );
  });

  it("opens a prefetched PR from the cache with one GitHub read in flight", async () => {
    const host = client();
    const target = prTarget(workspacePr);
    prefetchPrDetail(host as unknown as HostClient, target);
    prefetchPrDetail(host as unknown as HostClient, target);
    await waitFor(() => expect(host.pullRequestDetail).toHaveBeenCalledOnce());
    await act(async () => {});

    render(
      <PrWorkspaceView
        pr={workspacePr}
        client={host as unknown as HostClient}
        onBack={vi.fn()}
        onOpenThread={vi.fn()}
      />,
    );
    // Painted straight from the cache: no loading placeholder at all.
    expect(
      screen.queryByRole("status", { name: /Loading conversation/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("tab", { name: /Files changed/ }),
    ).toBeInTheDocument();
  });

  it("only refreshes the board on back when the visit changed the PR", async () => {
    const onRefresh = vi.fn();
    render(
      <PullRequestsView
        pullRequests={[workspacePr]}
        client={client() as unknown as HostClient}
        onRefresh={onRefresh}
        onOpenThread={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Open pull request/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "← Pull requests" }),
    );
    expect(onRefresh).not.toHaveBeenCalled();
  });

  it("draws placeholder rows, not an empty board, while the host is asked", () => {
    render(
      <PullRequestsView pullRequests={[]} loading onOpenThread={vi.fn()} />,
    );
    expect(
      screen.getByRole("status", { name: "Loading pull requests" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("No pull requests here."),
    ).not.toBeInTheDocument();
  });
});
