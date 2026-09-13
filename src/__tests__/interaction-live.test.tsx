/**
 * Question cards on a live thread (#298): the hook between the card and the
 * host.
 *
 * `interaction-cards.test.tsx` pins the reducer and the components on their
 * own. This is the wiring: a live `interaction/ask` draws the card, its
 * answer reaches `interaction/reply` in the agent's ids, a refused answer
 * unlocks the card, an undelivered one says so, and a resolution from
 * elsewhere locks it without a second answer.
 */
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type {
  HarnessCard,
  HostTarget,
  ThreadSummary,
} from "../components/types";
import {
  INTERACTION_ASK,
  INTERACTION_RESOLVED,
  type HostClient,
  type InteractionOutcome,
  type InteractionReplyParams,
  type InteractionView,
  type JsonRpcNotification,
} from "../host";
import { LiveThreadView } from "../views/ThreadView";

const THREAD: ThreadSummary = {
  id: "t1",
  folderId: null,
  botId: null,
  harnessId: "cursor",
  title: "Auth migration",
  state: "active",
  foldPolicy: "default",
  runState: null,
};
const HARNESSES: HarnessCard[] = [
  { id: "cursor", label: "Cursor", blurb: "", accent: "var(--h-cursor)" },
];
const HOST: HostTarget = { hostId: "h1", name: "This Mac", reachable: true };

function question(over: Partial<InteractionView> = {}): InteractionView {
  return {
    requestId: "req-q",
    threadId: "t1",
    ask: "question",
    method: "cursor/ask_question",
    title: "Need input",
    request: {
      toolCallId: "c",
      questions: [
        {
          id: "q-mode",
          prompt: "Which mode?",
          options: [
            { id: "agent", label: "Agent" },
            { id: "plan", label: "Plan" },
          ],
        },
      ],
    },
    createdAt: "2026-09-13T10:00:00.000Z",
    stale: false,
    ...over,
  };
}

interface Behaviour {
  delivered?: boolean;
  refuse?: boolean;
  /** An earlier resolution stands, with this outcome. */
  already?: InteractionOutcome;
}

function stubHost(pending: InteractionView[] = [], behaviour: Behaviour = {}) {
  const handlers = new Set<(n: JsonRpcNotification) => void>();
  const replyInteraction = vi.fn(async (params: InteractionReplyParams) => {
    if (behaviour.refuse) {
      throw new Error("question q-mode has no option yolo");
    }
    return {
      requestId: params.requestId,
      delivered: behaviour.delivered ?? true,
      alreadyAnswered: behaviour.already !== undefined,
      outcome: behaviour.already ?? params.outcome,
      state: "answered",
    };
  });
  const client = {
    deviceId: "dev-1",
    onNotification: (handler: (n: JsonRpcNotification) => void) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    threadTranscript: vi.fn(async () => ({
      threadId: "t1",
      headSeq: 0,
      events: [],
      truncated: false,
      queued: [],
    })),
    pendingPermissions: vi.fn(async () => ({ requests: [] })),
    pendingInteractions: vi.fn(async () => ({ requests: pending })),
    replyInteraction,
    threadState: vi.fn(async () => ({ threadId: "t1" })),
  } as unknown as HostClient;

  function notify(method: string, params: unknown) {
    act(() => {
      for (const handler of handlers) {
        handler({ jsonrpc: "2.0", method, params });
      }
    });
  }

  return {
    client,
    replyInteraction,
    ask(view: InteractionView) {
      notify(INTERACTION_ASK, {
        hostId: "h1",
        threadId: "t1",
        seq: 5,
        requestId: view.requestId,
        ask: view.ask,
        method: view.method,
        title: view.title,
        request: view.request,
      });
    },
    resolve(requestId: string, outcome: InteractionOutcome) {
      notify(INTERACTION_RESOLVED, {
        hostId: "h1",
        threadId: "t1",
        seq: 6,
        requestId,
        deviceId: "dev-2",
        outcome,
        delivered: true,
      });
    },
  };
}

async function renderThread(host: ReturnType<typeof stubHost>) {
  render(
    <LiveThreadView
      client={host.client}
      thread={THREAD}
      harnesses={HARNESSES}
      host={HOST}
    />,
  );
  await waitFor(() =>
    expect(host.client.pendingInteractions).toHaveBeenCalled(),
  );
}

describe("a question on a live thread", () => {
  it("draws the card from the live ask and sends the chosen ids", async () => {
    const host = stubHost();
    await renderThread(host);
    host.ask(question());

    await screen.findByRole("group", { name: "Which mode?" });
    await userEvent.click(screen.getByRole("radio", { name: "Plan" }));
    await userEvent.click(screen.getByRole("button", { name: "Send answer" }));

    expect(host.replyInteraction).toHaveBeenCalledWith({
      requestId: "req-q",
      deviceId: "dev-1",
      outcome: "answered",
      answers: [{ questionId: "q-mode", selectedOptionIds: ["plan"] }],
    });
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Answered: Plan."),
    );
    // Locked: the buttons are gone, the choice is disabled.
    expect(screen.queryByRole("button", { name: "Send answer" })).toBeNull();
    expect(screen.getByRole("radio", { name: "Plan" })).toBeDisabled();
  });

  it("unlocks the card and says why when the host refuses the answer", async () => {
    const host = stubHost([], { refuse: true });
    await renderThread(host);
    host.ask(question());
    await screen.findByRole("group", { name: "Which mode?" });
    await userEvent.click(screen.getByRole("radio", { name: "Agent" }));
    await userEvent.click(screen.getByRole("button", { name: "Send answer" }));

    // Refused: back to answerable, with the reason on the error line.
    expect(
      await screen.findByRole("button", { name: "Send answer" }),
    ).toBeEnabled();
    expect(screen.getByRole("alert")).toHaveTextContent(/has no option yolo/);
    expect(host.replyInteraction).toHaveBeenCalledTimes(1);
  });

  it("says when the answer was recorded but reached nobody", async () => {
    const host = stubHost([], { delivered: false });
    await renderThread(host);
    host.ask(question());
    await screen.findByRole("group", { name: "Which mode?" });
    await userEvent.click(screen.getByRole("button", { name: "Skip" }));

    // The card's own status line — the transcript gains a second status
    // region for the sys line that says the same thing out loud.
    const card = screen.getByRole("form", { name: "Need input" });
    await waitFor(() =>
      expect(within(card).getByRole("status")).toHaveTextContent(
        "Skipped. Recorded, but the agent that asked is gone.",
      ),
    );
    expect(
      screen.getByText(/message the thread to pick the work back up/),
    ).toBeInTheDocument();
  });

  it("locks the card when it was settled somewhere else, without answering", async () => {
    const host = stubHost();
    await renderThread(host);
    host.ask(question());
    await screen.findByRole("button", { name: "Send answer" });

    host.resolve("req-q", "cancelled");

    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Cancelled."),
    );
    expect(screen.queryByRole("button", { name: "Send answer" })).toBeNull();
    expect(host.replyInteraction).not.toHaveBeenCalled();
  });

  it("reports what stands when the click was a repeat", async () => {
    const host = stubHost([], { already: "expired" });
    await renderThread(host);
    host.ask(question());
    await screen.findByRole("group", { name: "Which mode?" });
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "The turn ended before this was answered.",
      ),
    );
  });

  it("hydrates a question the host that took it left behind as unanswerable", async () => {
    const host = stubHost([question({ stale: true })]);
    await renderThread(host);
    expect(await screen.findByRole("status")).toHaveTextContent(
      /JaBot restarted while the agent was waiting/,
    );
    expect(screen.getByRole("radio", { name: "Plan" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(host.replyInteraction).toHaveBeenCalledWith({
      requestId: "req-q",
      deviceId: "dev-1",
      outcome: "cancelled",
    });
  });

  it("draws one card when the live ask repeats the one it hydrated", async () => {
    const host = stubHost([question()]);
    await renderThread(host);
    await screen.findByRole("group", { name: "Which mode?" });
    host.ask(question());
    await waitFor(() =>
      expect(
        screen.getAllByRole("group", { name: "Which mode?" }),
      ).toHaveLength(1),
    );
  });
});
