/**
 * The PR board on host data (#28).
 *
 * Two claims, and they are different in kind. The first is a *mapping*: what
 * the host serves — facts about a pull request — becomes the props the
 * prototype's card was written against, including the copy the host
 * deliberately does not compose. The second is about what the view does when
 * the poll cannot reach GitHub: the rows are linkage, they needed no
 * credential, and they must not disappear because a token did.
 */
import { render, renderHook, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PullRequestsView } from "../views/PullRequestsView";
import { mergeBoards, mineRow, prRow, usePullRequests } from "../views/pulls";
import type {
  GithubPullRequestView,
  HostClient,
  PullRequestView,
} from "../host";

function wire(over: Partial<PullRequestView> = {}): PullRequestView {
  return {
    id: "pr-1",
    threadId: "t-auth",
    threadTitle: "Auth migration",
    threadState: "active",
    provider: "github",
    forgeHost: "github.com",
    repo: "jabreeflor/jabot",
    number: 23,
    url: "https://github.com/jabreeflor/jabot/pull/23",
    title: "Migrate auth to sessions",
    status: "open",
    checkState: "passing",
    headRef: "jabot/t-auth",
    baseRef: "main",
    additions: 214,
    deletions: 96,
    changedFiles: 3,
    checks: [
      { label: "tests", state: "passing" },
      { label: "lint", state: "passing" },
    ],
    updatedAt: "2026-08-21T09:14:02Z",
    detectedVia: "stdout",
    polledAt: "2026-08-21T09:20:00Z",
    ...over,
  };
}

describe("mapping a host row onto the card", () => {
  it("keeps every fact the row draws, and the thread it came from", () => {
    const pr = prRow(wire());
    expect(pr.threadId).toBe("t-auth");
    expect(pr.repo).toBe("jabreeflor/jabot");
    expect(pr.number).toBe(23);
    expect(pr.additions).toBe(214);
    expect(pr.filesChanged).toBe(3);
    expect(pr.headRef).toBe("jabot/t-auth");
    // GitHub's clock, not the poll's — the card's "38m ago" must not move
    // because we asked again.
    expect(pr.updatedAt).toBe("2026-08-21T09:14:02Z");
    expect(pr.detail?.bullets).toContain("Opened by “Auth migration”");
  });

  /**
   * The summary line is ordered by what the human can act on, and the
   * prototype's own copy is the target: "2 of 3 checks done", "from folded
   * session".
   */
  it("says the most actionable true thing", () => {
    expect(prRow(wire({ status: "merged" })).summary).toBe("merged");
    expect(
      prRow(wire({ reviewState: "changes_requested" })).summary,
    ).toBe("changes requested");
    expect(
      prRow(
        wire({
          checkState: "failing",
          checks: [
            { label: "tests", state: "failing" },
            { label: "lint", state: "passing" },
          ],
        }),
      ).summary,
    ).toBe("tests failed");
    expect(
      prRow(
        wire({
          checkState: "running",
          checks: [
            { label: "tests", state: "passing" },
            { label: "lint", state: "passing" },
            { label: "build", state: "running" },
          ],
        }),
      ).summary,
    ).toBe("2 of 3 checks done");
    expect(prRow(wire({ threadState: "folded" })).summary).toBe(
      "from folded session",
    );
    expect(prRow(wire()).summary).toBe("checks green");
  });

  /**
   * A row linked but never polled is the state of every PR on a machine with
   * no `gh` login. It has no title, no diffstat and no checks — so it must say
   * so rather than draw an empty card as if that were GitHub's answer.
   */
  it("does not pretend a never-polled row has been checked", () => {
    const pr = prRow(wire({ polledAt: undefined, title: "", changedFiles: 0 }));
    expect(pr.title).toBe("jabreeflor/jabot #23");
    expect(pr.summary).toBe("not checked with GitHub yet");
    expect(pr.detail).toBeUndefined();
    expect(pr.filesChanged).toBeUndefined();
  });

  /** No checks configured is not the same as checks that passed. */
  it("distinguishes no checks from green checks", () => {
    const none = prRow(wire({ checkState: undefined, checks: [] }));
    expect(none.checkState).toBeNull();
    expect(none.detail?.bullets).toContain(
      "No checks configured for this repository",
    );
  });

  /**
   * There is no Merge button. Merging from JaBot is a host action nobody has
   * built, and a button that opened GitHub while saying "Merge" would be worse
   * than the link that is honest about it.
   */
  it("offers the two actions that actually work", () => {
    const ids = prRow(wire()).detail?.actions.map((action) => action.id);
    expect(ids).toEqual(["diff", "reopen"]);
    // A PR whose thread has been purged keeps the GitHub link and loses the
    // one that would go nowhere.
    const orphan = prRow(wire({ threadState: "deleted" }));
    expect(orphan.detail?.actions.map((action) => action.id)).toEqual(["diff"]);
  });
});

describe("when GitHub cannot be reached", () => {
  it("keeps the board and says what would fix it", () => {
    render(
      <PullRequestsView
        pullRequests={[prRow(wire({ polledAt: undefined }))]}
        unavailable={{
          host: "github.com",
          reason: "gh_missing",
          detail: "GitHub CLI (gh) is not installed.",
          remedy: "brew install gh",
        }}
        onOpenThread={() => {}}
      />,
    );
    // The rows are linkage; they needed no credential and are still true.
    expect(screen.getByText(/jabreeflor\/jabot #23/)).toBeTruthy();
    const notice = screen.getByRole("status");
    expect(notice.textContent).toContain("GitHub CLI (gh) is not installed.");
    expect(notice.textContent).toContain("brew install gh");
  });
});

/**
 * The other half of the board (#28): what the *user* has open, wherever it
 * lives, once they have signed in. Same card, two differences that matter — a
 * row that GitHub answered about has always been polled, and a row written
 * somewhere else has no session to reopen.
 */
function mineWire(
  over: Partial<GithubPullRequestView> = {},
): GithubPullRequestView {
  return {
    id: "github:someone-else/infra#7",
    provider: "github",
    forgeHost: "github.com",
    repo: "someone-else/infra",
    number: 7,
    url: "https://github.com/someone-else/infra/pull/7",
    title: "Bump the pinned toolchain",
    status: "open",
    checkState: "passing",
    reviewState: "review_required",
    headRef: "toolchain",
    baseRef: "trunk",
    additions: 4,
    deletions: 4,
    changedFiles: 1,
    checks: [{ label: "verify", state: "passing" }],
    updatedAt: "2026-08-20T17:45:00Z",
    ...over,
  };
}

describe("one of your own pull requests, from GitHub", () => {
  it("draws the same card, minus the thread there never was", () => {
    const pr = mineRow(mineWire());
    expect(pr.repo).toBe("someone-else/infra");
    expect(pr.number).toBe(7);
    expect(pr.threadId).toBeUndefined();
    expect(pr.filesChanged).toBe(1);
    // Always fresh from GitHub, so never "not checked with GitHub yet".
    expect(pr.summary).not.toBe("not checked with GitHub yet");
    expect(pr.detail).toBeDefined();
    // No thread means no button that would go nowhere.
    expect(pr.detail?.actions.map((action) => action.id)).toEqual(["diff"]);
    expect(pr.detail?.bullets).toContain("Waiting for a review");
  });

  it("keeps the session on a PR that a session here did open", () => {
    const pr = mineRow(
      mineWire({
        repo: "jabreeflor/jabot",
        number: 23,
        threadId: "t-auth",
        threadTitle: "Auth migration",
        threadState: "folded",
      }),
    );
    expect(pr.threadId).toBe("t-auth");
    expect(pr.detail?.actions.map((action) => action.id)).toEqual([
      "diff",
      "reopen",
    ]);
    expect(pr.detail?.bullets).toContain("Opened by “Auth migration”");
  });
});

describe("folding the two boards into one list", () => {
  it("shows each pull request once, newest first", () => {
    const merged = mergeBoards(
      [prRow(wire())],
      [
        mineRow(mineWire()),
        mineRow(mineWire({ number: 23, repo: "jabreeflor/jabot" })),
      ],
    );
    // Three rows in, two out: #23 is on both boards and is one pull request.
    expect(merged).toHaveLength(2);
    expect(merged.map((pr) => pr.number)).toEqual([23, 7]);
  });

  it("keeps the linked row's identity, so signing in does not redraw the board", () => {
    const merged = mergeBoards(
      [prRow(wire())],
      [
        mineRow(
          mineWire({
            number: 23,
            repo: "jabreeflor/jabot",
            title: "Fresher title",
          }),
        ),
      ],
    );
    // React keys off `id`; GitHub's answer is the fresher one.
    expect(merged[0].id).toBe("pr-1");
    expect(merged[0].title).toBe("Fresher title");
  });

  it("does not drop a merged PR that pr/mine never mentions", () => {
    // `pr/mine` asks only for what is open, so the Merged tab has to survive
    // somebody signing in.
    const merged = mergeBoards(
      [prRow(wire({ status: "merged" }))],
      [mineRow(mineWire())],
    );
    expect(merged.map((pr) => pr.status).sort()).toEqual(["merged", "open"]);
  });

  it("is the linked board alone before anything else has answered", () => {
    const linked = [prRow(wire())];
    expect(mergeBoards(linked, null)).toEqual(linked);
  });
});

describe("what the board asks the host for", () => {
  function stub() {
    const listPullRequests = vi.fn(async () => ({
      pullRequests: [wire()],
    }));
    const myPullRequests = vi.fn(async () => ({
      account: "octocat",
      pullRequests: [mineWire()],
    }));
    return {
      listPullRequests,
      myPullRequests,
      client: { listPullRequests, myPullRequests } as unknown as HostClient,
    };
  }

  it("never asks GitHub who you are when nobody is signed in", async () => {
    const { client, listPullRequests, myPullRequests } = stub();
    const { result } = renderHook(() => usePullRequests(client, false));

    await waitFor(() => expect(result.current.pullRequests).toHaveLength(1));
    expect(listPullRequests).toHaveBeenCalled();
    // `pr/mine` is a subprocess and a network round trip that can only answer
    // "log in first". Spending one every poll to be told so is the cost this
    // gate exists to avoid.
    expect(myPullRequests).not.toHaveBeenCalled();
    expect(result.current.account).toBeNull();
  });

  it("folds your own pull requests in the moment there is a login", async () => {
    const { client, myPullRequests } = stub();
    const { result } = renderHook(() => usePullRequests(client, true));

    await waitFor(() => expect(result.current.pullRequests).toHaveLength(2));
    expect(myPullRequests).toHaveBeenCalled();
    expect(result.current.account).toBe("octocat");
    expect(result.current.pullRequests?.map((pr) => pr.number)).toEqual([
      23, 7,
    ]);
  });
});
