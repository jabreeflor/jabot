//! The approver screen. What it shows, and what it will not show.

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { InboxScreen } from "../InboxScreen";
import { MobileApp } from "../MobileApp";
import { TranscriptScreen } from "../TranscriptScreen";
import { projectInbox } from "../inbox";
import type {
  PendingPermissionView,
  ThreadTranscriptResult,
} from "../../host/protocol";
import type { TranscriptItem } from "../../components/types";

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


/**
 * The transcript screen (#29).
 *
 * `InboxScreen` has had an `onOpen` since #29 and nothing ever called it, so
 * tapping a card title was a dead callback and the phone could say an agent
 * was blocked without ever showing what it had been doing.
 */
describe("the transcript screen", () => {
  const ITEMS: TranscriptItem[] = [
    { kind: "user", id: "u1", text: "Migrate auth to sessions" },
    { kind: "agent", id: "a1", text: "Reading the current middleware." },
    {
      kind: "tool",
      id: "c1",
      call: {
        id: "c1",
        kind: "edit",
        target: "src/auth.ts",
        status: "completed",
        note: "+18 −7",
      },
    },
  ];

  function draw(over: Partial<Parameters<typeof TranscriptScreen>[0]> = {}) {
    const props = {
      title: "Auth migration",
      items: ITEMS,
      truncated: false,
      onBack: vi.fn(),
      ...over,
    };
    render(<TranscriptScreen {...props} />);
    return props;
  }

  it("shows what was said and, in one line each, what was run", () => {
    draw();

    expect(screen.getByText("Migrate auth to sessions")).toBeInTheDocument();
    expect(screen.getByText("Reading the current middleware.")).toBeInTheDocument();
    // The target, not the output: a phone is not where somebody reads a diff,
    // and a screen that tried would bury the sentence the question is about.
    expect(screen.getByText("src/auth.ts")).toBeInTheDocument();
    expect(screen.getByText("+18 −7")).toBeInTheDocument();
  });

  /**
   * `transcript()` takes the last 40 events and its own doc says "never
   * more". A screen that silently began mid-sentence would let somebody
   * answer a permission on a conversation whose start they think they read.
   */
  it("says out loud when it is only showing the end", () => {
    draw({ truncated: true });

    expect(
      screen.getByText(/Showing the end of this thread/),
    ).toBeInTheDocument();
  });

  it("does not claim truncation when there is none", () => {
    draw();

    expect(screen.queryByText(/Showing the end of this thread/)).toBeNull();
  });

  it("goes back to the Inbox", async () => {
    const props = draw();

    await userEvent.click(screen.getByRole("button", { name: /Inbox/ }));
    expect(props.onBack).toHaveBeenCalled();
  });

  it("says why when the host will not answer", () => {
    draw({ items: [], error: "thread/transcript is not something an approver device may call" });

    expect(screen.getByRole("alert")).toHaveTextContent("approver device");
  });

  it("waits rather than claiming an empty thread", () => {
    draw({ items: [], loading: true });

    expect(screen.getByText("Reading the thread…")).toBeInTheDocument();
    expect(screen.queryByText(/Nothing has been said/)).toBeNull();
  });
});

describe("opening a card", () => {
  const RESULT: ThreadTranscriptResult = {
    threadId: "t9",
    headSeq: 2,
    truncated: true,
    queued: [],
    events: [
      {
        seq: 1,
        method: "session/update",
        createdAt: "2026-08-20T10:59:00.000Z",
        payload: {
          sessionUpdate: "user_message_chunk",
          content: { type: "text", text: "Clear the build directory" },
        },
      },
      {
        seq: 2,
        method: "session/update",
        createdAt: "2026-08-20T10:59:30.000Z",
        payload: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "I need to run rm -rf build." },
        },
      },
    ],
  };

  /** The prop's first coverage since it was written. */
  it("asks the host for the thread the card is about", async () => {
    const transcript = vi.fn(async () => RESULT);
    render(
      <MobileApp
        inbox={inboxWith()}
        session={{ transcript }}
        onAnswer={vi.fn()}
        onDecline={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Run ls" }));

    // The card's thread, and the window `transcript()` defaults to — the
    // screen does not get to choose how much of somebody's conversation
    // leaves the Mac.
    expect(transcript).toHaveBeenCalledWith("t9");
    // Reduced through the desktop's own `hydrate`, so a phone cannot draw a
    // different conversation from the same rows.
    expect(
      await screen.findByText("I need to run rm -rf build."),
    ).toBeInTheDocument();
    expect(screen.getByText("Clear the build directory")).toBeInTheDocument();
    expect(screen.getByText(/Showing the end of this thread/)).toBeInTheDocument();
  });

  it("comes back to the Inbox with the buttons still there", async () => {
    render(
      <MobileApp
        inbox={inboxWith()}
        session={{ transcript: vi.fn(async () => RESULT) }}
        onAnswer={vi.fn()}
        onDecline={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Run ls" }));
    await screen.findByText("I need to run rm -rf build.");
    await userEvent.click(screen.getByRole("button", { name: /Inbox/ }));

    expect(screen.getByRole("button", { name: "Allow once" })).toBeInTheDocument();
  });

  /**
   * The payoff for #101's replay, and the reason a snapshot was not enough.
   *
   * The screen follows the thread while it is open, and the *same* reducer
   * folds the replay and the live stream — so a chunk that arrives while the
   * read is still in flight is neither lost nor drawn twice. The
   * de-duplication is the reducer's, against the seq the hydrate reached;
   * doing it a second time in the client is how two copies drift.
   */
  it("follows the thread while it is open, without drawing anything twice", async () => {
    const watchers: Array<(update: unknown) => void> = [];
    render(
      <MobileApp
        inbox={inboxWith()}
        session={{
          transcript: vi.fn(async () => RESULT),
          watchThread: (_threadId: string, listener: (u: unknown) => void) => {
            watchers.push(listener);
            return () => {};
          },
        } as never}
        onAnswer={vi.fn()}
        onDecline={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Run ls" }));
    await screen.findByText("I need to run rm -rf build.");

    // A replayed frame the hydrate already covered (seq 2), and a genuinely
    // new one (seq 3).
    for (const listener of watchers) {
      listener({
        threadId: "t9",
        seq: 2,
        transcriptSeq: 2,
        acp: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: " (again)" },
        },
      });
      listener({
        threadId: "t9",
        seq: 3,
        transcriptSeq: 3,
        acp: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: " Doing that now." },
        },
      });
    }

    // The new chunk landed. A new bubble rather than an append, which is
    // right: the replay carried no open run, so `hydrate` closed the last
    // bubble — a caret blinking over a turn that ended a week ago is the lie
    // the stop reason exists to stop telling.
    expect(await screen.findByText("Doing that now.")).toBeInTheDocument();
    // And the replayed frame did not draw itself a second time.
    expect(screen.getAllByText(/rm -rf build/)).toHaveLength(1);
    expect(screen.queryByText(/again/)).toBeNull();
  });

  /** A host that refuses is a sentence on the screen, not a blank one. */
  it("says what the host said when the read fails", async () => {
    render(
      <MobileApp
        inbox={inboxWith()}
        session={{
          transcript: vi.fn(async () => {
            throw new Error("thread is gone");
          }),
        }}
        onAnswer={vi.fn()}
        onDecline={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Run ls" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("thread is gone");
  });
});
