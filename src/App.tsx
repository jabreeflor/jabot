//! The app shell: sidebar, one main view, and the overlays that float over both.
//!
//! Two data sources meet here and they are deliberately separate. The *real*
//! host connection (#8) supplies who and where the host is, the registered
//! folders and the threads inside them (#16), and the crew, its templates, the
//! tool chips and the harness cards (#17). The rest still comes from
//! `mock-host`, and swapping it for host RPC is what #14, #22 and #28 each do
//! for their slice.
//!
//! Where the two meet, the host wins *once it has answered*. A `null` folder
//! list is "the host has not said yet" — a preview build, a unit test, a host
//! still starting — and the shell keeps its fixtures rather than blanking the
//! sidebar. An empty list is an answer, and an empty sidebar is the right
//! picture of a fresh install.
//!
//! Navigation, overlay state, and the leave animation live here because they are
//! the only state that spans the sidebar and the main pane.

import { useEffect, useReducer, useRef, useState } from "react";

import {
  connectHost,
  type FolderRegisterParams,
  type HelloResult,
  type HostClient,
  HostRpcError,
} from "./host";
import { AddFolderModal } from "./components/AddFolderModal";
import { BotEditorModal } from "./components/BotEditorModal";
import { ScheduleEditorModal } from "./components/ScheduleEditorModal";
import { NewChatModal } from "./components/NewChatModal";
import { Sidebar } from "./components/Sidebar";
import {
  ThreadContextMenu,
  type MenuPosition,
} from "./components/ThreadContextMenu";
import type {
  Bot,
  BotDraft,
  HarnessCard,
  HostTarget,
  NewChatDraft,
  Selection,
  ThreadSummary,
  ToolOption,
} from "./components/types";
import { useCrew } from "./views/crew";
import {
  useSchedules,
  type ScheduleDraft,
  type Schedules,
} from "./views/schedules";
import { allThreads, useFolders } from "./views/folders";
import { ChatView } from "./views/ChatView";
import { CrewView } from "./views/CrewView";
import { InboxView } from "./views/InboxView";
import { PullRequestsView } from "./views/PullRequestsView";
import { SchedulesView } from "./views/SchedulesView";
import { LiveThreadView, ThreadView } from "./views/ThreadView";
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
type ScheduleEditorState =
  | { open: false }
  | { open: true; scheduleId: string | null };
type MenuState = { thread: ThreadSummary; position: MenuPosition } | null;

function App() {
  const [state, dispatch] = useReducer(mockHostReducer, null, initialMockState);
  const [selection, setSelection] = useState<Selection>({
    view: "bot",
    botId: "chief",
  });
  const [newChat, setNewChat] = useState<NewChatState>({ open: false });
  const [newChatError, setNewChatError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState>({ open: false });
  const [editorError, setEditorError] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState>(null);
  const [leaving, setLeaving] = useState<readonly string[]>([]);
  const [addFolder, setAddFolder] = useState(false);
  const [scheduleEditor, setScheduleEditor] = useState<ScheduleEditorState>({
    open: false,
  });
  const [scheduleError, setScheduleError] = useState<string | null>(null);
  const { client, hello, hostError, connecting } = useHost();
  const registered = useFolders(client);
  const crew = useCrew(client);
  const schedules = useSchedules(client);
  // The host wins once it has answered, per source. A crew of `null` is "not
  // asked yet" — a preview build or a unit test — and the fixtures stand in;
  // a real answer always has Chief in it.
  const bots = crew.bots ?? state.bots;
  const templates = crew.templates ?? BOT_TEMPLATES;
  const toolChips = crew.tools ?? TOOL_CATALOG;
  const hostToolChips = crew.hostTools ?? HOST_TOOLS;
  const harnesses = crew.harnesses ?? HARNESSES;
  // Registered folders replace the fixtures the moment the host answers; the
  // threads inside them are real rows, so the main pane has to be able to find
  // one that the mock reducer has never heard of.
  const folders = registered.folders ?? sidebarFolders(state);
  const hostThreads = registered.folders ? allThreads(registered.folders) : [];

  const timers = useRef<number[]>([]);
  useEffect(() => {
    const pending = timers.current;
    return () => pending.forEach((id) => window.clearTimeout(id));
  }, []);

  /** Run the data change once the exit transition has finished. */
  function afterLeaving(then: () => void) {
    timers.current.push(window.setTimeout(then, LEAVE_MS));
  }

  /** Let the row animate out before the data change removes it. */
  function leaveThread(threadId: string, then: () => void) {
    setLeaving((ids) => [...ids, threadId]);
    setMenu(null);
    afterLeaving(() => {
      then();
      setLeaving((ids) => ids.filter((id) => id !== threadId));
    });
  }

  /**
   * Answering a notice card fades it out; this is what finally takes it out of
   * the transcript. `resolved` alone would leave an invisible card holding its
   * full height and still being read out.
   */
  function answerNotice(
    conversationId: string,
    itemId: string,
    actionId: string,
  ) {
    dispatch({ type: "answerNotice", conversationId, itemId, actionId });
    afterLeaving(() =>
      dispatch({ type: "removeNotice", conversationId, itemId }),
    );
  }

  function startThread(draft: NewChatDraft) {
    const folder = folders.find((f) => f.id === draft.folderId);
    // A registered folder is the host's, and so is the thread started in it:
    // `thread/open` is what stamps the row with its cwd and its repo (#16).
    if (client && registered.folders && folder) {
      setNewChatError(null);
      client
        .openThread({
          title: draft.task,
          cwd: folder.cwd ?? folder.path,
          harnessId: draft.harnessId,
          folderId: folder.id,
        })
        .then((thread) => {
          registered.reload();
          setNewChat({ open: false });
          setSelection({ view: "thread", threadId: thread.threadId });
        })
        // The card stays open holding the draft: a refused spawn is something
        // to fix and retry, not a reason to lose what the user typed.
        .catch((err) => setNewChatError(formatError(err)));
      return;
    }
    const threadId = nextThreadId(state);
    dispatch({ type: "startThread", draft });
    setNewChat({ open: false });
    setSelection({ view: "thread", threadId });
  }

  /** The editor *is* the record (#17), so a save is a host call and the modal
      stays open until the host has taken it. */
  function saveBot(draft: BotDraft) {
    if (!editor.open) return;
    if (crew.bots) {
      setEditorError(null);
      crew
        .save(editor.botId, draft)
        .then(() => closeEditor())
        .catch((err) => setEditorError(formatError(err)));
      return;
    }
    dispatch({ type: "saveBot", botId: editor.botId, draft });
    closeEditor();
  }

  /** Remove from the grid or from inside the editor. Chief is refused by the
      host; the UI hides the button, and this is what happens if it ever does
      not. */
  function removeBot(botId: string, fromEditor: boolean) {
    if (crew.bots) {
      setEditorError(null);
      crew
        .remove(botId)
        .then(() => {
          if (fromEditor) closeEditor();
        })
        .catch((err) => {
          if (fromEditor) setEditorError(formatError(err));
        });
      return;
    }
    dispatch({ type: "removeBot", botId });
    if (fromEditor) closeEditor();
  }

  function closeEditor() {
    setEditor({ open: false });
    setEditorError(null);
  }

  /** Like the bot editor, the schedule editor *is* the record: the modal stays
      open until the host has taken it, because a refused cron is something to
      correct rather than something to lose. */
  function saveSchedule(draft: ScheduleDraft) {
    if (!scheduleEditor.open) return;
    setScheduleError(null);
    schedules
      .save(scheduleEditor.scheduleId, draft)
      .then(() => closeScheduleEditor())
      .catch((err) => setScheduleError(formatError(err)));
  }

  function removeSchedule(scheduleId: string) {
    setScheduleError(null);
    schedules
      .remove(scheduleId)
      .then(() => closeScheduleEditor())
      .catch((err) => setScheduleError(formatError(err)));
  }

  function closeScheduleEditor() {
    setScheduleEditor({ open: false });
    setScheduleError(null);
  }

  const host: HostTarget = {
    hostId: hello?.hostId ?? "local",
    name: hello?.hostName ?? "This Mac",
    reachable: hello !== null,
  };
  const editingBot =
    editor.open && editor.botId
      ? (bots.find((bot) => bot.id === editor.botId) ?? null)
      : null;
  const editingSchedule =
    scheduleEditor.open && scheduleEditor.scheduleId
      ? ((schedules.schedules ?? []).find(
          (row) => row.scheduleId === scheduleEditor.scheduleId,
        ) ?? null)
      : null;

  return (
    <div className="app-shell">
      <div className="titlebar-drag" data-tauri-drag-region />

      <Sidebar
        bots={bots}
        folders={folders}
        foldersEmpty={registered.folders?.length === 0}
        onAddFolder={client ? () => setAddFolder(true) : undefined}
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
        onOpenSchedules={() => setSelection({ view: "schedules" })}
        onNewChat={(folderId) => setNewChat({ open: true, folderId })}
        onThreadMenu={(thread, position) => setMenu({ thread, position })}
      />

      <main className="main">
        <MainView
          client={client}
          state={state}
          schedules={schedules}
          onEditSchedule={(scheduleId) =>
            setScheduleEditor({ open: true, scheduleId })
          }
          onAddSchedule={() => setScheduleEditor({ open: true, scheduleId: null })}
          bots={bots}
          tools={[...toolChips, ...hostToolChips]}
          harnesses={harnesses}
          hostThreads={hostThreads}
          selection={selection}
          host={host}
          onSelect={setSelection}
          onSend={(conversationId, text) =>
            dispatch({ type: "sendMessage", conversationId, text })
          }
          onNotice={answerNotice}
          onInboxAction={(cardId, actionId) => {
            if (actionId === "archive") {
              dispatch({ type: "dismissInboxCard", cardId });
            }
          }}
          onEditBot={(botId) => setEditor({ open: true, botId })}
          onAddBot={() => setEditor({ open: true, botId: null })}
          onRemoveBot={(botId) => removeBot(botId, false)}
        />
      </main>

      {addFolder && (
        <AddFolderModal
          onRegister={(params: FolderRegisterParams) =>
            registered.register(params)
          }
          onCancel={() => setAddFolder(false)}
        />
      )}

      {newChat.open && (
        <NewChatModal
          harnesses={harnesses}
          folders={registered.folders ?? state.folders}
          defaultFolderId={newChat.folderId}
          error={newChatError}
          onStart={startThread}
          onCancel={() => {
            setNewChat({ open: false });
            setNewChatError(null);
          }}
        />
      )}

      {editor.open && (
        <BotEditorModal
          bot={editingBot}
          templates={templates}
          tools={toolChips}
          harnesses={harnesses}
          error={editorError}
          onSave={saveBot}
          onRemove={(botId) => removeBot(botId, true)}
          onCancel={closeEditor}
        />
      )}

      {scheduleEditor.open && (
        <ScheduleEditorModal
          schedule={
            editingSchedule
              ? {
                  scheduleId: editingSchedule.scheduleId,
                  botId: editingSchedule.botId,
                  name: editingSchedule.name,
                  cron: editingSchedule.cron,
                  prompt: editingSchedule.prompt,
                  catchUp: editingSchedule.catchUp,
                }
              : null
          }
          bots={bots}
          error={scheduleError}
          onSave={saveSchedule}
          onRemove={removeSchedule}
          onCancel={closeScheduleEditor}
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

/**
 * Both conversation views are keyed by the conversation they show. Without the
 * key, switching bot or thread is a props change rather than a remount, and the
 * composer's unsent draft follows you across — a half-typed instruction landing
 * in whichever session you opened next.
 */
function MainView({
  client,
  state,
  schedules,
  onEditSchedule,
  onAddSchedule,
  bots,
  tools,
  harnesses,
  hostThreads,
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
  /** Present once the host has answered. A thread the host owns is rendered
      live — hydrated from `thread/transcript` and streamed from there (#14). */
  client: HostClient | null;
  state: MockState;
  /** Recurring jobs, host-owned from the first answer (#25). */
  schedules: Schedules;
  onEditSchedule: (scheduleId: string) => void;
  onAddSchedule: () => void;
  /** The crew, host-owned once `crew/list` has answered (#17). */
  bots: readonly Bot[];
  /** Every chip a crew card may have to name: the MCP catalog plus Chief's
      host tools, which are in no `tools/list`. */
  tools: readonly ToolOption[];
  harnesses: readonly HarnessCard[];
  /** Rows the host owns. Looked up before the fixtures, because a folder the
      host registered lists threads the mock reducer has never heard of. */
  hostThreads: readonly ThreadSummary[];
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
          bots={bots}
          harnesses={harnesses}
          tools={tools}
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
    case "schedules":
      return (
        <SchedulesView
          schedules={schedules.schedules}
          bots={bots}
          error={schedules.error}
          onReload={schedules.reload}
          onAdd={onAddSchedule}
          onEdit={onEditSchedule}
          onToggle={(scheduleId, enabled) => {
            void schedules.setEnabled(scheduleId, enabled);
          }}
          onRunNow={(scheduleId) => {
            void schedules.runNow(scheduleId);
          }}
          onOpenThread={(threadId) => onSelect({ view: "thread", threadId })}
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
      const hostThread = hostThreads.find((t) => t.id === selection.threadId);
      // A real row gets the real transcript. The fixtures keep the mock
      // reducer, so the shell still renders before a host has answered.
      if (client && hostThread) {
        return (
          <LiveThreadView
            key={hostThread.id}
            client={client}
            thread={hostThread}
            harnesses={harnesses}
            host={host}
          />
        );
      }
      const thread =
        hostThread ?? state.threads.find((t) => t.id === selection.threadId);
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
          key={thread.id}
          thread={thread}
          harnesses={harnesses}
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
      const bot = bots.find((b) => b.id === selection.botId);
      if (!bot) return <div className="view" />;
      return (
        <ChatView
          key={bot.id}
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
  const [client, setClient] = useState<HostClient | null>(null);
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
        // Kept so the feature slices can call the host directly. The identity
        // is stable for the life of the connection, which is what lets it be an
        // effect dependency without re-fetching on every render.
        setClient(client);
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
      setClient(null);
    };
  }, []);

  return { client, hello, hostError, connecting };
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
