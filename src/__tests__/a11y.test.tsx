/**
 * Automated accessibility coverage for the primary views (#179).
 *
 * Axe runs on the same jsdom unit project as everything else, so
 * `./scripts/verify.sh` and CI fail on a new critical or serious violation
 * without a second runner. The fixtures are the ones the existing view tests
 * already trust — the point is the name/role/structure of those screens, not
 * a second copy of their behaviour.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { NewChatView } from "../components/NewChatView";
import { Sidebar } from "../components/Sidebar";
import type {
  Bot,
  Folder,
  FolderWithThreads,
  HostTarget,
  PullRequest,
  Selection,
  ThreadSummary,
  TranscriptItem,
} from "../components/types";
import { expectNoSeriousA11yViolations } from "../../tests/support/a11y";
import { SettingsView } from "../views/SettingsView";
import { ThreadView } from "../views/ThreadView";
import { PullRequestsView } from "../views/PullRequestsView";
import { HARNESSES } from "../views/mock-host";
import type { PairedDeviceView, SettingsView as HostSettings } from "../host";

const BOTS: Bot[] = [
  {
    id: "chief",
    name: "Chief",
    color: "b-teal",
    instructions: "Route work.",
    tools: [],
    harnessId: "claude",
    isChief: true,
  },
  {
    id: "code",
    name: "Code",
    color: "b-yellow",
    instructions: "Run coding sessions.",
    tools: ["github"],
    harnessId: "claude",
    isChief: false,
    unread: true,
    preview: "Opened PR #23 — checks are green.",
  },
];

const FOLDERS: FolderWithThreads[] = [
  {
    id: "jabot-app",
    name: "jabot-app",
    path: "~/code/jabot-app",
    threads: [
      {
        id: "auth",
        folderId: "jabot-app",
        botId: "code",
        harnessId: "claude",
        title: "Auth migration",
        state: "active",
        foldPolicy: "default",
        runState: "running",
      },
    ],
  },
];

const FOLDER_CHOICES: Folder[] = [
  { id: "jabot-app", name: "jabot-app", path: "~/code/jabot-app" },
];

const HOST: HostTarget = { hostId: "h1", name: "This Mac", reachable: true };

const THREAD: ThreadSummary = {
  id: "auth",
  folderId: "jabot-app",
  botId: "code",
  harnessId: "claude",
  title: "Auth migration",
  state: "active",
  foldPolicy: "default",
  runState: "running",
};

const THREAD_ITEMS: TranscriptItem[] = [
  { kind: "stamp", id: "s1", text: "Today" },
  { kind: "user", id: "u1", text: "Start the auth migration." },
  { kind: "agent", id: "a1", text: "Reading the session store first." },
  {
    kind: "tool",
    id: "t1",
    call: {
      id: "c1",
      kind: "read",
      target: "src/session.ts",
      status: "completed",
    },
  },
  {
    kind: "tool",
    id: "t2",
    call: {
      id: "c2",
      kind: "execute",
      target: "npm test",
      status: "in_progress",
    },
  },
  {
    kind: "notice",
    id: "n1",
    title: "Run npm test",
    pill: "execute",
    body: "The agent wants to run the test suite.",
    threadId: "auth",
    actions: [
      { id: "allow_once", label: "Allow", primary: true },
      { id: "reject_once", label: "Deny" },
    ],
  },
];

const SETTINGS: HostSettings = {
  idleTimeoutMs: 600_000,
  defaultFoldPolicy: "default",
  idleTimeoutFromEnv: false,
};

const DEVICE: PairedDeviceView = {
  deviceId: "dev-phone",
  name: "Jabree's iPhone",
  role: "approver",
  fingerprint: "ZZZZyyyyXXXXwwwwVVVVuuuuTTTT",
  pairedVia: "qr",
  sas: "1174-6602",
  createdAt: "2026-08-12T18:20:00Z",
  lastSeenAt: "2026-08-29T21:40:00Z",
  local: false,
  connected: false,
};

const NOW = new Date("2026-08-20T14:30:00Z");

const PRS: PullRequest[] = [
  {
    id: "pr-23",
    threadId: "auth",
    provider: "github",
    repo: "jabot-app",
    number: 23,
    url: "https://example.invalid/23",
    title: "Migrate auth to sessions",
    status: "open",
    checkState: "passing",
    updatedAt: "2026-08-20T13:52:00Z",
    additions: 214,
    deletions: 96,
    headRef: "auth/sessions",
    baseRef: "main",
    filesChanged: 3,
    detail: {
      checks: [
        { label: "48 tests passing", state: "passing" },
        { label: "lint", state: "passing" },
      ],
      bullets: ["Flagged: 30-day cookie expiry kept"],
      actions: [
        { id: "merge", label: "Merge", primary: true },
        { id: "reopen", label: "Reopen thread" },
      ],
    },
  },
  {
    id: "pr-21",
    threadId: "retry",
    provider: "github",
    repo: "globnet-sync",
    number: 21,
    url: "https://example.invalid/21",
    title: "Add retry logic to NAS backup",
    status: "open",
    checkState: "running",
    updatedAt: "2026-08-20T11:24:00Z",
    additions: 64,
    deletions: 12,
  },
];

describe("a11y gate", () => {
  it("fails on a nameless button, so the gate is not a no-op", async () => {
    const { container } = render(<button type="button" />);
    await expect(expectNoSeriousA11yViolations(container)).rejects.toThrow(
      /button-name/,
    );
  });
});

describe("primary views", () => {
  it("sidebar has no critical or serious violations", async () => {
    const { container } = render(
      <Sidebar
        bots={BOTS}
        folders={FOLDERS}
        selection={{ view: "bot", botId: "chief" } as Selection}
        inboxCount={2}
        openPrCount={4}
        userName="Jabree Flor"
        hostLine="This Mac · v0.1.0"
        onSelectBot={vi.fn()}
        onSelectThread={vi.fn()}
        onOpenCrew={vi.fn()}
        onOpenInbox={vi.fn()}
        onOpenPullRequests={vi.fn()}
        onOpenSchedules={vi.fn()}
        onOpenSettings={vi.fn()}
        onNewChat={vi.fn()}
        onThreadMenu={vi.fn()}
        onToggle={vi.fn()}
      />,
    );

    await expectNoSeriousA11yViolations(container);
  });

  it("chat thread has no critical or serious violations", async () => {
    const { container } = render(
      <ThreadView
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
        items={THREAD_ITEMS}
        onSend={vi.fn()}
        onAction={vi.fn()}
        onPickHost={vi.fn()}
        onFold={vi.fn()}
        busy
        queued={["and then fold it"]}
        onCancel={vi.fn()}
      />,
    );

    await expectNoSeriousA11yViolations(container);
  });

  it("Settings has no critical or serious violations", async () => {
    const { container } = render(
      <SettingsView
        settings={SETTINGS}
        onSave={vi.fn(async () => SETTINGS)}
        onRunSetup={vi.fn()}
        devices={[DEVICE]}
        devicesError={null}
        onReloadDevices={vi.fn()}
        onRevokeDevice={vi.fn(async () => undefined)}
      />,
    );

    await expectNoSeriousA11yViolations(container);

    await userEvent.click(screen.getByRole("tab", { name: "Devices" }));
    await expectNoSeriousA11yViolations(container);
  });

  it("PR board has no critical or serious violations", async () => {
    const { container } = render(
      <PullRequestsView
        pullRequests={PRS}
        now={NOW}
        onOpenThread={vi.fn()}
        onAction={vi.fn()}
        onRefresh={vi.fn()}
        onSignIn={vi.fn()}
        githubStatus={{
          installed: true,
          authenticated: false,
          host: "github.com",
          detail: "signed out",
        }}
      />,
    );

    await expectNoSeriousA11yViolations(container);
  });

  it("New Chat and the open harness picker have no critical or serious violations", async () => {
    const { container } = render(
      <NewChatView
        harnesses={HARNESSES}
        folders={FOLDER_CHOICES}
        onStart={vi.fn()}
      />,
    );

    await expectNoSeriousA11yViolations(container);

    await userEvent.click(screen.getByRole("button", { name: /Harness:/ }));
    expect(screen.getByRole("option", { name: /Claude Code/ })).toBeInTheDocument();
    await expectNoSeriousA11yViolations(container);
  });
});
