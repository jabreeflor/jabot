/**
 * End-to-end: one host-owned worktree per concurrent code thread (#23).
 *
 * `src-tauri/src/host/git/` proves the rules in-process. This file makes the
 * claim a user actually depends on, through the production `HostClient`, a live
 * `jabot-hostd`, a real SQLite store and a real `git` on a real repository:
 *
 * - two threads in one folder get two checkouts on two branches, and neither is
 *   the folder the user has open in their editor;
 * - a fresh tree is set up — the gitignored files a project needs are in it;
 * - archiving removes the tree and does **not** remove the work: uncommitted
 *   changes become a commit on the thread's own `jabot/<id>` branch, which is
 *   the answer to "where did my edits go";
 * - a thread with no repo gets no tree at all (decision #6);
 * - and the whole thing survives a host restart, because the row on disk is
 *   what says which directory belongs to which thread.
 *
 * `git` is driven directly here rather than through a helper: what is being
 * asserted is what git itself believes about the repository afterwards.
 */
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import { RPC_ERROR } from "../../src/host/protocol";
import { HostdProcess, type HostdOptions } from "../support/hostd";

const running: HostdProcess[] = [];

async function connected(options: HostdOptions = { persistent: true }) {
  const host = new HostdProcess(options);
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  await client.hello();
  return { host, client };
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
});

function git(cwd: string, ...args: string[]): string {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    env: {
      ...process.env,
      GIT_CONFIG_GLOBAL: "/dev/null",
      GIT_CONFIG_SYSTEM: "/dev/null",
    },
  }).trim();
}

/** A real repository with one commit, and one gitignored file beside it. */
function repository(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-repo-"));
  git(dir, "init", "--initial-branch=main");
  git(dir, "config", "user.email", "test@example.com");
  git(dir, "config", "user.name", "Test");
  writeFileSync(path.join(dir, ".gitignore"), ".env\n");
  writeFileSync(path.join(dir, "README.md"), "# project\n");
  git(dir, "add", "-A");
  git(dir, "commit", "-m", "first");
  writeFileSync(path.join(dir, ".env"), "TOKEN=secret\n");
  return dir;
}

describe("worktrees over the host protocol", () => {
  it("gives every code thread in a folder its own checkout and branch", async () => {
    const repo = repository();
    const { host, client } = await connected();
    const folder = await client.registerFolder({
      path: repo,
      filesToCopy: [".env"],
    });

    const first = await client.openThread({
      threadId: "t-auth",
      title: "Auth migration",
      cwd: folder.cwd,
      harnessId: "claude",
      folderId: folder.folderId,
    });
    const second = await client.openThread({
      threadId: "t-sidebar",
      title: "Sidebar overflow",
      cwd: folder.cwd,
      harnessId: "claude",
      folderId: folder.folderId,
    });

    // The prototype shows both of these running at once. Sharing one directory
    // is the collision this issue exists to remove.
    expect(first.worktreePath).toBeTruthy();
    expect(second.worktreePath).toBeTruthy();
    expect(first.worktreePath).not.toBe(second.worktreePath);
    expect(first.branch).not.toBe(second.branch);
    expect(first.branch?.startsWith("jabot/")).toBe(true);

    // cwd is the tree, not the folder: what ACP is handed is the isolated one.
    expect(first.cwd).toBe(first.worktreePath);
    expect(first.cwd).not.toBe(folder.cwd);
    // Host-owned means under the app's data directory, never inside the repo.
    expect(first.worktreePath?.startsWith(path.join(host.dataDir!, "worktrees"))).toBe(true);
    expect(first.repoRoot).toBe(folder.repoRoot);

    // A tracked file is there because it is a checkout; the ignored one is
    // there because the folder said to copy it (#16 records it, #23 uses it).
    expect(existsSync(path.join(first.worktreePath!, "README.md"))).toBe(true);
    expect(readFileSync(path.join(first.worktreePath!, ".env"), "utf8")).toBe("TOKEN=secret\n");

    // And the user's own checkout is exactly as they left it.
    expect(git(repo, "branch", "--show-current")).toBe("main");
    expect(git(repo, "status", "--porcelain")).toBe("");
  });

  it("archives without losing uncommitted work, and the tree does not outlive the thread", async () => {
    const repo = repository();
    const { client } = await connected();
    const folder = await client.registerFolder({ path: repo });
    const thread = await client.openThread({
      threadId: "t-archive",
      title: "Auth migration",
      cwd: folder.cwd,
      harnessId: "claude",
      folderId: folder.folderId,
    });
    const tree = thread.worktreePath!;
    const branch = thread.branch!;

    // What an agent leaves behind mid-task: edits nobody committed.
    writeFileSync(path.join(tree, "auth.ts"), "export const login = () => {};\n");

    const archived = await client.archiveThread({ threadId: "t-archive" });
    expect(archived.state).toBe("archived");
    expect(existsSync(tree)).toBe(false);
    expect(archived.worktreePath).toBeUndefined();

    // The work is a commit on the thread's branch — recoverable by hand with
    // `git checkout`, and never silently deleted.
    expect(git(repo, "show", `${branch}:auth.ts`)).toBe("export const login = () => {};");
    expect(git(repo, "worktree", "list", "--porcelain")).not.toContain(tree);
  });

  it("gives a thread with no repository no worktree at all", async () => {
    const plain = mkdtempSync(path.join(tmpdir(), "jabot-notes-"));
    const repo = repository();
    const { client } = await connected();

    // A worker's standing thread: no folder, no repo, no tree (decision #6).
    const standing = await client.openThread({
      threadId: "t-inbox-mgr",
      title: "Inbox Mgr",
      cwd: plain,
      harnessId: "claude",
    });
    expect(standing.worktreePath).toBeUndefined();
    expect(standing.cwd).toBe(plain);

    // The advanced opt-out: work in the checkout I already have open.
    const folder = await client.registerFolder({ path: repo });
    const shared = await client.openThread({
      threadId: "t-shared",
      title: "Quick fix",
      cwd: folder.cwd,
      harnessId: "claude",
      folderId: folder.folderId,
      useCheckout: true,
    });
    expect(shared.worktreePath).toBeUndefined();
    expect(shared.cwd).toBe(folder.cwd);
  });

  it("refuses the spawn when the requested base ref does not exist", async () => {
    const repo = repository();
    const { client } = await connected();
    const folder = await client.registerFolder({ path: repo });

    // A refusal, not a quiet fall back to the user's checkout: falling back is
    // two agents and a human in one directory.
    const failure = await client
      .openThread({
        threadId: "t-bad-base",
        title: "From a tag that is not there",
        cwd: folder.cwd,
        harnessId: "claude",
        folderId: folder.folderId,
        baseRef: "v9.9.9",
      })
      .catch((err: unknown) => err);
    expect(failure).toBeInstanceOf(HostRpcError);
    expect((failure as HostRpcError).code).toBe(RPC_ERROR.WORKTREE_FAILED);

    // Nothing half-made: no thread, and no branch minted for one.
    await expect(client.threadState({ threadId: "t-bad-base" })).rejects.toBeInstanceOf(
      HostRpcError,
    );
    expect(git(repo, "branch", "--list", "jabot/*")).toBe("");
  });

  it("keeps each thread's tree across a host restart", async () => {
    const repo = repository();
    const dataDir = mkdtempSync(path.join(tmpdir(), "jabot-hostd-"));
    const first = await connected({ dataDir });
    const folder = await first.client.registerFolder({ path: repo });
    const opened = await first.client.openThread({
      threadId: "t-restart",
      title: "Auth migration",
      cwd: folder.cwd,
      harnessId: "claude",
      folderId: folder.folderId,
    });
    const tree = opened.worktreePath!;
    writeFileSync(path.join(tree, "wip.ts"), "// still going\n");
    await first.host.dispose();

    const second = await connected({ dataDir });
    const after = await second.client.threadState({ threadId: "t-restart" });

    // The boot sweep collects trees nobody claims. This one is claimed, so it
    // and everything uncommitted in it has to still be there — a sweep that
    // took it would delete a folded agent's work on every launch.
    expect(after.worktreePath).toBe(tree);
    expect(after.cwd).toBe(tree);
    expect(existsSync(path.join(tree, "wip.ts"))).toBe(true);
    expect(git(tree, "status", "--porcelain")).toContain("wip.ts");
  });
});
