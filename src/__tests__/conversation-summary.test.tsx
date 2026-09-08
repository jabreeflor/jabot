/**
 * Code conversation summary popover (#269).
 *
 * The header used to name the job and the engine and stop there. The panel
 * is the rest of the location: which checkout, whether it is dirty, and
 * which files the person attached — so Git actions have a repository they
 * obviously apply to.
 */
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import { ConversationSummary } from "../components/ConversationSummary";
import { ThreadView } from "../views/ThreadView";
import {
  JSONRPC_VERSION,
  type HostClient,
  type JsonRpcNotification,
  type ThreadSummaryResult,
} from "../host";
import type {
  HarnessCard,
  HostTarget,
  ThreadSummary,
} from "../components/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

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

  it("names a missing checkout and an empty Git state", async () => {
    const host = client({
      threadId: "t-auth",
      selectedRepoId: "f1",
      repositories: [
        {
          id: "f1",
          name: "gone",
          primary: true,
          environment: "Local",
          isGit: true,
          available: false,
          status: "unavailable",
        },
      ],
      sources: [],
      availableFolders: [],
    });
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for gone",
      }),
    );
    expect(
      await screen.findByText("gone is not available on disk."),
    ).toBeInTheDocument();

    const empty = client({
      threadId: "t-auth",
      selectedRepoId: "f1",
      repositories: [
        {
          id: "f1",
          name: "fresh",
          primary: true,
          environment: "Local",
          isGit: true,
          available: true,
          status: "empty",
        },
      ],
      sources: [],
      availableFolders: [],
    });
    render(<ConversationSummary client={empty} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for fresh",
      }),
    );
    expect(
      await screen.findByText("Git state is not available yet."),
    ).toBeInTheDocument();
  });

  it("says when no repository is attached", async () => {
    const host = client({
      threadId: "t-auth",
      selectedRepoId: "",
      repositories: [],
      sources: [],
      availableFolders: [],
    });
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      screen.getByRole("button", {
        name: "Conversation summary for Repositories",
      }),
    );
    expect(
      await screen.findByText(
        "No repository is attached to this conversation.",
      ),
    ).toBeInTheDocument();
  });

  it("surfaces a load error from the host", async () => {
    const host = {
      ...client(),
      threadSummary: vi.fn(async () => {
        throw new Error("host is down");
      }),
    } as unknown as HostClient;
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      screen.getByRole("button", {
        name: "Conversation summary for Repositories",
      }),
    );
    expect(await screen.findByText("host is down")).toBeInTheDocument();
  });

  it("attaches an extra folder from the picker", async () => {
    const next: ThreadSummaryResult = {
      ...SUMMARY,
      selectedRepoId: "f3",
      repositories: [
        ...SUMMARY.repositories,
        {
          id: "f3",
          name: "docs",
          primary: false,
          environment: "Local",
          isGit: true,
          available: true,
          status: "ok",
          branch: "main",
          additions: 0,
          deletions: 0,
        },
      ],
      availableFolders: [],
    };
    const host = client({
      ...SUMMARY,
      availableFolders: [
        { folderId: "f3", name: "docs", path: "/tmp/docs", isGit: true },
      ],
    });
    host.attachThreadRepo = vi.fn(async () => next);
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "Selected repository jabot" }),
    );
    await userEvent.click(screen.getByRole("option", { name: "Add docs" }));
    await waitFor(() =>
      expect(host.attachThreadRepo).toHaveBeenCalledWith({
        threadId: "t-auth",
        folderId: "f3",
      }),
    );
    expect(
      await screen.findByRole("button", { name: "Selected repository docs" }),
    ).toBeInTheDocument();
  });

  it("commits and pushes from the commit modal", async () => {
    const host = client();
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "Commit or push" }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Commit or push jabot" }),
    ).toBeInTheDocument();

    const message = screen.getByLabelText("Commit message");
    await userEvent.click(screen.getByRole("button", { name: "Commit" }));
    expect(host.threadGitCommit).not.toHaveBeenCalled();

    await userEvent.type(message, "cover the commit path");
    await userEvent.click(screen.getByRole("button", { name: "Commit" }));
    await waitFor(() =>
      expect(host.threadGitCommit).toHaveBeenCalledWith({
        threadId: "t-auth",
        repoId: "f1",
        message: "cover the commit path",
      }),
    );
  });

  it("reports a failed push and a successful one", async () => {
    const host = client();
    host.threadGitPush = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, detail: "rejected by remote" })
      .mockResolvedValueOnce({ ok: true });
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "Commit or push" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Push" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "rejected by remote",
    );

    await userEvent.click(screen.getByRole("button", { name: "Push" }));
    await waitFor(() => expect(host.threadGitPush).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(host.threadSummary).toHaveBeenCalled());
  });

  it("opens a compare URL and the pull request callback", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    const onOpenPullRequest = vi.fn();
    const host = client({
      ...SUMMARY,
      repositories: [
        {
          ...SUMMARY.repositories[0],
          compareUrl: "https://github.com/jabot/compare/main...dev",
          pullRequestUrl: "https://github.com/jabot/pull/1",
        },
        SUMMARY.repositories[1],
      ],
    });
    render(
      <ConversationSummary
        client={host}
        threadId="t-auth"
        onOpenPullRequest={onOpenPullRequest}
      />,
    );
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Compare branch" }),
    );
    expect(open).toHaveBeenCalledWith(
      "https://github.com/jabot/compare/main...dev",
      "_blank",
      "noopener",
    );
    expect(onOpenPullRequest).toHaveBeenCalledWith(
      "https://github.com/jabot/pull/1",
    );
    open.mockRestore();
  });

  it("adds a picked source and paints an https thumbnail", async () => {
    vi.mocked(invoke).mockResolvedValue(["/tmp/extra.md"]);
    const host = client({
      ...SUMMARY,
      sources: [
        {
          id: "s-img",
          name: "shot.png",
          kind: "file",
          path: "https://example.com/shot.png",
          available: true,
        },
      ],
    });
    host.addThreadSource = vi.fn(async () => SUMMARY);
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    expect(
      screen
        .getByRole("button", { name: "Open shot.png" })
        .querySelector("img"),
    ).toHaveAttribute("src", "https://example.com/shot.png");
    await userEvent.click(screen.getByRole("button", { name: "Add source" }));
    await waitFor(() =>
      expect(host.addThreadSource).toHaveBeenCalledWith({
        threadId: "t-auth",
        path: "/tmp/extra.md",
      }),
    );
  });

  it("treats a cancelled source picker as no paths", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("cancelled"));
    const host = client();
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Add source" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("pick_sources"));
    expect(host.addThreadSource).not.toHaveBeenCalled();
  });

  it("reloads when the session updates and closes on an outside click", async () => {
    let notify: ((note: JsonRpcNotification) => void) | undefined;
    const host = client();
    host.onNotification = (listener) => {
      notify = listener;
      return () => {
        notify = undefined;
      };
    };
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Conversation summary" }),
    ).toBeInTheDocument();
    const before = vi.mocked(host.threadSummary).mock.calls.length;
    await act(async () => {
      notify?.({ jsonrpc: JSONRPC_VERSION, method: "session/update" });
    });
    await waitFor(() =>
      expect(vi.mocked(host.threadSummary).mock.calls.length).toBeGreaterThan(
        before,
      ),
    );
    fireEvent.mouseDown(document.body);
    expect(
      screen.queryByRole("dialog", { name: "Conversation summary" }),
    ).toBeNull();
  });

  it("opens a pull request from an empty review", async () => {
    const onOpenPullRequest = vi.fn();
    const host = client({
      ...SUMMARY,
      repositories: [
        {
          ...SUMMARY.repositories[0],
          pullRequestUrl: "https://github.com/jabot/pull/9",
        },
        SUMMARY.repositories[1],
      ],
    });
    host.threadGitDiff = vi.fn(async () => ({
      repoId: "f1",
      additions: 0,
      deletions: 0,
      files: [],
      patch: "",
    }));
    render(
      <ConversationSummary
        client={host}
        threadId="t-auth"
        onOpenPullRequest={onOpenPullRequest}
      />,
    );
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Inspect changes" }),
    );
    expect(
      await screen.findByText("No changes in this repository."),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "Open pull request" }),
    );
    expect(onOpenPullRequest).toHaveBeenCalledWith(
      "https://github.com/jabot/pull/9",
    );
  });

  it("says when the sources list is empty", async () => {
    const host = client({
      ...SUMMARY,
      sources: [],
    });
    render(<ConversationSummary client={host} threadId="t-auth" />);
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Conversation summary for jabot",
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: "View all" }));
    expect(
      await screen.findByText("No sources attached yet."),
    ).toBeInTheDocument();
  });
});
