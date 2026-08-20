/**
 * Row status is where the two-layer model from #5 becomes visible: a thread's
 * `state` says whether you can see it, its latest run says what the machine is
 * doing. When the two disagree, visibility wins.
 */
import { describe, expect, it } from "vitest";

import { inboxTag, prTag, threadStatus } from "../components/status";
import type { PullRequest, ThreadSummary } from "../components/types";

function thread(over: Partial<ThreadSummary> = {}): ThreadSummary {
  return {
    id: "t1",
    folderId: "f1",
    botId: "code",
    harnessId: "claude",
    title: "Auth migration",
    state: "active",
    foldPolicy: "default",
    runState: null,
    ...over,
  };
}

function pr(over: Partial<PullRequest> = {}): PullRequest {
  return {
    id: "pr1",
    threadId: "t1",
    repo: "jabot-app",
    number: 23,
    url: "https://example.invalid/23",
    title: "Migrate auth to sessions",
    status: "open",
    checkState: "passing",
    updatedAt: new Date().toISOString(),
    additions: 214,
    deletions: 96,
    ...over,
  };
}

describe("threadStatus", () => {
  it("says sleeping for a folded thread even while its run continues", () => {
    expect(threadStatus(thread({ state: "folded", runState: "running" }))).toEqual(
      { label: "sleeping", tone: "quiet" },
    );
  });

  it("reports the latest run for a visible thread", () => {
    expect(threadStatus(thread({ runState: "running" })).tone).toBe("running");
    expect(threadStatus(thread({ runState: "succeeded" }))).toEqual({
      label: "done",
      tone: "ok",
    });
    expect(threadStatus(thread({ runState: "failed" })).tone).toBe("bad");
    expect(threadStatus(thread({ runState: "lost" })).tone).toBe("bad");
  });

  it("treats needs_you as attention, not as failure", () => {
    expect(threadStatus(thread({ runState: "needs_you" }))).toEqual({
      label: "needs you",
      tone: "running",
    });
  });

  it("has something to say about a thread that has never run", () => {
    expect(threadStatus(thread({ runState: null })).label).toBe("idle");
  });

  it("shows the outcome of a resurfaced thread, not the word resurfaced", () => {
    expect(
      threadStatus(thread({ state: "resurfaced", runState: "succeeded" })).label,
    ).toBe("done");
  });
});

describe("inboxTag", () => {
  it("separates what wants you from what merely finished", () => {
    expect(inboxTag("needs_you").tone).toBe("needs");
    expect(inboxTag("judgment_call").tone).toBe("needs");
    expect(inboxTag("done").tone).toBe("done");
    expect(inboxTag("folded")).toEqual({ label: "SLEEPING", tone: "quiet" });
    expect(inboxTag("lost").tone).toBe("bad");
  });
});

describe("prTag", () => {
  it("does not ask for review while checks are still running", () => {
    expect(prTag(pr({ checkState: "running" })).label).toBe("CHECKS RUNNING");
    expect(prTag(pr({ checkState: "failing" })).tone).toBe("bad");
    expect(prTag(pr()).label).toBe("NEEDS REVIEW");
  });

  it("lets status win over checks once the PR has landed or is a draft", () => {
    expect(prTag(pr({ status: "merged", checkState: "running" })).label).toBe(
      "MERGED",
    );
    expect(prTag(pr({ status: "draft" })).label).toBe("DRAFT");
  });
});
