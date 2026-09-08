/**
 * End-to-end: the Code conversation summary (#269).
 *
 * The panel is a host fact, not a renderer guess: change counts come from
 * git in the thread's worktree, extra repos are attached folders, and
 * sources are real paths. This file drives that through `jabot-hostd`.
 */
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
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

function repository(name: string): string {
  const dir = mkdtempSync(path.join(tmpdir(), `jabot-${name}-`));
  git(dir, "init", "--initial-branch=main");
  git(dir, "config", "user.email", "test@example.com");
  git(dir, "config", "user.name", "Test");
  writeFileSync(path.join(dir, "README.md"), `# ${name}\n`);
  git(dir, "add", "-A");
  git(dir, "commit", "-m", "first");
  return dir;
}

describe("conversation summary over the host protocol", () => {
  it("reports the worktree's changes, an extra repo, and a source", async () => {
    const repo = repository("jabot");
    const extra = repository("frontend");
    const { client } = await connected();
    const folder = await client.registerFolder({ path: repo, name: "jabot" });
    const other = await client.registerFolder({
      path: extra,
      name: "jabot-frontend",
    });
    const thread = await client.openThread({
      threadId: "t-sum",
      title: "Auth",
      cwd: repo,
      harnessId: "claude",
      folderId: folder.folderId,
    });
    writeFileSync(path.join(thread.worktreePath ?? thread.cwd, "added.rs"), "fn main() {}\n");

    let summary = await client.threadSummary({ threadId: "t-sum" });
    expect(summary.repositories[0]?.name).toBe("jabot");
    expect(summary.repositories[0]?.environment).toBe("Local");
    expect(summary.repositories[0]?.status).toBe("ok");
    expect(summary.repositories[0]?.additions).toBeGreaterThanOrEqual(1);

    summary = await client.attachThreadRepo({
      threadId: "t-sum",
      folderId: other.folderId,
    });
    expect(summary.repositories.map((r) => r.name)).toEqual([
      "jabot",
      "jabot-frontend",
    ]);

    const note = path.join(mkdtempSync(path.join(tmpdir(), "jabot-src-")), "notes.md");
    writeFileSync(note, "remember this\n");
    summary = await client.addThreadSource({ threadId: "t-sum", path: note });
    expect(summary.sources[0]?.name).toBe("notes.md");
    expect(summary.sources[0]?.available).toBe(true);

    const diff = await client.threadGitDiff({ threadId: "t-sum" });
    expect(diff.files.some((file) => file.path === "added.rs")).toBe(true);

    summary = await client.threadGitCommit({
      threadId: "t-sum",
      message: "add the entry point",
    });
    expect(summary.repositories[0]?.additions).toBe(0);
  });
});
