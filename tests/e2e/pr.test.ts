/**
 * End-to-end: the Pull Requests board, from an agent's shell output to a card
 * in the Inbox (#28).
 *
 * `src-tauri/tests/pr.rs` proves the linkage half in-process. What this file
 * adds is the half that needs a *credential* — and the point of the exercise is
 * that it needs one only at the very edge. `gh` is put on the host's PATH as a
 * script that answers with a recorded GitHub GraphQL body, so everything
 * between the host and GitHub is the real thing: the query the host builds, the
 * argv it runs, the JSON it parses, the row it writes, and the Inbox card the
 * change earns. Only the network is a fixture, because there is no GitHub
 * credential in this environment and no egress to the API.
 *
 * The production `HostClient` drives it, so the wire shape is asserted too.
 */
import { execFileSync } from "node:child_process";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import { HostdProcess, fakeAcpRuntime } from "../support/hostd";

const running: HostdProcess[] = [];

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

/** A real checkout with a real `origin`, because linkage is refused without one. */
function repository(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-pr-repo-"));
  git(dir, "init", "--initial-branch=main");
  git(dir, "config", "user.email", "test@example.com");
  git(dir, "config", "user.name", "Test");
  git(dir, "remote", "add", "origin", "git@github.com:jabreeflor/jabot.git");
  writeFileSync(path.join(dir, "README.md"), "# project\n");
  git(dir, "add", "-A");
  git(dir, "commit", "-m", "first");
  return dir;
}

/**
 * A `gh` on PATH that answers `gh api graphql` from a file and refuses
 * everything else.
 *
 * Refusing the rest matters: `gh pr view` is the host's post-turn fallback, and
 * a fake that answered it would hide whether the *stdout* path works at all.
 * The body is read at call time so a test can change GitHub's mind between two
 * polls, which is the only way to assert that a card is written on a change and
 * not on a state.
 */
function fakeGh(): { dir: string; bodyPath: string } {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-fake-gh-"));
  const bodyPath = path.join(dir, "graphql.json");
  const script = `#!/bin/sh
if [ "$1" = "api" ] && [ "$2" = "graphql" ]; then
  cat "${bodyPath}"
  exit 0
fi
echo "no pull requests found" >&2
exit 1
`;
  const bin = path.join(dir, "gh");
  writeFileSync(bin, script);
  chmodSync(bin, 0o755);
  return { dir, bodyPath };
}

/** One PR node, in the shape GitHub's GraphQL API answers with. */
function body(over: {
  state?: string;
  rollup?: string | null;
  checks?: Array<{ name: string; conclusion: string | null; status: string }>;
  reviewDecision?: string | null;
}) {
  const checks = over.checks ?? [
    { name: "tests", status: "COMPLETED", conclusion: "FAILURE" },
    { name: "lint", status: "COMPLETED", conclusion: "SUCCESS" },
  ];
  return JSON.stringify({
    data: {
      pr0: {
        pullRequest: {
          number: 23,
          title: "Migrate auth to sessions",
          url: "https://github.com/jabreeflor/jabot/pull/23",
          isDraft: false,
          state: over.state ?? "OPEN",
          additions: 214,
          deletions: 96,
          changedFiles: 3,
          headRefName: "jabot/t-auth",
          baseRefName: "main",
          reviewDecision: over.reviewDecision ?? null,
          updatedAt: "2026-08-21T09:14:02Z",
          commits: {
            nodes: [
              {
                commit: {
                  statusCheckRollup:
                    over.rollup === null
                      ? null
                      : {
                          state: over.rollup ?? "FAILURE",
                          contexts: {
                            nodes: checks.map((check) => ({
                              __typename: "CheckRun",
                              ...check,
                            })),
                          },
                        },
                },
              },
            ],
          },
        },
      },
    },
  });
}

async function board(gh: { dir: string; bodyPath: string }) {
  const host = new HostdProcess({
    persistent: true,
    env: { PATH: `${gh.dir}${path.delimiter}${process.env.PATH ?? ""}` },
  });
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  await client.hello();

  const repo = repository();
  const folder = await client.registerFolder({ path: repo });
  await client.openThread({
    threadId: "t-auth",
    title: "Auth migration",
    cwd: folder.cwd,
    harnessId: "claude",
    folderId: folder.folderId,
    runtime: fakeAcpRuntime("execute"),
  });
  // `execute` mode echoes the prompt as the stdout of a shell tool call —
  // which is precisely what `gh pr create` prints.
  await client.prompt({
    threadId: "t-auth",
    content: "https://github.com/jabreeflor/jabot/pull/23",
  });
  return { host, client };
}

async function settle(
  client: HostClient,
  until: (rows: Awaited<ReturnType<HostClient["listPullRequests"]>>) => boolean,
) {
  const deadline = Date.now() + 15_000;
  for (;;) {
    const listed = await client.listPullRequests();
    if (until(listed)) return listed;
    if (Date.now() > deadline) {
      throw new Error(`board never settled: ${JSON.stringify(listed)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

describe("the pull request board over the host protocol", () => {
  it("links what the session opened, then fills it in from GitHub", async () => {
    const gh = fakeGh();
    writeFileSync(gh.bodyPath, body({}));
    const { client } = await board(gh);

    const linked = await settle(client, (rows) => rows.pullRequests.length > 0);
    const pr = linked.pullRequests[0];
    expect(pr.threadId).toBe("t-auth");
    expect(pr.repo).toBe("jabreeflor/jabot");
    expect(pr.number).toBe(23);
    expect(pr.detectedVia).toBe("stdout");
    // Linked but never polled: no credential was needed to get this far, and
    // the row says honestly that GitHub has not been asked.
    expect(pr.polledAt).toBeUndefined();
    expect(pr.title).toBe("");

    const refreshed = await client.refreshPullRequests();
    expect(refreshed.unavailable).toEqual([]);
    expect(refreshed.checked).toBe(1);
    expect(refreshed.updated).toBe(1);

    const filled = refreshed.pullRequests[0];
    expect(filled.title).toBe("Migrate auth to sessions");
    expect(filled.status).toBe("open");
    expect(filled.additions).toBe(214);
    expect(filled.deletions).toBe(96);
    expect(filled.changedFiles).toBe(3);
    expect(filled.headRef).toBe("jabot/t-auth");
    expect(filled.baseRef).toBe("main");
    expect(filled.checkState).toBe("failing");
    expect(filled.checks).toEqual([
      { label: "tests", state: "failing" },
      { label: "lint", state: "passing" },
    ]);
    // GitHub's clock, not ours.
    expect(filled.updatedAt).toBe("2026-08-21T09:14:02Z");
    expect(filled.polledAt).toBeTruthy();
  });

  /**
   * The property the Inbox depends on: a poll runs every fifteen seconds, and a
   * PR that has been red since lunch is not news each time.
   */
  it("cards a change and not a state", async () => {
    const gh = fakeGh();
    writeFileSync(gh.bodyPath, body({}));
    const { client } = await board(gh);
    await settle(client, (rows) => rows.pullRequests.length > 0);

    const first = await client.refreshPullRequests();
    expect(first.cards).toBe(1);

    const again = await client.refreshPullRequests();
    expect(again.cards).toBe(0);
    expect(again.updated).toBe(0);

    // A reviewer asks for changes: that is new, and it is a job.
    writeFileSync(gh.bodyPath, body({ reviewDecision: "CHANGES_REQUESTED" }));
    const reviewed = await client.refreshPullRequests();
    expect(reviewed.cards).toBe(1);
    expect(reviewed.pullRequests[0].reviewState).toBe("changes_requested");

    // Checks going green again is not an interruption — the absence of the red
    // card is that news.
    writeFileSync(
      gh.bodyPath,
      body({
        rollup: "SUCCESS",
        reviewDecision: "CHANGES_REQUESTED",
        checks: [{ name: "tests", status: "COMPLETED", conclusion: "SUCCESS" }],
      }),
    );
    const green = await client.refreshPullRequests();
    expect(green.cards).toBe(0);
    expect(green.pullRequests[0].checkState).toBe("passing");

    const inbox = await client.inbox();
    const cards = inbox.events.filter((event) => event.kind === "pr");
    expect(cards.map((card) => card.title)).toEqual([
      "PR #23 · changes requested",
      "PR #23 · checks failed",
      "PR #23 opened",
    ]);
    // The thread never folded, so nothing resurfaced — and the cards still
    // have to be counted, or nobody is ever told they exist.
    expect(inbox.unread).toBeGreaterThanOrEqual(3);
  });

  it("reports the PR on the thread that opened it", async () => {
    const gh = fakeGh();
    writeFileSync(gh.bodyPath, body({}));
    const { client } = await board(gh);
    await settle(client, (rows) => rows.pullRequests.length > 0);

    const state = await client.threadState({ threadId: "t-auth" });
    expect(state.pullRequests?.[0]?.number).toBe(23);
    expect(state.pullRequests?.[0]?.threadId).toBe("t-auth");
  });

  /**
   * The state every user without a `gh` login is in. A poll is a background
   * loop; one that throws takes the board down with it.
   */
  it("survives a gh that cannot answer", async () => {
    const gh = fakeGh();
    // No fixture on disk: `cat` fails, `gh` exits non-zero with a body that is
    // not JSON — the shape a logged-out or rate-limited `gh` produces.
    const { client } = await board(gh);
    await settle(client, (rows) => rows.pullRequests.length > 0);

    const refreshed = await client.refreshPullRequests();
    expect(refreshed.pullRequests).toHaveLength(1);
    expect(refreshed.cards).toBe(0);
    expect(refreshed.unavailable).toHaveLength(1);
    expect(refreshed.unavailable[0].host).toBe("github.com");
    expect(refreshed.unavailable[0].detail).not.toBe("");
  });
});
