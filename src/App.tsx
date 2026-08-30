//! The app shell: sidebar, one main view, and the overlays that float over both.
//!
//! Two data sources meet here and they are deliberately separate. The *real*
//! host connection (#8) supplies who and where the host is, the registered
//! folders and the threads inside them (#16), the crew, its templates, the
//! tool chips and the harness cards (#17), and the Inbox — the cards, the
//! badge and what its buttons do (#22). What is left on `mock-host` is the
//! bot conversations and Chief's notices, which #24 owns.
//!
//! Where the two meet, the host wins *once it has answered*. A `null` folder
//! list is "the host has not said yet" — a preview build, a unit test, a host
//! still starting — and the shell keeps its fixtures rather than blanking the
//! sidebar. An empty list is an answer, and an empty sidebar is the right
//! picture of a fresh install.
//!
//! Navigation, overlay state, and the leave animation live here because they are
//! the only state that spans the sidebar and the main pane.

import { useCallback, useEffect, useReducer, useRef, useState } from "react";

import {
  connectHost,
  onNotificationActivated,
  type FolderRegisterParams,
  type HelloResult,
  type HostClient,
  HostRpcError,
} from "./host";
import { AddFolderModal } from "./components/AddFolderModal";
import { FolderSettingsModal } from "./components/FolderSettingsModal";
import { GithubSignInModal } from "./components/GithubSignInModal";
import { BotEditorModal } from "./components/BotEditorModal";
import { ScheduleEditorModal } from "./components/ScheduleEditorModal";
import { NewChatModal } from "./components/NewChatModal";
import { SettingsView } from "./views/SettingsView";
import { Sidebar } from "./components/Sidebar";
import {
  ThreadContextMenu,
  type MenuPosition,
} from "./components/ThreadContextMenu";
import type {
  Bot,
  BotDraft,
  FoldPolicy,
  HarnessCard,
  HostTarget,
  NewChatDraft,
  Selection,
  ThreadSummary,
  ToolOption,
} from "./components/types";
import { Onboarding } from "./onboarding/Onboarding";
import {
  loadOnboarding,
  saveOnboarding,
  type OnboardingProfile,
} from "./onboarding/state";
import { useCrew, useHarnessCatalog } from "./views/crew";
import { usePullRequests, type PullRequests } from "./views/pulls";
import { useGithubAuth, type GithubAuth } from "./views/github";
import {
  useSchedules,
  type ScheduleDraft,
  type Schedules,
} from "./views/schedules";
import { allThreads, useFolders, useHostThread } from "./views/folders";
import { useThreadActions } from "./views/fold";
import { useInbox, type HostInbox } from "./views/inbox";
import { useSettings, type Settings } from "./views/settings";
import { CrossIcon } from "./components/Icon";
import { ChatView, LiveChatView } from "./views/ChatView";
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
  noticeThreadId,
  openPrCount,
  sidebarFolders,
  type MockState,
} from "./views/mock-host";
import "./App.css";

/** Matches the row's exit transition, so the state change lands after it. */
const LEAVE_MS = 380;

type NewChatState = { open: false } | { open: true; folderId: string | null };
type EditorState = { open: false } | { open: true; botId: string | null };
/** Editing only: a *new* schedule is written as a prompt inside the Schedules
    screen (#25), so the modal never opens without a record behind it. */
type ScheduleEditorState =
  | { open: false }
  | { open: true; scheduleId: string };
type MenuState = { thread: ThreadSummary; position: MenuPosition } | null;
type HostSession = ReturnType<typeof useHost>;

/**
 * The first-run gate. On a launch with no stored profile the takeover renders
 * instead of the shell — none of the shell's hooks run during setup. The host
 * handshake is hoisted *above* the gate on purpose: the host may have to spawn
 * on a real first launch, so the connection opens while the user is reading
 * pane 1, its status shows under the card, and the same live session is handed
 * to the shell when setup ends — nobody finishes a setup flow and *then*
 * watches "Connecting to host…".
 */
function App() {
  const host = useHost();
  // The first harness a user ever picks came from `mock-host.ts` — the three
  // compiled-in defaults — so a fresh install never saw a tier-2 preset or the
  // user's own tier-3 JSON on the one screen that asks them to choose, and
  // could pick an engine the host would refuse at thread start. The connection
  // is already hoisted above this gate, so the catalog is one call away.
  //
  // Falls back to the fixtures exactly as `AppShell` does: `null` means the
  // host has not answered, and a setup screen that waited on it would be a
  // blank window.
  const liveHarnesses = useHarnessCatalog(host.client);
  const [profile, setProfile] = useState<OnboardingProfile | null>(
    loadOnboarding,
  );
  // The record a re-run is editing. The gate reads this state and never the
  // store, so storage is not wiped while the takeover is open: `onFinish`
  // overwrites it, quitting mid-re-run keeps it, and the seeded draft means
  // Escape or Skip re-persists the name that was already there rather than
  // "You".
  const [editing, setEditing] = useState<OnboardingProfile | null>(null);

  return profile ? (
    <AppShell
      profile={profile}
      host={host}
      onRunSetup={() => {
        setEditing(profile);
        setProfile(null);
      }}
    />
  ) : (
    <Onboarding
      harnesses={liveHarnesses ?? HARNESSES}
      profile={editing ?? undefined}
      hostLine={hostLine(host.hello, host.hostError, host.connecting)}
      hostOffline={host.hostError !== null}
      onFinish={(next) => {
        saveOnboarding(next);
        setProfile(next);
        setEditing(null);
      }}
    />
  );
}

function AppShell({
  profile,
  host: hostSession,
  onRunSetup,
}: {
  profile: OnboardingProfile;
  host: HostSession;
  /** Re-enter setup from Crew — the one in-app way to change the name. */
  onRunSetup: () => void;
}) {
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
  const [folderSettings, setFolderSettings] = useState<string | null>(null);
  const [signIn, setSignIn] = useState(false);
  const [scheduleEditor, setScheduleEditor] = useState<ScheduleEditorState>({
    open: false,
  });
  const [scheduleError, setScheduleError] = useState<string | null>(null);
  const { client, hello, hostError, connecting } = hostSession;
  const registered = useFolders(client);
  const crew = useCrew(client);
  // The Inbox (#22). Reloads the sidebar too: reopening a card's thread puts
  // its row back, and archiving one takes it away. Handed the crew so a card
  // on a bot's thread wears that bot's face rather than the code mark — the
  // roster is the only place a bot id becomes a name and a colour. Declared
  // after `useCrew` for that reason, and passed `crew.bots` rather than the
  // fixture fallback below: a host card's bot id would never match a fixture's.
  const inbox = useInbox(client, registered.reload, crew.bots);
  const schedules = useSchedules(client);
  const settings = useSettings(client);
  // Whether GitHub can be asked as anybody (#16). The PR board is the only
  // surface that needs it, and it needs it twice: to decide whether to ask for
  // the user's own pull requests at all, and to know what to offer if not.
  const github = useGithubAuth(client);
  // The PR board (#28). Two calls behind one hook: an instant store read on
  // mount, and a poll that keeps it warm without ever being able to empty it.
  // A third once signed in — the user's own open PRs, folded into the same
  // list.
  const pulls = usePullRequests(client, github.signedIn);
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
  // The row the settings card is open on, resolved against the live list so a
  // reload after a save re-seeds the form from what the host now holds.
  const settingsFolder =
    folderSettings === null
      ? null
      : (folders.find((f) => f.id === folderSettings) ?? null);
  const hostThreads = registered.folders ? allThreads(registered.folders) : [];
  // A thread the host owns that no folder lists — a bot's standing thread,
  // whose `folder_id` is null. `folder/list` walks folder rows, so those are
  // invisible to `hostThreads`, and the shell used to treat them as fixtures:
  // the main pane said "That thread is gone", and fold or archive went to the
  // mock reducer while the real permissions, runs and process sat untouched.
  const selectedThreadId = selection.view === "thread" ? selection.threadId : null;
  const resolved = useHostThread(
    client,
    selectedThreadId,
    selectedThreadId !== null && hostThreads.some((t) => t.id === selectedThreadId),
  );
  /** Is this the host's thread, whether or not a folder claims it? */
  const isHostThread = useCallback(
    (threadId: string) =>
      hostThreads.some((t) => t.id === threadId) || resolved?.id === threadId,
    [hostThreads, resolved],
  );

  // Fold, Archive and Delete are host calls for a host row (#26).
  // `registered.reload` runs whether the host took them or not: the leave
  // animation has already pulled the row off screen, so a refusal has to put it
  // back rather than leave the sidebar showing a thread that is still active
  // and no longer drawn.
  const {
    fold,
    archive,
    remove,
    error: foldError,
    clearError: clearFoldError,
  } = useThreadActions(client, registered.reload);

  const timers = useRef<number[]>([]);
  useEffect(() => {
    const pending = timers.current;
    return () => pending.forEach((id) => window.clearTimeout(id));
  }, []);

  // Clicking a native notification opens the thread it named (#27). The Tauri
  // layer has already brought the window back by the time this arrives, so the
  // only thing left is to point the main pane at the right row — after asking
  // the host for it, because the thread the banner is about was very likely
  // folded out of the sidebar until a moment ago. A thread that really is gone
  // falls through to the existing "check the Inbox" pane, which is the honest
  // answer rather than an error.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void onNotificationActivated(({ threadId }) => {
      registered.reload();
      setSelection({ view: "thread", threadId });
    }).then((off) => {
      if (cancelled) off();
      else unlisten = off;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [registered.reload]);

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
   * Fold a thread: hide the row, keep the work.
   *
   * A row the host owns folds on the host, because the fold *is* a row on disk
   * and the policy the permission broker reads while the user is away (#26).
   * Only the fixtures fall back to the reducer. `policy` is left out for
   * "Disappear until done" on purpose — that gesture keeps whatever policy the
   * thread already had (`state-machine.md`), and both paths honour it.
   */
  function foldThread(threadId: string, policy?: FoldPolicy) {
    const onHost = client !== null && isHostThread(threadId);
    leaveThread(threadId, () => {
      if (!onHost) {
        dispatch({ type: "foldThread", threadId, policy });
        return;
      }
      void fold({ threadId, policy }).then((folded) => {
        // The row is out of the sidebar; do not leave the main pane pointed at
        // a thread the user just sent away. A refused fold keeps them there.
        if (!folded) return;
        setSelection((current) =>
          current.view === "thread" && current.threadId === threadId
            ? { view: "inbox" }
            : current,
        );
      });
    });
  }

  /**
   * Archive a thread: close it out for good.
   *
   * A row the host owns is archived on the host, because the archive is what
   * withdraws the outstanding permissions, drains the queued prompts, closes
   * the run and releases the worktree (#26). Only the fixtures fall back to the
   * reducer — a `dispatch` here for a real thread would animate the row away
   * and leave every one of those still running.
   */
  function archiveThread(threadId: string) {
    const onHost = client !== null && isHostThread(threadId);
    leaveThread(threadId, () => {
      if (!onHost) {
        dispatch({ type: "archiveThread", threadId });
        return;
      }
      void archive(threadId).then((archived) => {
        if (!archived) return;
        setSelection((current) =>
          current.view === "thread" && current.threadId === threadId
            ? { view: "inbox" }
            : current,
        );
      });
    });
  }

  /** Delete a thread. Same split as archive, and the same reason. */
  function deleteThread(threadId: string) {
    const onHost = client !== null && isHostThread(threadId);
    leaveThread(threadId, () => {
      if (!onHost) {
        dispatch({ type: "deleteThread", threadId });
        return;
      }
      void remove(threadId).then((deleted) => {
        if (!deleted) return;
        setSelection((current) =>
          current.view === "thread" && current.threadId === threadId
            ? { view: "inbox" }
            : current,
        );
      });
    });
  }

  /**
   * Open the thread an Inbox card is about.
   *
   * Opening it is a state change, not a navigation: `thread/reopen` clears the
   * thread's badge, puts an archived thread's worktree back, and moves the row
   * out of Still Sleeping and into the sidebar (#22). The hook decides whether
   * this thread is somewhere `reopen` is legal from; the pane points at it
   * either way, because a card the user clicked has to open something.
   */
  function openInboxThread(threadId: string) {
    setSelection({ view: "thread", threadId });
    void inbox.open(threadId);
  }

  /**
   * Answering a notice card fades it out; this is what finally takes it out of
   * the transcript. `resolved` alone would leave an invisible card holding its
   * full height and still being read out.
   *
   * Chief's "Disappear until done" is the same fold as the sidebar's, aimed at
   * whatever thread the card names — so a card about a *host* thread has to
   * reach the host. The reducer's own fold only ever finds the fixtures, which
   * is why this runs beside it rather than inside it.
   */
  function answerNotice(
    conversationId: string,
    itemId: string,
    actionId: string,
  ) {
    const threadId = noticeThreadId(state, conversationId, itemId);
    if (
      actionId === "fold" &&
      threadId &&
      client &&
      isHostThread(threadId)
    ) {
      void fold({ threadId });
    }
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
          // Advanced, and both undefined unless the card was opened and used
          // (#23). A base ref the repo does not have comes back as
          // WORKTREE_FAILED, which the catch below already puts on the card.
          useCheckout: draft.useCheckout,
          baseRef: draft.baseRef,
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
  const editingSchedule = scheduleEditor.open
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
        // Only for folders the host actually has. A fixture folder has nothing
        // `folder/update` could be pointed at.
        onFolderSettings={
          client && registered.folders ? setFolderSettings : undefined
        }
        selection={selection}
        // The host's own count, not a second classification of the rows this
        // renderer happens to be holding: `count_unread_inbox` is the badge
        // `resurface.md` specifies, and it is the number the phone already
        // draws. Two devices disagreeing about one host would be two products.
        inboxCount={inbox.unread ?? needsYouCount(state)}
        openPrCount={openPrCount(state)}
        userName={profile.userName}
        hostLine={hostLine(hello, hostError, connecting)}
        hostOffline={hostError !== null}
        leavingThreadIds={leaving}
        onSelectBot={(botId) => setSelection({ view: "bot", botId })}
        onSelectThread={(threadId) =>
          setSelection({ view: "thread", threadId })
        }
        onOpenCrew={() => setSelection({ view: "crew" })}
        onOpenInbox={() => setSelection({ view: "inbox" })}
        onOpenPullRequests={() => setSelection({ view: "prs" })}
        onOpenSchedules={() => setSelection({ view: "schedules" })}
        onOpenSettings={client ? () => setSelection({ view: "settings" }) : undefined}
        onNewChat={(folderId) => setNewChat({ open: true, folderId })}
        onThreadMenu={(thread, position) => setMenu({ thread, position })}
      />

      {foldError && (
        <div className="app-error" role="alert">
          <span>{foldError}</span>
          <button type="button" onClick={clearFoldError} aria-label="Dismiss">
            <CrossIcon />
          </button>
        </div>
      )}

      <main className="main">
        <MainView
          client={client}
          state={state}
          inbox={inbox}
          schedules={schedules}
          settings={settings}
          pulls={pulls}
          github={github}
          onSignIn={() => setSignIn(true)}
          onEditSchedule={(scheduleId) =>
            setScheduleEditor({ open: true, scheduleId })
          }
          bots={bots}
          tools={[...toolChips, ...hostToolChips]}
          harnesses={harnesses}
          hostThreads={hostThreads}
          resolvedThread={resolved}
          selection={selection}
          host={host}
          onSelect={setSelection}
          onFoldThread={foldThread}
          onSend={(conversationId, text) =>
            dispatch({ type: "sendMessage", conversationId, text })
          }
          onNotice={answerNotice}
          onOpenInboxThread={openInboxThread}
          onInboxAction={(cardId, actionId) => {
            // A host card's buttons reach the host: Archive is `thread/archive`
            // and an `ask:` button is `permission/reply`. The reducer only ever
            // knew how to hide a fixture.
            if (inbox.cards) {
              void inbox.act(cardId, actionId);
              return;
            }
            if (actionId === "archive") {
              dispatch({ type: "dismissInboxCard", cardId });
            }
          }}
          onEditBot={(botId) => setEditor({ open: true, botId })}
          onAddBot={() => setEditor({ open: true, botId: null })}
          onRemoveBot={(botId) => removeBot(botId, false)}
          onRunSetup={onRunSetup}
        />
      </main>

      {signIn && (
        <GithubSignInModal
          host={github.status?.host ?? "github.com"}
          // Absent status means the host has not answered yet; assume `gh` is
          // there rather than showing an install line we have no evidence for.
          installed={github.status?.installed !== false}
          installHint={github.status?.remedy}
          onSignIn={async (token) => {
            await github.signIn(token);
            // The board's own poll is up to a minute away, and somebody who
            // just signed in is looking at it now.
            pulls.reload();
          }}
          onCancel={() => setSignIn(false)}
          onOpenUrl={(url) => window.open(url, "_blank", "noopener,noreferrer")}
        />
      )}

      {addFolder && (
        <AddFolderModal
          onRegister={(params: FolderRegisterParams) =>
            registered.register(params)
          }
          onCancel={() => setAddFolder(false)}
        />
      )}

      {settingsFolder && (
        <FolderSettingsModal
          folder={settingsFolder}
          onSave={registered.update}
          onCancel={() => setFolderSettings(null)}
        />
      )}

      {newChat.open && (
        <NewChatModal
          harnesses={harnesses}
          folders={registered.folders ?? state.folders}
          defaultFolderId={newChat.folderId}
          defaultHarnessId={profile.harnessId ?? undefined}
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

      {scheduleEditor.open && editingSchedule && (
        <ScheduleEditorModal
          schedule={{
            scheduleId: editingSchedule.scheduleId,
            botId: editingSchedule.botId,
            name: editingSchedule.name,
            cron: editingSchedule.cron,
            prompt: editingSchedule.prompt,
            catchUp: editingSchedule.catchUp,
          }}
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
          threadState={menu.thread.state}
          position={menu.position}
          onClose={() => setMenu(null)}
          onFold={(policy) => foldThread(menu.thread.id, policy)}
          onArchive={() => archiveThread(menu.thread.id)}
          onDelete={() => deleteThread(menu.thread.id)}
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
  inbox,
  schedules,
  settings,
  pulls,
  github,
  onSignIn,
  onEditSchedule,
  bots,
  tools,
  harnesses,
  hostThreads,
  resolvedThread,
  selection,
  host,
  onSelect,
  onFoldThread,
  onSend,
  onNotice,
  onOpenInboxThread,
  onInboxAction,
  onEditBot,
  onAddBot,
  onRemoveBot,
  onRunSetup,
}: {
  /** Present once the host has answered. A thread the host owns is rendered
      live — hydrated from `thread/transcript` and streamed from there (#14). */
  client: HostClient | null;
  state: MockState;
  /** The Inbox, host-owned from the first answer (#22). */
  inbox: HostInbox;
  /** Recurring jobs, host-owned from the first answer (#25). */
  schedules: Schedules;
  /** App-wide preferences (#26). */
  settings: Settings;
  /** The PR board, host-owned from the first answer (#28). */
  pulls: PullRequests;
  /** Whether GitHub can be asked as anybody, and as whom (#16). */
  github: GithubAuth;
  /** Open the GitHub sign-in dialog. */
  onSignIn: () => void;
  onEditSchedule: (scheduleId: string) => void;
  /** The crew, host-owned once `crew/list` has answered (#17). */
  bots: readonly Bot[];
  /** Every chip a crew card may have to name: the MCP catalog plus Chief's
      host tools, which are in no `tools/list`. */
  tools: readonly ToolOption[];
  harnesses: readonly HarnessCard[];
  /** Rows the host owns. Looked up before the fixtures, because a folder the
      host registered lists threads the mock reducer has never heard of. */
  hostThreads: readonly ThreadSummary[];
  /** The selected thread when the host owns it but no folder lists it — a
      bot's standing thread. Resolved one id at a time; see `useHostThread`. */
  resolvedThread: ThreadSummary | null;
  selection: Selection;
  host: HostTarget;
  onSelect: (selection: Selection) => void;
  /** Fold from the chat you are reading, not only from the sidebar row (#26). */
  onFoldThread: (threadId: string, policy?: FoldPolicy) => void;
  onSend: (conversationId: string, text: string) => void;
  onNotice: (conversationId: string, itemId: string, actionId: string) => void;
  /** Open a card's thread — a reopen on the host, not just a navigation. */
  onOpenInboxThread: (threadId: string) => void;
  onInboxAction: (cardId: string, actionId: string) => void;
  onEditBot: (botId: string) => void;
  onAddBot: () => void;
  onRemoveBot: (botId: string) => void;
  /** Wipe the first-run record and re-enter setup. Surfaced in Crew. */
  onRunSetup: () => void;
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
          onRunSetup={onRunSetup}
        />
      );
    case "inbox":
      return (
        <InboxView
          // The fixtures stand in only until the host has answered — `null` is
          // "not asked yet" (a preview build, a unit test), and an empty array
          // is the real answer of a morning with nothing waiting.
          cards={inbox.cards ?? state.inbox}
          error={inbox.error}
          loading={inbox.loading && inbox.cards === null}
          onOpenThread={onOpenInboxThread}
          onAction={onInboxAction}
          notify={inbox.notify}
        />
      );
    case "settings":
      return (
        <SettingsView
          settings={settings.settings}
          error={settings.error}
          // The promise is handed to the pane rather than resolved here: it
          // keeps what was typed and shows the host's own refusal, which is
          // the sentence worth reading.
          onSave={settings.save}
        />
      );
    case "schedules":
      return (
        <SchedulesView
          schedules={schedules.schedules}
          bots={bots}
          error={schedules.error}
          onReload={schedules.reload}
          // The prompt keeps the draft and says why, so the promise is handed
          // straight to it rather than resolved into a shell-level error.
          onCreate={(draft) => schedules.save(null, draft)}
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
          // The fixtures stand in only until the host has answered — `null` is
          // "not asked yet" (a preview build, a unit test), and an empty array
          // is the real and common answer of "no open pull requests".
          pullRequests={pulls.pullRequests ?? state.pullRequests}
          unavailable={pulls.unavailable}
          error={pulls.error}
          githubStatus={github.status}
          account={pulls.account}
          onSignIn={onSignIn}
          onRefresh={() => void pulls.refresh()}
          onOpenThread={(threadId) => onSelect({ view: "thread", threadId })}
          onAction={(prId, actionId) => {
            if (actionId !== "diff") return;
            const pr = (pulls.pullRequests ?? state.pullRequests).find(
              (row) => row.id === prId,
            );
            // The PR itself is on GitHub; JaBot has no in-app diff for it and
            // `pr-linkage.md` defers one. Opening the page is the honest verb.
            if (pr) window.open(pr.url, "_blank", "noopener,noreferrer");
          }}
        />
      );
    case "thread": {
      const hostThread =
        hostThreads.find((t) => t.id === selection.threadId) ??
        (resolvedThread?.id === selection.threadId ? resolvedThread : undefined);
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
            onFold={(policy) => onFoldThread(hostThread.id, policy)}
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
          onAction={(itemId, actionId) => onNotice(thread.id, itemId, actionId)}
          onFold={(policy) => onFoldThread(thread.id, policy)}
        />
      );
    }
    case "bot": {
      const bot = bots.find((b) => b.id === selection.botId);
      if (!bot) return <div className="view" />;
      // A bot the host serves gets its real standing thread (#24). The
      // fixtures stay as the fallback for the same reason the thread case
      // keeps them — the shell renders before a host has answered — and the
      // method lookup is part of the test: a transport that predates
      // `crew/thread` has no thread to open, and a preview build drawing its
      // own fixtures is a better answer than an error where a chat should be.
      if (client && typeof client.botThread === "function") {
        return (
          <LiveChatView key={bot.id} client={client} bot={bot} host={host} />
        );
      }
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
