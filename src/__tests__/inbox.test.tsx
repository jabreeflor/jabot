/**
 * The Inbox is a projection of run events (#5). This view's own contract is
 * narrower: it sorts cards into what came back versus what is still asleep,
 * filters honestly, and never lets a sleeping card claim your attention.
 */
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { InboxView } from "../views/InboxView";
import type { InboxCard } from "../components/types";

const NOW = new Date("2026-08-20T14:30:00Z");
const at = (minutes: number) =>
  new Date(NOW.getTime() - minutes * 60_000).toISOString();

const CARDS: InboxCard[] = [
  {
    id: "done-1",
    threadId: "sidebar",
    kind: "done",
    title: "Sidebar overflow fix finished",
    summary: "jabot-app · PR #22 opened",
    createdAt: at(38),
    source: { type: "code" },
    detail: {
      path: "jabot-app · started → folded → resurfaced",
      bullets: ["Rail collapses under 900px", "All 48 tests passing"],
      actions: [
        { id: "open-pr", label: "Open PR #22", primary: true },
        { id: "reopen", label: "Reopen thread" },
        { id: "archive", label: "Archive" },
      ],
    },
  },
  {
    id: "needs-1",
    threadId: "inboxm",
    kind: "needs_you",
    title: "Inbox Manager needs a call",
    summary: "Two invoices from UGREEN — archive, or flag for finance?",
    createdAt: at(63),
    source: { type: "bot", id: "inboxm", name: "Inbox Mgr", color: "b-purple" },
  },
  {
    id: "sleep-1",
    threadId: "nas",
    kind: "folded",
    title: "Nightly NAS backup",
    summary: "globnet-sync · resurfaces on success, failure, or question.",
    createdAt: at(120),
    source: { type: "code" },
  },
];

function renderInbox(over: Partial<Parameters<typeof InboxView>[0]> = {}) {
  const props = {
    cards: CARDS,
    now: NOW,
    onOpenThread: vi.fn(),
    onAction: vi.fn(),
    ...over,
  };
  render(<InboxView {...props} />);
  return props;
}

describe("InboxView", () => {
  it("separates what came back from what is still asleep", () => {
    renderInbox();

    expect(screen.getByText("RESURFACED")).toBeInTheDocument();
    expect(screen.getByText("STILL SLEEPING")).toBeInTheDocument();
    expect(screen.getByText("SLEEPING")).toBeInTheDocument();
    expect(screen.getByText("DONE")).toBeInTheDocument();
  });

  it("drops sleeping and finished work from Needs you", async () => {
    renderInbox();

    await userEvent.click(screen.getByRole("tab", { name: "Needs you" }));

    expect(screen.getByText("Inbox Manager needs a call")).toBeInTheDocument();
    expect(screen.queryByText("Nightly NAS backup")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Sidebar overflow fix finished"),
    ).not.toBeInTheDocument();
  });

  it("keeps only finished work under Done", async () => {
    renderInbox();

    await userEvent.click(screen.getByRole("tab", { name: "Done" }));

    expect(
      screen.getByText("Sidebar overflow fix finished"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Inbox Manager needs a call"),
    ).not.toBeInTheDocument();
  });

  it("expands a card into what actually happened", async () => {
    renderInbox();

    const summary = screen.getByRole("button", { expanded: true });
    expect(screen.getByText("All 48 tests passing")).toBeInTheDocument();

    await userEvent.click(summary);
    expect(screen.queryByText("All 48 tests passing")).not.toBeInTheDocument();
  });

  it("reopens the thread a card came from", async () => {
    const props = renderInbox();

    await userEvent.click(screen.getByRole("button", { name: "Reopen thread" }));

    expect(props.onOpenThread).toHaveBeenCalledWith("sidebar");
  });

  it("passes other card actions through by id", async () => {
    const props = renderInbox();

    await userEvent.click(screen.getByRole("button", { name: "Archive" }));

    expect(props.onAction).toHaveBeenCalledWith("done-1", "archive");
  });

  it("opens the thread when a sleeping card is clicked", async () => {
    const props = renderInbox();

    const sleeping = screen.getByText("Nightly NAS backup").closest("button");
    await userEvent.click(sleeping!);

    expect(props.onOpenThread).toHaveBeenCalledWith("nas");
  });

  it("says when there is nothing waiting", () => {
    renderInbox({ cards: [] });

    expect(screen.getByText(/Nothing waiting/)).toBeInTheDocument();
  });

  it("stamps each card with when it arrived", () => {
    renderInbox();

    const row = screen.getByText("Inbox Manager needs a call").closest("button");
    expect(within(row!).getByText(/^\d/)).toBeInTheDocument();
  });
});
