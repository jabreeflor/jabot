//! The projection a phone reads (#29): needs you, done, still sleeping.

import { describe, expect, it } from "vitest";

import {
  projectInbox,
  withAsk,
  withoutAsk,
  askCard,
  interactionCard,
} from "../inbox";
import type {
  InboxEventView,
  InboxListResult,
  InteractionView,
  PendingPermissionView,
} from "../../host/protocol";

function event(over: Partial<InboxEventView> = {}): InboxEventView {
  return {
    id: "e1",
    threadId: "t1",
    threadTitle: "Auth migration",
    threadState: "resurfaced",
    kind: "done",
    title: "Auth migration finished",
    summary: "12 files",
    createdAt: "2026-08-20T10:00:00.000Z",
    ...over,
  };
}

function ask(over: Partial<PendingPermissionView> = {}): PendingPermissionView {
  return {
    requestId: "req-1",
    threadId: "t9",
    title: "Run ls",
    kind: "execute",
    subject: { title: "Run ls", command: "ls -la /etc" },
    options: [
      { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
      { optionId: "reject_once", name: "Reject", kind: "reject_once" },
    ],
    createdAt: "2026-08-20T11:00:00.000Z",
    stale: false,
    ...over,
  };
}

function list(over: Partial<InboxListResult> = {}): InboxListResult {
  return { events: [], sleeping: [], unread: 0, ...over };
}

describe("the Inbox as the phone sees it", () => {
  it("sorts the host's events into the three sections", () => {
    const inbox = projectInbox(
      list({
        events: [
          event({ id: "done-1", kind: "done" }),
          event({ id: "failed-1", kind: "failed", threadId: "t2" }),
          event({ id: "stuck-1", kind: "stuck", threadId: "t3" }),
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
        unread: 2,
      }),
    );

    expect(inbox.done.map((c) => c.id)).toEqual(["done-1"]);
    // Failed and stuck are things asking for a human, so they are needs-you —
    // the same rule the desktop's `NEEDS_YOU_KINDS` applies (#22).
    expect(inbox.needs.map((c) => c.id).sort()).toEqual([
      "failed-1",
      "stuck-1",
    ]);
    expect(inbox.sleeping[0]).toMatchObject({
      threadId: "t4",
      // A folded thread whose adapter is still going says so; that is the
      // whole feature of folding (decision #5).
      summary: "Still working",
      kind: "folded",
    });
    expect(inbox.unread).toBe(2);
  });

  /// The reason this file is not just the desktop projection: an ask on an
  /// *active* thread never writes an inbox event, and it is exactly what the
  /// phone exists to answer.
  it("shows an outstanding ask that no inbox event mentions", () => {
    const inbox = projectInbox(list(), { requests: [ask()] });
    expect(inbox.needs).toHaveLength(1);
    expect(inbox.needs[0]).toMatchObject({
      id: "req-1",
      threadId: "t9",
      title: "Run ls",
      // The command, not just the title: "Run ls" and "Run rm -rf /" have the
      // same title, and the phone is where that difference is easiest to miss.
      summary: "ls -la /etc",
    });
    expect(inbox.needs[0].ask?.options.map((o) => o.optionId)).toEqual([
      "allow_once",
      "reject_once",
    ]);
  });

  it("does not show one question twice", () => {
    // The host resurfaced the thread as `needs_you` *because* of this ask.
    const inbox = projectInbox(
      list({
        events: [
          event({ id: "needs-1", kind: "needs_you", threadId: "t9" }),
          event({ id: "done-1", kind: "done", threadId: "t1" }),
        ],
      }),
      { requests: [ask({ threadId: "t9" })] },
    );
    expect(inbox.needs.map((c) => c.id)).toEqual(["req-1"]);
    expect(inbox.done.map((c) => c.id)).toEqual(["done-1"]);
  });

  it("leaves a dismissed card out", () => {
    const inbox = projectInbox(
      list({ events: [event({ dismissedAt: "2026-08-20T10:05:00.000Z" })] }),
    );
    expect(inbox.done).toEqual([]);
  });

  it("keeps the agent's options and drops what cannot be answered with", () => {
    const card = askCard(
      ask({
        options: [
          { name: "no id here" },
          { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
          "not an option at all",
        ],
      }),
    );
    expect(card.ask?.options).toEqual([
      { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
    ]);
  });

  it("adds a live ask and removes an answered one", () => {
    const start = projectInbox(list());
    const added = withAsk(start, askCard(ask()));
    expect(added.needs).toHaveLength(1);
    // Idempotent: two notifications for one ask is one card.
    expect(withAsk(added, askCard(ask())).needs).toHaveLength(1);
    expect(withoutAsk(added, "req-1").needs).toEqual([]);
    // An id nobody has heard of leaves the snapshot alone, identity included.
    expect(withoutAsk(added, "req-nope")).toBe(added);
  });

  /**
   * A PR card is its own kind (#28). The phone's fallback turns anything it
   * does not recognise into `needs_you`, so a kind the host has and this list
   * has not draws "checks failed" under a NEEDS YOU pill — a claim that an
   * agent is blocked on the human, about a session that finished yesterday.
   */
  it("draws a pull request card as a pull request", () => {
    const inbox = projectInbox(
      list({
        events: [
          event({
            id: "pr-1",
            kind: "pr",
            threadState: "archived",
            title: "PR #23 · checks failed",
            summary: "jabreeflor/jabot · tests failed",
          }),
        ],
      }),
    );
    const card = inbox.needs[0];
    expect(card.kind).toBe("pr");
    expect(card.tag.label).toBe("PULL REQUEST");
  });

  // #298: a question or plan is "what needs you" too, in its own words.
  it("draws a plan for review and a one-tap question as their own kinds", () => {
    const plan: InteractionView = {
      requestId: "int-plan",
      threadId: "t9",
      ask: "plan",
      method: "cursor/create_plan",
      title: "Auth migration",
      request: { toolCallId: "c", overview: "Move sessions over.", plan: "" },
      createdAt: "2026-08-20T12:00:00.000Z",
      stale: false,
    };
    const question: InteractionView = {
      requestId: "int-q",
      threadId: "t8",
      ask: "question",
      method: "cursor/ask_question",
      title: "Need input",
      request: {
        toolCallId: "c",
        questions: [
          {
            id: "q1",
            prompt: "Which mode?",
            options: [
              { id: "agent", label: "Agent" },
              { id: "plan", label: "Plan" },
            ],
          },
        ],
      },
      createdAt: "2026-08-20T12:01:00.000Z",
      stale: false,
    };
    const inbox = projectInbox(
      list({
        events: [event({ id: "n1", threadId: "t9", kind: "needs_you" })],
      }),
      { requests: [] },
      { requests: [plan, question] },
    );
    // The plan's thread had a `needs_you` row too; the answerable card wins.
    expect(inbox.needs.map((card) => card.id)).toEqual(["int-q", "int-plan"]);
    const planCard = inbox.needs[1];
    expect(planCard.kind).toBe("plan");
    expect(planCard.tag.label).toBe("PLAN REVIEW");
    expect(planCard.summary).toBe("Move sessions over.");
    expect(planCard.ask).toBeUndefined();
    expect(planCard.interaction).toMatchObject({ ask: "plan", stale: false });

    const questionCard = inbox.needs[0];
    expect(questionCard.kind).toBe("question");
    expect(questionCard.tag.label).toBe("QUESTION");
    expect(questionCard.summary).toBe("Which mode?");
    expect(questionCard.interaction).toMatchObject({
      questionId: "q1",
      options: [
        { optionId: "agent", name: "Agent" },
        { optionId: "plan", name: "Plan" },
      ],
    });
    // Answered somewhere: the card goes, whichever kind it was.
    expect(withoutAsk(inbox, "int-plan").needs.map((c) => c.id)).toEqual([
      "int-q",
    ]);
  });

  it("does not flatten several questions, or a multiple choice, into buttons", () => {
    const many = interactionCard({
      requestId: "int-many",
      threadId: "t8",
      ask: "question",
      method: "cursor/ask_question",
      title: "Need input",
      request: {
        questions: [
          { id: "a", prompt: "First?", options: [{ id: "x", label: "X" }] },
          { id: "b", prompt: "Second?", options: [{ id: "y", label: "Y" }] },
        ],
      },
      createdAt: "2026-08-20T12:02:00.000Z",
      stale: false,
    });
    expect(many.interaction?.options).toBeUndefined();
    expect(many.interaction?.prompts).toEqual(["First?", "Second?"]);
    const multi = interactionCard({
      requestId: "int-multi",
      threadId: "t8",
      ask: "question",
      method: "cursor/ask_question",
      title: "Need input",
      request: {
        questions: [
          {
            id: "a",
            prompt: "Which?",
            allowMultiple: true,
            options: [{ id: "x", label: "X" }],
          },
        ],
      },
      createdAt: "2026-08-20T12:03:00.000Z",
      stale: true,
    });
    expect(multi.interaction?.options).toBeUndefined();
    expect(multi.interaction?.stale).toBe(true);
  });
});
