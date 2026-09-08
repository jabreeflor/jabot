import { existsSync, writeFileSync } from "node:fs";
import path from "node:path";

import { expect, test } from "./fixtures";
import { tempRepository, breakCommitting, git } from "./helpers/git";
import {
  archiveThread,
  captureEvidence,
  deleteThread,
  openConnectedApp,
  startFolderSession,
} from "./ui";

test.describe("real worktree session", () => {
  test("starts a folder thread from the UI, then archives dirty work onto the branch", async ({
    page,
    jabot,
  }) => {
    const repo = tempRepository();
    await jabot.rpc("folder/register", {
      path: repo.dir,
      name: "demo-repo",
      filesToCopy: [".env"],
    });
    await openConnectedApp(page, jabot.baseURL);

    const task = "Auth migration";
    await startFolderSession(page, "demo-repo", task);
    await expect(page.getByText("hello from fake-acp")).toBeVisible();
    await expect(
      page.getByRole("button", { name: new RegExp(`^${task}`) }),
    ).toBeVisible();
    await captureEvidence(page, "worktree-session");

    const folders = await jabot.rpc<{
      folders: Array<{
        name: string;
        threads: Array<{ threadId: string; title: string }>;
      }>;
    }>("folder/list");
    const folder = folders.folders.find((row) => row.name === "demo-repo");
    expect(folder).toBeTruthy();
    const listed = folder!.threads.find((row) => row.title === task);
    expect(listed).toBeTruthy();
    const thread = await jabot.rpc<{
      worktreePath?: string;
      branch?: string;
    }>("thread/state", { threadId: listed!.threadId });
    expect(thread.worktreePath).toBeTruthy();
    expect(thread.branch?.startsWith("jabot/")).toBe(true);
    expect(
      thread.worktreePath!.startsWith(path.join(jabot.dataDir, "worktrees")),
    ).toBe(true);
    expect(existsSync(path.join(thread.worktreePath!, "README.md"))).toBe(true);
    expect(git(repo.dir, "branch", "--show-current")).toBe("main");

    writeFileSync(
      path.join(thread.worktreePath!, "auth.ts"),
      "export const login = () => {};\n",
    );

    await archiveThread(page, task);
    await expect(
      page.getByRole("button", { name: new RegExp(`^${task}`) }),
    ).toHaveCount(0);

    expect(existsSync(thread.worktreePath!)).toBe(false);
    expect(git(repo.dir, "show", `${thread.branch}:auth.ts`)).toBe(
      "export const login = () => {};",
    );

    const other = "Sidebar overflow";
    await startFolderSession(page, "demo-repo", other);
    await expect(page.getByRole("heading", { name: other })).toBeVisible();
    await expect(page.getByText("hello from fake-acp")).toBeVisible();
    await deleteThread(page, other);
    await expect(
      page.getByRole("button", { name: new RegExp(`^${other}`) }),
    ).toHaveCount(0);
  });

  test("keeps an unsaveable dirty tree when Archive cannot commit the work", async ({
    page,
    jabot,
  }) => {
    const repo = tempRepository();
    breakCommitting(repo.dir);
    await jabot.rpc("folder/register", { path: repo.dir, name: "broken-repo" });
    await openConnectedApp(page, jabot.baseURL);

    const task = "Unsaveable model";
    await startFolderSession(page, "broken-repo", task);
    await expect(page.getByText("hello from fake-acp")).toBeVisible();

    const folders = await jabot.rpc<{
      folders: Array<{
        threads: Array<{ title: string; threadId: string }>;
      }>;
    }>("folder/list");
    const listed = folders.folders
      .flatMap((folder) => folder.threads)
      .find((row) => row.title === task);
    expect(listed).toBeTruthy();
    const thread = await jabot.rpc<{ worktreePath?: string }>("thread/state", {
      threadId: listed!.threadId,
    });
    expect(thread.worktreePath).toBeTruthy();
    writeFileSync(
      path.join(thread.worktreePath!, "model.bin"),
      "an hour of work",
    );

    await archiveThread(page, task);
    expect(existsSync(thread.worktreePath!)).toBe(true);
    expect(
      (
        await jabot.rpc<{ worktreePath?: string }>("thread/state", {
          threadId: listed!.threadId,
        })
      ).worktreePath,
    ).toBe(thread.worktreePath);

    // Archive hid the row; delete is the verb that is allowed to collect a
    // tree the save could not keep. Protocol check — the row is already gone.
    await jabot.rpc("thread/delete", { threadId: listed!.threadId });
    expect(existsSync(thread.worktreePath!)).toBe(false);
  });
});
