/**
 * Question and plan-review cards (#298): the reducer that draws them and the
 * components that answer them.
 *
 * The reducer half pins the same properties #20's permission cards have —
 * one card per request however many ways it arrives, a lock that survives a
 * second click — plus the two that are new: a settled card stays and says
 * what was decided, and a host's decision is never overwritten by an
 * optimistic one. The component half is keyboard-first: every control is a
 * native input or button, and settling a card moves focus to the line that
 * says what happened.
 */
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Transcript } from "../components/Transcript";
import { PlanCard, QuestionCard } from "../components/InteractionCards";
import type { TranscriptItem } from "../components/types";
import type { InteractionView, TranscriptEventView } from "../host";
import { INTERACTION_RESOLVED } from "../host";
import { expectNoSeriousA11yViolations } from "../../tests/support/a11y";
import {
  applyInteractionAsk,
  applyInteractionResolved,
  applyInteractionUnresolved,
  EMPTY_STREAM,
  hydrate,
  hydrateInteractions,
  interactionItemId,
} from "../views/transcript";

const QUESTION_REQUEST = {
  toolCallId: "call-ask-1",
  title: "Need input",
  questions: [
    {
      id: "q-mode",
      prompt: "Which mode should the migration run in?",
      options: [
        { id: "agent", label: "Agent" },
        { id: "plan", label: "Plan" },
      ],
    },
    {
      id: "q-scope",
      prompt: "Which packages are in scope?",
      allowMultiple: true,
      options: [
        { id: "api", label: "api" },
        { id: "web", label: "web" },
        { id: "worker", label: "worker" },
      ],
    },
  ],
};

const PLAN_REQUEST = {
  toolCallId: "call-plan-1",
  name: "Auth migration",
  overview: "Move session handling onto the new auth service.",
  plan: "## Steps\n\n1. Add the adapter.\n2. Migrate the middleware.",
  todos: [
    { id: "t1", content: "Add the adapter", status: "completed" },
    { id: "t2", content: "Migrate the middleware", status: "in_progress" },
  ],
  phases: [],
};

function questionView(over: Partial<InteractionView> = {}): InteractionView {
  return {
    requestId: "req-q",
    threadId: "t1",
    ask: "question",
    method: "cursor/ask_question",
    title: "Need input",
    request: QUESTION_REQUEST,
    createdAt: "2026-09-13T10:00:00.000Z",
    stale: false,
    ...over,
  };
}

function planView(over: Partial<InteractionView> = {}): InteractionView {
  return {
    requestId: "req-p",
    threadId: "t1",
    ask: "plan",
    method: "cursor/create_plan",
    title: "Auth migration",
    request: PLAN_REQUEST,
    createdAt: "2026-09-13T10:01:00.000Z",
    stale: false,
    ...over,
  };
}

function questionItem(
  over: Partial<Extract<TranscriptItem, { kind: "question" }>> = {},
): Extract<TranscriptItem, { kind: "question" }> {
  const stream = applyInteractionAsk(EMPTY_STREAM, questionView());
  const item = stream.items[0];
  if (item.kind !== "question") throw new Error("not a question");
  return { ...item, ...over };
}

function planItem(
  over: Partial<Extract<TranscriptItem, { kind: "plan" }>> = {},
): Extract<TranscriptItem, { kind: "plan" }> {
  const stream = applyInteractionAsk(EMPTY_STREAM, planView());
  const item = stream.items[0];
  if (item.kind !== "plan") throw new Error("not a plan");
  return { ...item, ...over };
}

describe("the reducer: question and plan cards", () => {
  it("draws one card however many ways the same ask arrives", () => {
    // The transcript row first, as a reopened thread sees it.
    const rows: TranscriptEventView[] = [
      {
        seq: 3,
        method: "cursor/ask_question",
        createdAt: "",
        payload: {
          requestId: "req-q",
          ask: "question",
          method: "cursor/ask_question",
          title: "Need input",
          request: QUESTION_REQUEST,
          raw: { ...QUESTION_REQUEST, runAsync: true },
        },
      },
    ];
    let stream = hydrate({
      threadId: "t1",
      headSeq: 3,
      events: rows,
      truncated: false,
      queued: [],
    });
    expect(stream.items).toHaveLength(1);
    expect(stream.items[0]).toMatchObject({
      kind: "question",
      id: interactionItemId("question", "req-q"),
      title: "Need input",
    });
    const first = stream.items[0];
    // Then the pending list, then the live notification: same object.
    stream = hydrateInteractions(stream, [questionView()]);
    stream = applyInteractionAsk(stream, questionView());
    expect(stream.items).toHaveLength(1);
    expect(stream.items[0]).toBe(first);
    // Only the pending list knows the host that took it is gone.
    stream = hydrateInteractions(stream, [questionView({ stale: true })]);
    expect(stream.items[0]).toMatchObject({ stale: true });
  });

  it("reads the agent's questions and options, and drops what it cannot answer with", () => {
    const item = questionItem();
    expect(item.questions).toHaveLength(2);
    expect(item.questions[0]).toMatchObject({
      id: "q-mode",
      allowMultiple: false,
      options: [
        { id: "agent", label: "Agent" },
        { id: "plan", label: "Plan" },
      ],
    });
    expect(item.questions[1].allowMultiple).toBe(true);
    const odd = applyInteractionAsk(
      EMPTY_STREAM,
      questionView({
        request: {
          questions: [
            { id: "", prompt: "no id", options: [{ id: "a", label: "A" }] },
            { id: "q", prompt: "no options", options: [] },
            { id: "ok", prompt: "fine", options: [{ id: "x" }] },
          ],
        },
      }),
    );
    expect(odd.items[0]).toMatchObject({
      kind: "question",
      questions: [{ id: "ok", options: [{ id: "x", label: "x" }] }],
    });
    // Nothing answerable at all: nothing drawn, nothing crashed.
    const empty = applyInteractionAsk(
      EMPTY_STREAM,
      questionView({ request: { questions: "nope" } }),
    );
    expect(empty.items).toEqual([]);
  });

  it("reads a plan's overview, body and todos", () => {
    const item = planItem();
    expect(item.title).toBe("Auth migration");
    expect(item.overview).toBe(
      "Move session handling onto the new auth service.",
    );
    expect(item.plan.startsWith("## Steps")).toBe(true);
    expect(item.todos.map((todo) => todo.status)).toEqual([
      "completed",
      "in_progress",
    ]);
  });

  it("locks the card on a decision, keeps it, and prefers the host's word", () => {
    let stream = applyInteractionAsk(EMPTY_STREAM, questionView());
    // Optimistic: what this window sent.
    stream = applyInteractionResolved(stream, "req-q", {
      outcome: "answered",
      answers: [{ questionId: "q-mode", selectedOptionIds: ["plan"] }],
    });
    expect(stream.items[0]).toMatchObject({
      decision: { outcome: "answered" },
    });
    // The host's resolution knows whether the agent heard it.
    stream = applyInteractionResolved(stream, "req-q", {
      outcome: "answered",
      answers: [{ questionId: "q-mode", selectedOptionIds: ["plan"] }],
      delivered: true,
    });
    expect(stream.items[0]).toMatchObject({ decision: { delivered: true } });
    // A slower optimistic echo does not take that knowledge away.
    const before = stream.items[0];
    stream = applyInteractionResolved(stream, "req-q", { outcome: "answered" });
    expect(stream.items[0]).toBe(before);
    // And the card is still there — a decision is part of the conversation.
    expect(stream.items).toHaveLength(1);
    // A refused answer unlocks it.
    stream = applyInteractionUnresolved(stream, "req-q");
    expect(stream.items[0]).not.toHaveProperty("decision");
  });

  it("replays a settled card from the transcript alone", () => {
    const rows: TranscriptEventView[] = [
      {
        seq: 1,
        method: "cursor/create_plan",
        createdAt: "",
        payload: {
          requestId: "req-p",
          ask: "plan",
          method: "cursor/create_plan",
          title: "Auth migration",
          request: PLAN_REQUEST,
        },
      },
      {
        seq: 2,
        method: INTERACTION_RESOLVED,
        createdAt: "",
        payload: {
          requestId: "req-p",
          ask: "plan",
          outcome: "unavailable",
          reason: "host shutdown",
          deviceId: "host",
          delivered: true,
        },
      },
    ];
    const stream = hydrate({
      threadId: "t1",
      headSeq: 2,
      events: rows,
      truncated: false,
      queued: [],
    });
    expect(stream.items).toHaveLength(1);
    expect(stream.items[0]).toMatchObject({
      kind: "plan",
      decision: { outcome: "unavailable", reason: "host shutdown" },
    });
    expect(stream.headSeq).toBe(2);
    // Replayed rows at or below the hydrated head are copies, not new cards.
    const again = hydrate(
      {
        threadId: "t1",
        headSeq: 2,
        events: rows,
        truncated: false,
        queued: [],
      },
      stream,
    );
    expect(again.items).toHaveLength(1);
  });
});

describe("the question card", () => {
  it("offers radios for a single choice, checkboxes for several, and sends the ids", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    render(<QuestionCard item={questionItem()} onReply={onReply} />);

    const mode = screen.getByRole("group", {
      name: "Which mode should the migration run in?",
    });
    expect(within(mode).getAllByRole("radio")).toHaveLength(2);
    const scope = screen.getByRole("group", {
      name: "Which packages are in scope?",
    });
    expect(within(scope).getAllByRole("checkbox")).toHaveLength(3);

    const send = screen.getByRole("button", { name: "Send answer" });
    expect(send).toBeDisabled();
    await user.click(within(mode).getByRole("radio", { name: "Plan" }));
    expect(send).toBeDisabled();
    await user.click(within(scope).getByRole("checkbox", { name: "api" }));
    await user.click(within(scope).getByRole("checkbox", { name: "web" }));
    expect(send).toBeEnabled();
    await user.click(send);

    expect(onReply).toHaveBeenCalledTimes(1);
    expect(onReply).toHaveBeenCalledWith("req-q", {
      outcome: "answered",
      answers: [
        { questionId: "q-mode", selectedOptionIds: ["plan"] },
        { questionId: "q-scope", selectedOptionIds: ["api", "web"] },
      ],
    });
  });

  it("works from the keyboard alone", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    render(
      <QuestionCard
        item={questionItem({
          questions: [questionItem().questions[0]],
        })}
        onReply={onReply}
      />,
    );
    await user.tab();
    expect(screen.getByRole("radio", { name: "Agent" })).toHaveFocus();
    await user.keyboard("[ArrowDown]");
    expect(screen.getByRole("radio", { name: "Plan" })).toBeChecked();
    await user.tab();
    expect(screen.getByRole("button", { name: "Send answer" })).toHaveFocus();
    await user.keyboard("[Enter]");
    expect(onReply).toHaveBeenCalledWith("req-q", {
      outcome: "answered",
      answers: [{ questionId: "q-mode", selectedOptionIds: ["plan"] }],
    });
  });

  it("can be skipped or cancelled without choosing", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    render(<QuestionCard item={questionItem()} onReply={onReply} />);
    await user.click(screen.getByRole("button", { name: "Skip" }));
    expect(onReply).toHaveBeenLastCalledWith("req-q", { outcome: "skipped" });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onReply).toHaveBeenLastCalledWith("req-q", {
      outcome: "cancelled",
    });
  });

  it("locks once settled, says what was chosen, and moves focus to the status line", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    const { rerender } = render(
      <QuestionCard item={questionItem()} onReply={onReply} />,
    );
    const plan = screen.getByRole("radio", { name: "Plan" });
    await user.click(plan);
    expect(plan).toHaveFocus();

    rerender(
      <QuestionCard
        item={questionItem({
          decision: {
            outcome: "answered",
            answers: [
              { questionId: "q-mode", selectedOptionIds: ["plan"] },
              { questionId: "q-scope", selectedOptionIds: ["api", "web"] },
            ],
            delivered: true,
          },
        })}
        onReply={onReply}
      />,
    );
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Answered: Plan; api, web.");
    expect(status).toHaveFocus();
    expect(screen.queryByRole("button", { name: "Send answer" })).toBeNull();
    expect(screen.getByRole("radio", { name: "Plan" })).toBeDisabled();
  });

  it("says when the answer was recorded but reached nobody, and why a card cannot be answered", () => {
    const { rerender } = render(
      <QuestionCard
        item={questionItem({
          decision: { outcome: "skipped", delivered: false },
        })}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Skipped. Recorded, but the agent that asked is gone.",
    );
    rerender(
      <QuestionCard
        item={questionItem({ decision: { outcome: "expired" } })}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "The turn ended before this was answered.",
    );
    rerender(
      <QuestionCard
        item={questionItem({ decision: { outcome: "unavailable" } })}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "The session that asked this is gone",
    );
    // Stale and unsettled: nothing to choose, one honest button.
    rerender(
      <QuestionCard item={questionItem({ stale: true })} onReply={vi.fn()} />,
    );
    expect(screen.getByRole("radio", { name: "Plan" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Send answer" })).toBeNull();
  });
});

describe("the plan card", () => {
  it("shows the plan and accepts it, without claiming any permission", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    render(<PlanCard item={planItem()} onReply={onReply} />);
    expect(
      screen.getByText("Move session handling onto the new auth service."),
    ).toBeInTheDocument();
    // The markdown body renders as structure, not as asterisks and hashes.
    expect(screen.getByText("Add the adapter.")).toBeInTheDocument();
    const steps = screen.getByRole("list", { name: "Plan steps" });
    expect(within(steps).getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByRole("status")).toHaveTextContent(
      "does not change what the agent is allowed to do",
    );
    await user.click(screen.getByRole("button", { name: "Accept plan" }));
    expect(onReply).toHaveBeenCalledWith("req-p", { outcome: "accepted" });
  });

  it("asks why before rejecting, and passes the reason along", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    render(<PlanCard item={planItem()} onReply={onReply} />);
    await user.click(screen.getByRole("button", { name: "Reject…" }));
    const why = screen.getByLabelText("Why? (optional)");
    expect(why).toHaveFocus();
    await user.type(why, "too broad");
    await user.keyboard("[Enter]");
    expect(onReply).toHaveBeenCalledWith("req-p", {
      outcome: "rejected",
      reason: "too broad",
    });
  });

  it("locks once settled and reports the outcome", () => {
    const { rerender } = render(
      <PlanCard
        item={planItem({ decision: { outcome: "accepted", delivered: true } })}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Plan accepted.");
    expect(screen.queryByRole("button", { name: "Accept plan" })).toBeNull();
    rerender(
      <PlanCard
        item={planItem({
          decision: { outcome: "rejected", reason: "too broad" },
        })}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Plan rejected — too broad",
    );
  });
});

describe("in the transcript", () => {
  it("routes a card's answer through the transcript's own callback", async () => {
    const user = userEvent.setup();
    const onInteract = vi.fn();
    const onAction = vi.fn();
    render(
      <Transcript
        items={[
          { kind: "user", id: "u1", text: "go" },
          questionItem(),
          planItem(),
        ]}
        onAction={onAction}
        onInteract={onInteract}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Accept plan" }));
    expect(onInteract).toHaveBeenCalledWith("req-p", { outcome: "accepted" });
    // Never the permission path.
    expect(onAction).not.toHaveBeenCalled();
  });

  it("has no critical or serious accessibility violations", async () => {
    const { container } = render(
      <Transcript
        items={[
          questionItem(),
          planItem(),
          questionItem({
            id: "settled",
            requestId: "req-s",
            decision: { outcome: "cancelled" },
          }),
        ]}
        onInteract={vi.fn()}
      />,
    );
    await expectNoSeriousA11yViolations(container);
  });
});

describe("edges", () => {
  it("groups a plan's todos by phase and turns its headings into bold lines", () => {
    render(
      <PlanCard
        item={planItem({
          plan: "## Steps\n\nDo the thing.",
          phases: [
            {
              name: "Wire it up",
              todos: [
                { id: "t1", content: "Add the adapter", status: "completed" },
              ],
            },
            {
              name: "",
              todos: [{ id: "t2", content: "Cut over", status: "pending" }],
            },
          ],
        })}
        onReply={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("list", { name: "Wire it up" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Phase 2" }),
    ).toBeInTheDocument();
    // Not "## Steps": the renderer has no headings, so a plan's become bold.
    expect(screen.getByText("Steps").tagName).toBe("STRONG");
    expect(screen.queryByText(/## Steps/)).toBeNull();
  });

  it("offers a stale plan only Dismiss, and a settled one nothing", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    const { rerender } = render(
      <PlanCard item={planItem({ stale: true })} onReply={onReply} />,
    );
    expect(screen.queryByRole("button", { name: "Accept plan" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(onReply).toHaveBeenCalledWith("req-p", { outcome: "cancelled" });
    rerender(
      <PlanCard
        item={planItem({ decision: { outcome: "unavailable" } })}
        onReply={onReply}
      />,
    );
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent(
      "The session that asked this is gone",
    );
  });

  it("rejects a plan without a reason when none was given", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    render(<PlanCard item={planItem()} onReply={onReply} />);
    await user.click(screen.getByRole("button", { name: "Reject…" }));
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(
      screen.getByRole("button", { name: "Accept plan" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Reject…" }));
    await user.click(screen.getByRole("button", { name: "Reject plan" }));
    expect(onReply).toHaveBeenCalledWith("req-p", { outcome: "rejected" });
    // With a handler that records rather than settles, the card is still
    // open: Back returns to the three verbs.
    await user.click(screen.getByRole("button", { name: "Back" }));
    await user.click(screen.getByRole("button", { name: "Not now" }));
    expect(onReply).toHaveBeenLastCalledWith("req-p", { outcome: "cancelled" });
  });

  it("unpicks a multiple-choice option and re-picks a single one", async () => {
    const user = userEvent.setup();
    const onReply = vi.fn();
    render(<QuestionCard item={questionItem()} onReply={onReply} />);
    await user.click(screen.getByRole("radio", { name: "Agent" }));
    await user.click(screen.getByRole("radio", { name: "Plan" }));
    await user.click(screen.getByRole("checkbox", { name: "api" }));
    await user.click(screen.getByRole("checkbox", { name: "web" }));
    await user.click(screen.getByRole("checkbox", { name: "api" }));
    await user.click(screen.getByRole("button", { name: "Send answer" }));
    expect(onReply).toHaveBeenCalledWith("req-q", {
      outcome: "answered",
      answers: [
        { questionId: "q-mode", selectedOptionIds: ["plan"] },
        { questionId: "q-scope", selectedOptionIds: ["web"] },
      ],
    });
  });

  it("ignores a resolution it cannot place, and a kind it does not draw", () => {
    const rows: TranscriptEventView[] = [
      {
        seq: 1,
        method: INTERACTION_RESOLVED,
        createdAt: "",
        payload: { outcome: "answered" },
      },
      {
        seq: 2,
        method: INTERACTION_RESOLVED,
        createdAt: "",
        payload: { requestId: "nobody", outcome: "whatever" },
      },
      {
        seq: 3,
        method: "session/request_permission",
        createdAt: "",
        payload: { requestId: "perm", ask: "permission", request: {} },
      },
    ];
    const stream = hydrate({
      threadId: "t1",
      headSeq: 3,
      events: rows,
      truncated: false,
      queued: [],
    });
    expect(stream.items).toEqual([]);
    expect(stream.headSeq).toBe(3);
    // Nor does a resolution for a card that is not there change anything.
    expect(
      applyInteractionResolved(stream, "nobody", { outcome: "cancelled" }),
    ).toBe(stream);
    expect(applyInteractionUnresolved(stream, "nobody")).toBe(stream);
    const drawn = applyInteractionAsk(EMPTY_STREAM, questionView());
    expect(applyInteractionUnresolved(drawn, "req-q")).toBe(drawn);
  });

  it("keeps only string option ids from a recorded answer", () => {
    const rows: TranscriptEventView[] = [
      {
        seq: 1,
        method: "cursor/ask_question",
        createdAt: "",
        payload: {
          requestId: "req-q",
          ask: "question",
          title: "Need input",
          request: QUESTION_REQUEST,
        },
      },
      {
        seq: 2,
        method: INTERACTION_RESOLVED,
        createdAt: "",
        payload: {
          requestId: "req-q",
          outcome: "answered",
          answers: [
            { questionId: "q-mode", selectedOptionIds: ["plan", 7] },
            { selectedOptionIds: ["x"] },
          ],
        },
      },
    ];
    const stream = hydrate({
      threadId: "t1",
      headSeq: 2,
      events: rows,
      truncated: false,
      queued: [],
    });
    expect(stream.items[0]).toMatchObject({
      decision: {
        outcome: "answered",
        answers: [{ questionId: "q-mode", selectedOptionIds: ["plan"] }],
      },
    });
  });
});
