/**
 * A bot's standing chat, on the host (#24).
 *
 * `crew/thread` and `HostClient.botThread` have been served and typed since
 * #24 with no caller anywhere in `src/`, so `case "bot"` drew the mock
 * reducer's fixtures keyed by bot id — and everything typed into that chat
 * went to the reducer too. The bot's real thread, its runs and its memory
 * directory were somewhere else.
 *
 * The load-bearing assertion here is the one about ids: the transcript has to
 * be read for the *thread* `crew/thread` resolved, not for the bot. Those are
 * two different strings, and using the bot's was the whole of the bug.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import App from "../App";
import { LiveChatView } from "../views/ChatView";
import {
  connectHost,
  type CrewRefParams,
  type HelloResult,
  type HostClient,
  type JsonRpcNotification,
  type PromptParams,
} from "../host";
import { SESSION_UPDATE } from "../host";
import type { Bot, HostTarget } from "../components/types";

vi.mock("../host", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../host")>();
  return { ...actual, connectHost: vi.fn() };
});

const HELLO: HelloResult = {
  protocolVersion: 1,
  hostId: "host-1",
  hostName: "This Mac",
  hostMode: "in-process",
  version: "0.1.0",
  platform: "macos",
  device: { deviceId: "dev-1", name: "This Mac", role: "full" },
  methods: [],
  notifications: [],
};

const BOT: Bot = {
  id: "writer",
  name: "Writer",
  color: "b-orange",
  instructions: "Drafts and edits.",
  tools: [],
  harnessId: "claude",
  isChief: false,
};

const HOST: HostTarget = { hostId: "host-1", name: "This Mac", reachable: true };

/** The id `crew/thread` derives from the bot. Deliberately not the bot id —
    the fixture path used that, and a test where the two are equal cannot tell
    the fix from the bug. */
const THREAD_ID = "th-standing-writer";

function stub(over: Record<string, unknown> = {}) {
  const handlers = new Set<(n: JsonRpcNotification) => void>();
  const prompts: PromptParams[] = [];
  const cancel = vi.fn(async () => {});
  let busy = false;

  const botThread = vi.fn(async (_params: CrewRefParams) => ({
    threadId: THREAD_ID,
    title: "Writer",
    state: "active",
    foldPolicy: "default",
    cwd: "/data/bots/writer",
    harnessId: "claude",
    botId: BOT.id,
    process: {
      connected: true,
      acpState: "idle",
      pendingPermissions: 0,
      resumable: true,
    },
    runs: [],
    unread: 0,
  }));

  const threadTranscript = vi.fn(async (params: { threadId: string }) => ({
    threadId: params.threadId,
    headSeq: 1,
    events: [
      {
        seq: 1,
        method: SESSION_UPDATE,
        createdAt: "",
        payload: {
          sessionUpdate: "user_message_chunk",
          content: { type: "text", text: "summarise the overnight mail" },
        },
      },
    ],
    truncated: false,
    queued: [],
  }));

  const client = {
    disconnect: vi.fn(),
    deviceId: "dev-1",
    botThread,
    threadTranscript,
    onNotification: (handler: (n: JsonRpcNotification) => void) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    pendingPermissions: vi.fn(async () => ({ requests: [] })),
    prompt: vi.fn(async (params: PromptParams) => {
      prompts.push(params);
      const queued = busy;
      busy = true;
      return {
        threadId: params.threadId,
        acpSessionId: "sess-1",
        accepted: !queued,
        queued,
        queuePosition: queued ? 1 : undefined,
      };
    }),
    cancel,
    ...over,
  } as unknown as HostClient;

  return {
    client,
    botThread,
    threadTranscript,
    prompts,
    cancel,
    /** One `session/update`, as the host relays it. */
    update(threadId: string, payload: unknown) {
      act(() => {
        for (const handler of handlers) {
          handler({
            jsonrpc: "2.0",
            method: SESSION_UPDATE,
            params: { hostId: "h1", threadId, seq: 2, transcriptSeq: 2, acp: payload },
          });
        }
      });
    },
  };
}

function draw(host = stub()) {
  render(<LiveChatView client={host.client} bot={BOT} host={HOST} />);
  return host;
}

describe("a bot's standing chat, live", () => {
  it("opens the bot's standing thread and reads that thread's transcript", async () => {
    const host = draw();

    await waitFor(() =>
      expect(host.botThread).toHaveBeenCalledWith({ botId: "writer" }),
    );
    // Once: `crew/thread` is idempotent host-side, but a second call per
    // render would still be a round trip per keystroke's worth of state.
    expect(host.botThread).toHaveBeenCalledTimes(1);

    // The resolved thread, not the bot. This is the fixture path's bug.
    await waitFor(() =>
      expect(host.threadTranscript).toHaveBeenCalledWith(
        expect.objectContaining({ threadId: THREAD_ID }),
      ),
    );
    expect(await screen.findByText("summarise the overnight mail")).toBeInTheDocument();
  });

  it("draws what the agent says on that thread", async () => {
    const host = draw();
    await screen.findByText("summarise the overnight mail");

    host.update(THREAD_ID, {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "Three need a reply." },
    });

    expect(await screen.findByText("Three need a reply.")).toBeInTheDocument();
  });

  it("sends what I type on the standing thread", async () => {
    const host = draw();
    await screen.findByText("summarise the overnight mail");

    await userEvent.type(
      screen.getByLabelText("Message Writer"),
      "file the receipts{Enter}",
    );

    await waitFor(() => expect(host.prompts).toHaveLength(1));
    expect(host.prompts[0].threadId).toBe(THREAD_ID);
  });

  it("offers Stop while the turn is in flight, and cancels that thread", async () => {
    const host = draw();
    await screen.findByText("summarise the overnight mail");

    await userEvent.type(
      screen.getByLabelText("Message Writer"),
      "file the receipts{Enter}",
    );

    const stop = await screen.findByRole("button", { name: "Stop" });
    await userEvent.click(stop);

    await waitFor(() =>
      expect(host.cancel).toHaveBeenCalledWith({ threadId: THREAD_ID }),
    );
  });

  /**
   * Said rather than swallowed. Every other host read in this app degrades to
   * a fixture or a missing caption; this one cannot, because without a thread
   * there is no conversation at all — and a chat that silently discards what
   * you type is the failure this view exists to fix.
   */
  it("says why when the bot's thread cannot be opened", async () => {
    draw(
      stub({
        botThread: vi.fn(async () => {
          throw new Error("Harness unavailable: claude");
        }),
      }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Harness unavailable: claude",
    );
  });
});

/**
 * The shell picks the live path.
 *
 * `LiveChatView` existing is not the same as it being reached: the app opens
 * on Chief's standing chat, and before this change that pane was the mock
 * reducer's fixtures no matter what the host was.
 */
describe("the shell's bot pane", () => {
  it("opens the host's standing thread for the bot it is showing", async () => {
    const host = stub();
    vi.mocked(connectHost).mockResolvedValue({ client: host.client, hello: HELLO });

    render(<App />);
    await screen.findByText("This Mac · v0.1.0");

    // Chief is what the shell opens on, so this is the pane under test.
    await waitFor(() =>
      expect(host.botThread).toHaveBeenCalledWith({ botId: "chief" }),
    );
    expect(await screen.findByText("summarise the overnight mail")).toBeInTheDocument();
    // And not the reducer's fixture conversation, which is what it drew before.
    expect(screen.queryByText(/Fold the migration/)).toBeNull();
  });

  /** A transport that predates `crew/thread` has no thread to open. The shell
      keeps its fixtures rather than showing an error where a chat should be —
      the same fallback every other host read here takes. */
  it("keeps the fixtures on a host that cannot open one", async () => {
    vi.mocked(connectHost).mockResolvedValue({
      client: { disconnect: vi.fn() } as unknown as HostClient,
      hello: HELLO,
    });

    render(<App />);
    await screen.findByText("This Mac · v0.1.0");

    expect(screen.getByRole("heading", { level: 2, name: "Chief" })).toBeInTheDocument();
    expect(screen.getByLabelText("Message Chief")).toBeInTheDocument();
  });
});
