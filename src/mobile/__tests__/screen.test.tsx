//! The approver screen. What it shows, and what it will not show.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { InboxScreen } from "../InboxScreen";
import { projectInbox } from "../inbox";
import type { PendingPermissionView } from "../../host/protocol";

const ASK: PendingPermissionView = {
  requestId: "req-1",
  threadId: "t9",
  title: "Run ls",
  kind: "execute",
  subject: { title: "Run ls", command: "rm -rf build" },
  options: [
    { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
    { optionId: "allow_always", name: "Always allow", kind: "allow_always" },
    { optionId: "reject_once", name: "Reject", kind: "reject_once" },
  ],
  createdAt: "2026-08-20T11:00:00.000Z",
  stale: false,
};

function inboxWith(requests: PendingPermissionView[] = [ASK]) {
  return projectInbox(
    {
      events: [
        {
          id: "done-1",
          threadId: "t1",
          threadTitle: "Auth migration",
          threadState: "resurfaced",
          kind: "done",
          title: "Auth migration finished",
          summary: "12 files changed",
          createdAt: "2026-08-20T10:00:00.000Z",
        },
      ],
      sleeping: [
        {
          threadId: "t4",
          title: "Nightly deps",
          foldPolicy: "default",
          foldedAt: "2026-08-20T09:00:00.000Z",
          acpState: "running",
        },
      ],
      unread: 1,
    },
    { requests },
  );
}

describe("the approver screen", () => {
  it("puts the agent's own options on the buttons", async () => {
    const onAnswer = vi.fn();
    render(
      <InboxScreen
        inbox={inboxWith()}
        deviceName="Jabree's iPhone"
        onAnswer={onAnswer}
        onDecline={vi.fn()}
      />,
    );

    // Verbatim, in the agent's order. #20 promises the host never invents an
    // option; a phone inventing one would break that where it is least visible.
    for (const label of ["Allow once", "Always allow", "Reject"]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
    // What is actually about to happen, not just the title.
    expect(screen.getByText("rm -rf build")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Always allow" }));
    expect(onAnswer).toHaveBeenCalledWith("req-1", "allow_always");
  });

  it("offers no answer for a sleeping thread", () => {
    render(
      <InboxScreen inbox={inboxWith()} onAnswer={vi.fn()} onDecline={vi.fn()} />,
    );
    const sleeping = screen.getByRole("region", { name: "STILL SLEEPING" });
    // Decision #5: a folded thread is not a notification. On the device most
    // likely to buzz, that has to mean nothing to press.
    expect(sleeping).toHaveTextContent("Nightly deps");
    expect(sleeping.querySelectorAll("button")).toHaveLength(0);
  });

  it("declines without inventing an option the agent never offered", async () => {
    const onDecline = vi.fn();
    render(
      <InboxScreen
        inbox={inboxWith([{ ...ASK, options: [] }])}
        onAnswer={vi.fn()}
        onDecline={onDecline}
      />,
    );
    // No answerable options: the only honest affordance is `cancelled`, which
    // the host records as its own outcome rather than as a rejection.
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(onDecline).toHaveBeenCalledWith("req-1");
  });

  it("says when an answer will not reach anyone", () => {
    render(
      <InboxScreen
        inbox={inboxWith([{ ...ASK, stale: true }])}
        onAnswer={vi.fn()}
        onDecline={vi.fn()}
      />,
    );
    expect(screen.getByText(/recorded, not acted on/)).toBeInTheDocument();
  });

  it("keeps the buttons visible while an answer is in flight", () => {
    render(
      <InboxScreen
        inbox={inboxWith()}
        busyId="req-1"
        onAnswer={vi.fn()}
        onDecline={vi.fn()}
      />,
    );
    // Inert, not gone: a card that vanishes on tap and comes back on failure
    // is how somebody answers the same question twice.
    expect(screen.getByRole("button", { name: "Allow once" })).toBeDisabled();
  });
});
