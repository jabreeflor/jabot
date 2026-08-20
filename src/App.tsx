//! The app shell: sidebar, one main view, and the overlays that float over both.
//!
//! Two data sources meet here and they are deliberately separate. The *real*
//! host connection (#8) supplies who and where the host is — that seam is live
//! today. Everything the views render comes from `mock-host`, and swapping it
//! for host RPC is what #14, #16, #17, #22 and #28 each do for their slice.
//!
//! Navigation, overlay state, and the leave animation live here because they are
//! the only state that spans the sidebar and the main pane.

import { useEffect, useReducer, useRef, useState } from "react";

import { connectHost, type HelloResult, HostRpcError } from "./host";
import { BotEditorModal } from "./components/BotEditorModal";
import { NewChatModal } from "./components/NewChatModal";
import { Sidebar } from "./components/Sidebar";
import {
  ThreadContextMenu,
  type MenuPosition,
} from "./components/ThreadContextMenu";
import type {
  BotDraft,
  HostTarget,
  NewChatDraft,
  Selection,
  ThreadSummary,
} from "./components/types";
import { ChatView } from "./views/ChatView";
import { CrewView } from "./views/CrewView";
import { InboxView } from "./views/InboxView";
import { PullRequestsView } from "./views/PullRequestsView";
import { ThreadView } from "./views/ThreadView";
import {
  BOT_TEMPLATES,
  HARNESSES,
  HOST_TOOLS,
  TOOL_CATALOG,
  initialMockState,
  mockHostReducer,
  needsYouCount,
  nextThreadId,
  openPrCount,
  sidebarFolders,
  type MockState,
} from "./views/mock-host";
import "./App.css";

/** Matches the row's exit transition, so the state change lands after it. */
const LEAVE_MS = 380;

const USER_NAME = "Jabree Flor";

type NewChatState = { open: false } | { open: true; folderId: string | null };
type EditorState = { open: false } | { open: true; botId: string | null };
type MenuState = { thread: ThreadSummary; position: MenuPosition } | null;

function App() {
  const [state, dispatch] = useReducer(mockHostReducer, null, initialMockState);
  const [selection, setSelection] = useState<Selection>({
    view: "bot",
    botId: "chief",
  });
  const [newChat, setNewChat] = useState<NewChatState>({ open: false });
  const [editor, setEditor] = useState<EditorState>({ open: false });
  const [menu, setMenu] = useState<MenuState>(null);
  const [leaving, setLeaving] = useState<readonly string[]>([]);
  const { hello, hostError, connecting } = useHost();

  const timers = useRef<number[]>([]);
  useEffect(() => {
    const pending = timers.current;
    return () => pending.forEach((id) => window.clearTimeout(id));
  }, []);

  /** Let the row animate out before the data change removes it. */
  function leaveThread(threadId: string, then: () => void) {
    setLeaving((ids) => [...ids, threadId]);
    setMenu(null);
    const timer = window.setTimeout(() => {
      then();
      setLeaving((ids) => ids.filter((id) => id !== threadId));
    }, LEAVE_MS);
    timers.current.push(timer);
  }

  function startThread(draft: NewChatDraft) {
    const threadId = nextThreadId(state);
    dispatch({ type: "startThread", draft });
    setNewChat({ open: false });
    setSelection({ view: "thread", threadId });
  }

  function saveBot(draft: BotDraft) {
    if (!editor.open) return;
    dispatch({ type: "saveBot", botId: editor.botId, draft });
    setEditor({ open: false });
  }

  const host: HostTarget = {
    hostId: hello?.hostId ?? "local",
    name: hello?.hostName ?? "This Mac",
    reachable: hello !== null,
  };
  const editingBot =
    editor.open && editor.botId
      ? (state.bots.find((bot) => bot.id === editor.botId) ?? null)
      : null;

  return (
    <div className="app-shell">
      <div className="titlebar-drag" data-tauri-drag-region />

      <Sidebar
        bots={state.bots}
        folders={sidebarFolders(state)}
        selection={selection}
        inboxCount={needsYouCount(state)}
        openPrCount={openPrCount(state)}
        userName={USER_NAME}
        hostLine={hostLine(hello, hostError, connecting)}
        hostOffline={hostError !== null}
        leavingThreadIds={leaving}
        onSelectBot={(botId) => setSelection({ view: "bot", botId })}
        onSelectThread={(threadId) => setSelection({ view: "thread", threadId })}
        onOpenCrew={() => setSelection({ view: "crew" })}
        onOpenInbox={() => setSelection({ view: "inbox" })}
        onOpenPullRequests={() => setSelection({ view: "prs" })}
        onNewChat={(folderId) => setNewChat({ open: true, folderId })}
        onThreadMenu={(thread, position) => setMenu({ thread, position })}
      />

      <main className="main">
        <MainView
          state={state}
          selection={selection}
          host={host}
          onSelect={setSelection}
          onSend={(conversationId, text) =>
            dispatch({ type: "sendMessage", conversationId, text })
          }
          onNotice={(conversationId, itemId, actionId) =>
            dispatch({
              type: "answerNotice",
              conversationId,
              itemId,
              actionId,
            })
          }
          onInboxAction={(cardId, actionId) => {
            if (actionId === "archive") {
              dispatch({ type: "dismissInboxCard", cardId });
            }
          }}
          onEditBot={(botId) => setEditor({ open: true, botId })}
          onAddBot={() => setEditor({ open: true, botId: null })}
          onRemoveBot={(botId) => dispatch({ type: "removeBot", botId })}
        />
      </main>

      {newChat.open && (
        <NewChatModal
          harnesses={HARNESSES}
          folders={state.folders}
          defaultFolderId={newChat.folderId}
          onStart={startThread}
          onCancel={() => setNewChat({ open: false })}
        />
      )}

      {editor.open && (
        <BotEditorModal
          bot={editingBot}
          templates={BOT_TEMPLATES}
          tools={TOOL_CATALOG}
          harnesses={HARNESSES}
          onSave={saveBot}
          onRemove={(botId) => {
            dispatch({ type: "removeBot", botId });
            setEditor({ open: false });
          }}
          onCancel={() => setEditor({ open: false })}
        />
      )}

      {menu && (
        <ThreadContextMenu
          threadTitle={menu.thread.title}
          position={menu.position}
          onClose={() => setMenu(null)}
          onWaitForInbox={() =>
            leaveThread(menu.thread.id, () =>
              dispatch({ type: "foldThread", threadId: menu.thread.id }),
            )
          }
          onArchive={() =>
            leaveThread(menu.thread.id, () =>
              dispatch({ type: "archiveThread", threadId: menu.thread.id }),
            )
          }
          onDelete={() =>
            leaveThread(menu.thread.id, () =>
              dispatch({ type: "deleteThread", threadId: menu.thread.id }),
            )
          }
        />
      )}
    </div>
  );
}

function MainView({
  state,
  selection,
  host,
  onSelect,
  onSend,
  onNotice,
  onInboxAction,
  onEditBot,
  onAddBot,
  onRemoveBot,
}: {
  state: MockState;
  selection: Selection;
  host: HostTarget;
  onSelect: (selection: Selection) => void;
  onSend: (conversationId: string, text: string) => void;
  onNotice: (conversationId: string, itemId: string, actionId: string) => void;
  onInboxAction: (cardId: string, actionId: string) => void;
  onEditBot: (botId: string) => void;
  onAddBot: () => void;
  onRemoveBot: (botId: string) => void;
}) {
  switch (selection.view) {
    case "crew":
      return (
        <CrewView
          bots={state.bots}
          harnesses={HARNESSES}
          tools={[...TOOL_CATALOG, ...HOST_TOOLS]}
          onEdit={onEditBot}
          onAdd={onAddBot}
          onRemove={onRemoveBot}
        />
      );
    case "inbox":
      return (
        <InboxView
          cards={state.inbox}
          onOpenThread={(threadId) => onSelect({ view: "thread", threadId })}
          onAction={onInboxAction}
        />
      );
    case "prs":
      return (
        <PullRequestsView
          pullRequests={state.pullRequests}
          onOpenThread={(threadId) => onSelect({ view: "thread", threadId })}
        />
      );
    case "thread": {
      const thread = state.threads.find((t) => t.id === selection.threadId);
      // A deleted thread is not an error state — the Inbox is where work goes.
      if (!thread) {
        return (
          <div className="view">
            <div className="page-scroll">
              <div className="page page-empty">
                That thread is gone. Check the Inbox.
              </div>
            </div>
          </div>
        );
      }
      return (
        <ThreadView
          thread={thread}
          harnesses={HARNESSES}
          host={host}
          items={state.transcripts[thread.id] ?? []}
          onSend={(text) => onSend(thread.id, text)}
          onAction={(itemId, actionId) =>
            onNotice(thread.id, itemId, actionId)
          }
        />
      );
    }
    case "bot": {
      const bot = state.bots.find((b) => b.id === selection.botId);
      if (!bot) return <div className="view" />;
      return (
        <ChatView
          bot={bot}
          host={host}
          items={state.transcripts[bot.id] ?? []}
          onSend={(text) => onSend(bot.id, text)}
          onAction={(itemId, actionId) => onNotice(bot.id, itemId, actionId)}
        />
      );
    }
  }
}

/**
 * The live host handshake (#8). The renderer never touches ACP stdio; this is
 * the whole of its relationship with the host until the feature issues land.
 */
function useHost() {
  const [hello, setHello] = useState<HelloResult | null>(null);
  const [hostError, setHostError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(true);

  useEffect(() => {
    let cancelled = false;
    let disconnect: (() => void) | undefined;

    connectHost()
      .then(({ client, hello: result }) => {
        if (cancelled) {
          client.disconnect();
          return;
        }
        disconnect = () => client.disconnect();
        setHello(result);
      })
      .catch((err) => {
        if (!cancelled) setHostError(formatError(err));
      })
      .finally(() => {
        if (!cancelled) setConnecting(false);
      });

    return () => {
      cancelled = true;
      disconnect?.();
    };
  }, []);

  return { hello, hostError, connecting };
}

function hostLine(
  hello: HelloResult | null,
  hostError: string | null,
  connecting: boolean,
): string {
  if (hello) return `${hello.hostName} · v${hello.version}`;
  if (connecting) return "Connecting to host…";
  return hostError ?? "Host unreachable";
}

function formatError(err: unknown): string {
  if (err instanceof HostRpcError) {
    return `${err.message} (${err.code})`;
  }
  return String(err);
}

export default App;
