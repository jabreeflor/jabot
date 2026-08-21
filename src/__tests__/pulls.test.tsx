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
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { PullRequestsView } from "../views/PullRequestsView";
import { prRow } from "../views/pulls";
import type { PullRequestView } from "../host";

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
