import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HostClient } from "../host";
import { usePrDetails, PR_DETAIL_REFRESH_MS } from "../views/prDetails";
import { workspaceFixture } from "../../tests/support/pr-workspace-fixture";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const flush = async () => {
  await act(async () => {
    await Promise.resolve();
  });
};
const target = { repo: "acme/workspace", number: 42, host: "github.com" };
function mount() {
  const client = {
    pullRequestDetail: vi.fn().mockResolvedValue(workspaceFixture),
  };
  const writing = { current: false };
  const hook = renderHook(
    ({ number }) =>
      usePrDetails(
        client as unknown as HostClient,
        { ...target, number },
        writing,
      ),
    { initialProps: { number: 42 } },
  );
  return { ...hook, client, writing };
}
beforeEach(() => {
  vi.useFakeTimers();
  Object.defineProperty(document, "hidden", {
    configurable: true,
    value: false,
  });
});
afterEach(() => {
  vi.useRealTimers();
});

describe("live PR details", () => {
  it("updates conversation/checks on the timer without foreground loading", async () => {
    const { client, result } = mount();
    await flush();
    const next = { ...workspaceFixture, comments: [], checks: null };
    const pending = deferred<typeof next>();
    client.pullRequestDetail.mockReturnValueOnce(pending.promise);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
    });
    expect(result.current.loading).toBe(false);
    expect(result.current.refreshing).toBe(true);
    await act(async () => pending.resolve(next));
    expect(result.current.data).toEqual(next);
  });
  it("skips hidden windows and resumes on visibility/focus without overlapping", async () => {
    const { client } = mount();
    await flush();
    Object.defineProperty(document, "hidden", {
      configurable: true,
      value: true,
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS * 2);
      window.dispatchEvent(new Event("focus"));
    });
    expect(client.pullRequestDetail).toHaveBeenCalledTimes(1);
    Object.defineProperty(document, "hidden", {
      configurable: true,
      value: false,
    });
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      window.dispatchEvent(new Event("focus"));
    });
    expect(client.pullRequestDetail).toHaveBeenCalledTimes(2);
  });
  it("does not overlap slow reads and stops polling/listening after unmount", async () => {
    const { client, unmount } = mount();
    await flush();
    const pending = deferred<typeof workspaceFixture>();
    client.pullRequestDetail.mockReturnValueOnce(pending.promise);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS * 3);
    });
    expect(client.pullRequestDetail).toHaveBeenCalledTimes(2);
    unmount();
    await act(async () => pending.resolve(workspaceFixture));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS * 2);
      window.dispatchEvent(new Event("focus"));
    });
    expect(client.pullRequestDetail).toHaveBeenCalledTimes(2);
  });
  it("pauses while writing and drops an in-flight background response during the write", async () => {
    const { client, writing, result } = mount();
    await flush();
    const pending = deferred<typeof workspaceFixture>();
    client.pullRequestDetail.mockReturnValueOnce(pending.promise);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
    });
    writing.current = true;
    await act(async () =>
      pending.resolve({ ...workspaceFixture, comments: [] }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
    });
    expect(client.pullRequestDetail).toHaveBeenCalledTimes(2);
    expect(result.current.data?.comments).toHaveLength(1);
    await act(async () => {
      await result.current.refresh();
    });
    expect(client.pullRequestDetail).toHaveBeenCalledTimes(3);
  });
  it("announces a new head without retargeting the review until explicit refresh", async () => {
    const { client, result } = mount();
    await flush();
    const next = {
      ...workspaceFixture,
      pr: {
        ...workspaceFixture.pr,
        head: { ...workspaceFixture.pr.head, sha: "new-head" },
      },
    };
    client.pullRequestDetail.mockResolvedValue(next);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
    });
    expect(result.current.pendingHead).toBe("new-head");
    expect(result.current.data?.pr.head.sha).toBe(workspaceFixture.pr.head.sha);
    await act(async () => {
      await result.current.refresh();
    });
    expect(result.current.pendingHead).toBeNull();
    expect(result.current.data?.pr.head.sha).toBe("new-head");
  });
  it("keeps last good data on a failed read and recovers automatically", async () => {
    const { client, result } = mount();
    await flush();
    client.pullRequestDetail.mockImplementationOnce(() => {
      throw new Error("Offline");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
    });
    expect(result.current.refreshError).toBe("Offline");
    expect(result.current.data).toEqual(workspaceFixture);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
    });
    expect(result.current.refreshError).toBeNull();
    expect(client.pullRequestDetail).toHaveBeenCalledTimes(3);
  });
  it("ignores responses for a PR that is no longer selected", async () => {
    const { client, result, rerender } = mount();
    await flush();
    const pending = deferred<typeof workspaceFixture>();
    client.pullRequestDetail.mockReturnValueOnce(pending.promise);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
    });
    const next = { ...workspaceFixture, comments: [] };
    client.pullRequestDetail.mockResolvedValue(next);
    rerender({ number: 43 });
    await flush();
    await act(async () => pending.resolve(workspaceFixture));
    expect(result.current.data?.comments).toEqual([]);
  });
});
