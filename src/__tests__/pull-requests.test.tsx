/**
 * Pull Requests. The ordering claim is the one worth testing: a PR whose checks
 * are still running is not review-ready, and a merged or draft PR should never
 * shout. Every PR here was opened by a session, so "Reopen thread" always has a
 * thread to go to.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { PullRequestsView } from "../views/PullRequestsView";
import type { PullRequest } from "../components/types";

const NOW = new Date("2026-08-20T14:30:00Z");
const at = (minutes: number) =>
  new Date(NOW.getTime() - minutes * 60_000).toISOString();

const PRS: PullRequest[] = [
  {
    id: "pr-23",
    threadId: "auth",
    provider: "github",
    repo: "jabot-app",
    number: 23,
    url: "https://example.invalid/23",
    title: "Migrate auth to sessions",
    status: "open",
    checkState: "passing",
    updatedAt: at(38),
    additions: 214,
    deletions: 96,
    headRef: "auth/sessions",
    baseRef: "main",
    filesChanged: 3,
    detail: {
      checks: [
        { label: "48 tests passing", state: "passing" },
        { label: "lint", state: "passing" },
      ],
      bullets: ["Flagged: 30-day cookie expiry kept"],
      actions: [
        { id: "merge", label: "Merge", primary: true },
        { id: "reopen", label: "Reopen thread" },
      ],
    },
  },
  {
    id: "pr-21",
    threadId: "retry",
    provider: "github",
    repo: "globnet-sync",
    number: 21,
    url: "https://example.invalid/21",
    title: "Add retry logic to NAS backup",
    status: "open",
    checkState: "running",
    updatedAt: at(186),
    additions: 64,
    deletions: 12,
  },
  {
    id: "pr-19",
    threadId: "deps",
    provider: "github",
    repo: "jabot-app",
    number: 19,
    url: "https://example.invalid/19",
    title: "Bump dependencies",
    status: "draft",
    checkState: null,
    updatedAt: at(1500),
    additions: 302,
    deletions: 288,
  },
  {
    id: "pr-18",
    threadId: "onboarding",
    provider: "github",
    repo: "jabot-app",
    number: 18,
    url: "https://example.invalid/18",
    title: "Onboarding flow polish",
    status: "merged",
    checkState: "passing",
    updatedAt: at(1600),
    additions: 142,
    deletions: 58,
  },
];

function renderPrs(over: Partial<Parameters<typeof PullRequestsView>[0]> = {}) {
  const props = {
    pullRequests: PRS,
    now: NOW,
    onOpenThread: vi.fn(),
    onAction: vi.fn(),
    ...over,
  };
  render(<PullRequestsView {...props} />);
  return props;
}

describe("PullRequestsView", () => {
  it("counts only open PRs on the Open tab", () => {
    renderPrs();

    expect(screen.getByRole("tab", { name: "Open · 2" })).toBeInTheDocument();
  });

  it("keeps a PR out of NEEDS REVIEW while its checks run", () => {
    renderPrs();

    expect(
      screen.getByText("CHECKS RUNNING", { selector: ".page-section" }),
    ).toBeInTheDocument();
    // Exactly one PR wears the review pill: the one whose checks are done.
    expect(
      screen.getAllByText("NEEDS REVIEW", { selector: ".tagpill" }),
    ).toHaveLength(1);
  });

  it("shows drafts only under Drafts", async () => {
    renderPrs();

    expect(screen.queryByText("Bump dependencies")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("tab", { name: "Drafts" }));
    expect(screen.getByText("Bump dependencies")).toBeInTheDocument();
    expect(
      screen.queryByText("Migrate auth to sessions"),
    ).not.toBeInTheDocument();
  });

  it("expands a PR into its checks and flags", () => {
    renderPrs();

    expect(screen.getByText("48 tests passing")).toBeInTheDocument();
    expect(
      screen.getByText("Flagged: 30-day cookie expiry kept"),
    ).toBeInTheDocument();
    expect(screen.getByText(/3 files changed/)).toBeInTheDocument();
  });

  it("reopens the session that opened the PR", async () => {
    const props = renderPrs();

    await userEvent.click(screen.getByRole("button", { name: "Reopen thread" }));

    expect(props.onOpenThread).toHaveBeenCalledWith("auth");
  });

  it("passes other actions through by id", async () => {
    const props = renderPrs();

    await userEvent.click(screen.getByRole("button", { name: "Merge" }));

    expect(props.onAction).toHaveBeenCalledWith("pr-23", "merge");
  });

  it("shows the diff stat on every row", () => {
    renderPrs();

    expect(screen.getByText("+214")).toBeInTheDocument();
    expect(screen.getByText("−96")).toBeInTheDocument();
  });

  it("moves between filters with the arrow keys", async () => {
    renderPrs();

    screen.getByRole("tab", { name: "Open · 2" }).focus();
    await userEvent.keyboard("{ArrowRight}");

    expect(screen.getByRole("tab", { name: "Merged" })).toHaveFocus();
    expect(screen.getByText("Onboarding flow polish")).toBeInTheDocument();
    expect(
      screen.queryByText("Migrate auth to sessions"),
    ).not.toBeInTheDocument();

    // Left from the first tab wraps to the last rather than dead-ending.
    await userEvent.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(screen.getByRole("tab", { name: "Drafts" })).toHaveFocus();
    expect(screen.getByText("Bump dependencies")).toBeInTheDocument();
  });

  it("names the panel after the tab that is showing it", () => {
    renderPrs();

    expect(screen.getByRole("tabpanel", { name: "Open · 2" })).toBeInTheDocument();
  });
});
