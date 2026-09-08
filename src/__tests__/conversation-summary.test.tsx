/**
 * Code conversation summary popover (#269).
 *
 * The header used to name the job and the engine and stop there. The panel
 * is the rest of the location: which checkout, whether it is dirty, and
 * which files the person attached — so Git actions have a repository they
 * obviously apply to.
 */
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ConversationSummary } from "../components/ConversationSummary";
import { ThreadView } from "../views/ThreadView";
import type { HostClient, ThreadSummaryResult } from "../host";
import type {
  HarnessCard,
  HostTarget,
  ThreadSummary,
} from "../components/types";

const THREAD: ThreadSummary = {
  id: "t-auth",
  folderId: "f1",
  botId: null,
  harnessId: "claude",
  title: "Auth migration",
  state: "active",
  foldPolicy: "default",
  runState: null,
};

const HARNESSES: HarnessCard[] = [
  { id: "claude", label: "Claude Code", blurb: "", accent: "var(--h-claude)" },
];

const HOST: HostTarget = { hostId: "h1", name: "This Mac", reachable: true };

const SUMMARY: ThreadSummaryResult = {
  threadId: "t-auth",
  selectedRepoId: "f1",
  repositories: [
    {
      id: "f1",
      name: "jabot",
      primary: true,
      environment: "Local",
      isGit: true,
      available: true,
      status: "ok",
      branch: "main",
      additions: 1,
      deletions: 0,
      path: "/tmp/jabot",
    },
    {
      id: "f2",
      name: "jabot-frontend",
      primary: false,
      environment: "Local",
      isGit: true,
      available: true,
      status: "ok",
      branch: "dev",
      additions: 4,
      deletions: 2,
      path: "/tmp/frontend",
    },
  ],
  sources: [
    {
      id: "s1",
      name: "notes.md",
      kind: "file",
      path: "/tmp/notes.md",
      available: true,
    },
  ],
  availableFolders: [],
};

function client(summary: ThreadSummaryResult = SUMMARY): HostClient {
  return {
    threadSummary: vi.fn(async () => summary),
    attachThreadRepo: vi.fn(async () => summary),
    threadGitDiff: vi.fn(async () => ({
      repoId: "f1",
      additions: 1,
      deletions: 0,
      files: [
        { path: "added.rs", status: "added", additions: 1, deletions: 0 },
      ],
      patch: "+fn main() {}",
    })),
    threadGitCommit: vi.fn(async () => summary),
    threadGitPush: vi.fn(async () => ({ ok: true })),
    addThreadSource: vi.fn(async () => summary),
    openThreadSource: vi.fn(async () => ({ opened: true })),
    onNotification: () => () => {},
  } as unknown as HostClient;
}

describe("ConversationSummary", () => {
  it("opens from the header with the conversation's repos, counts, and sources", async () => {
    const host = client();
    render(
      <ThreadView
        thread={THREAD}
        harnesses={HARNESSES}
        host={HOST}
        items={[]}
        onSend={vi.fn()}
        client={host}
      />,
    );

    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Conversation summary" }),
    ).toBeInTheDocument();
    expect(host.threadSummary).toHaveBeenCalledWith({ threadId: "t-auth" });
    expect(
      await screen.findByRole("button", { name: "Selected repository jabot" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Changes in jabot: +1 −0" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Local")).toBeInTheDocument();
    expect(screen.getByText("main")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "jabot-frontend, +4 −2" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Sources" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Open notes.md" }),
    ).toBeInTheDocument();
  });

  it("applies Git actions to the selected repository", async () => {
    const host = client();
    render(<ConversationSummary client={host} threadId="t-auth" />);

    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await screen.findByRole("button", { name: "Selected repository jabot" });
    await userEvent.click(
      screen.getByRole("button", { name: "jabot-frontend, +4 −2" }),
    );
    expect(
      await screen.findByRole("button", {
        name: "Selected repository jabot-frontend",
      }),
    ).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Inspect changes" }),
    );
    await waitFor(() =>
      expect(host.threadGitDiff).toHaveBeenCalledWith({
        threadId: "t-auth",
        repoId: "f2",
      }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Changes in jabot-frontend" }),
    ).toBeInTheDocument();
  });

  it("opens a source and the full list", async () => {
    const host = client();
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "Open notes.md" }),
    );
    expect(host.openThreadSource).toHaveBeenCalledWith({
      threadId: "t-auth",
      sourceId: "s1",
    });
    await userEvent.click(screen.getByRole("button", { name: "View all" }));
    expect(screen.getByRole("dialog", { name: "Sources" })).toBeInTheDocument();
  });

  it("dismisses with Escape", async () => {
    const host = client();
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Conversation summary" }),
    ).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(
      screen.queryByRole("dialog", { name: "Conversation summary" }),
    ).toBeNull();
  });

  it("names empty, missing, and non-Git states", async () => {
    const host = client({
      threadId: "t-auth",
      selectedRepoId: "f1",
      repositories: [
        {
          id: "f1",
          name: "notes",
          primary: true,
          environment: "Local",
          isGit: false,
          available: true,
          status: "not_git",
        },
      ],
      sources: [],
      availableFolders: [],
    });
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for notes",
      }),
    );
    expect(
      await screen.findByText("notes is not a Git repository."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Inspect changes" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Commit or push" }),
    ).toBeDisabled();
  });

  it("says so when the host cannot answer", async () => {
    render(<ConversationSummary threadId="t-auth" />);
    await userEvent.click(
      screen.getByRole("button", {
        name: "Conversation summary for Repositories",
      }),
    );
    expect(
      await screen.findByText("Summary is unavailable on this host."),
    ).toBeInTheDocument();
  });
});
