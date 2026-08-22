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
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
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


/**
 * A `gh` that can also be *logged into*, for the sign-in half (#28).
 *
 * Richer than [`fakeGh`] on purpose: this one holds state. `auth login
 * --with-token` reads stdin and stores the token, `auth token` and `auth
 * status` answer from what was stored, and `api graphql` refuses with a 401
 * until there is one — which is what makes "signed out" a state the test can
 * actually be in rather than a claim.
 *
 * It also logs every argv it is ever called with, so the test can assert the
 * one property that matters more than any of the behaviour: a token reaches
 * `gh` on stdin and never, on any call, on a command line that `ps` would
 * show.
 */
function fakeGhWithAuth(): {
  dir: string;
  linkedPath: string;
  viewerPath: string;
  argvPath: string;
  tokenPath: string;
} {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-fake-gh-auth-"));
  const linkedPath = path.join(dir, "graphql.json");
  const viewerPath = path.join(dir, "viewer.json");
  const argvPath = path.join(dir, "argv.log");
  const tokenPath = path.join(dir, "token");
  const script = `#!/bin/sh
echo "$@" >> "${argvPath}"
if [ "$1" = "api" ] && [ "$2" = "graphql" ]; then
  if [ ! -f "${tokenPath}" ]; then
    echo '{"message":"Bad credentials","documentation_url":"https://docs.github.com/graphql"}' >&2
    exit 1
  fi
  for arg in "$@"; do
    case "$arg" in
      *viewer*) cat "${viewerPath}"; exit 0;;
    esac
  done
  cat "${linkedPath}"
  exit 0
fi
if [ "$1" = "auth" ] && [ "$2" = "login" ]; then
  read -r token
  case "$token" in
    ghp_good*) printf '%s' "$token" > "${tokenPath}"; exit 0;;
    *) echo "error validating token: HTTP 401: Bad credentials" >&2; exit 1;;
  esac
fi
if [ "$1" = "auth" ] && [ "$2" = "token" ]; then
  if [ -f "${tokenPath}" ]; then cat "${tokenPath}"; echo; exit 0; fi
  echo "not logged in to any hosts" >&2
  exit 1
fi
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  if [ -f "${tokenPath}" ]; then
    echo "github.com"
    echo "  ✓ Logged in to github.com account octocat (keyring)"
    exit 0
  fi
  echo "You are not logged into any GitHub hosts." >&2
  exit 1
fi
echo "no pull requests found" >&2
exit 1
`;
  const bin = path.join(dir, "gh");
  writeFileSync(bin, script);
  chmodSync(bin, 0o755);
  return { dir, linkedPath, viewerPath, argvPath, tokenPath };
}

/** What `viewer.pullRequests` answers with: one PR a session here opened, and
    one written somewhere else entirely — which is the whole point of asking. */
function viewerBody() {
  const node = (over: {
    number: number;
    title: string;
    repo: string;
    isDraft?: boolean;
    updatedAt: string;
  }) => ({
    number: over.number,
    title: over.title,
    url: `https://github.com/${over.repo}/pull/${over.number}`,
    isDraft: over.isDraft ?? false,
    state: "OPEN",
    additions: 12,
    deletions: 3,
    changedFiles: 2,
    headRefName: "work",
    baseRefName: "main",
    reviewDecision: "REVIEW_REQUIRED",
    updatedAt: over.updatedAt,
    repository: { nameWithOwner: over.repo },
    commits: {
      nodes: [
        {
          commit: {
            statusCheckRollup: {
              state: "SUCCESS",
              contexts: {
                nodes: [
                  {
                    __typename: "CheckRun",
                    name: "verify",
                    status: "COMPLETED",
                    conclusion: "SUCCESS",
                  },
                ],
              },
            },
          },
        },
      ],
    },
  });
  return JSON.stringify({
    data: {
      viewer: {
        login: "octocat",
        pullRequests: {
          nodes: [
            node({
              number: 23,
              title: "Migrate auth to sessions",
              repo: "jabreeflor/jabot",
              updatedAt: "2026-08-21T09:14:02Z",
            }),
            node({
              number: 7,
              title: "Bump the pinned toolchain",
              repo: "someone-else/infra",
              updatedAt: "2026-08-20T17:45:00Z",
            }),
          ],
        },
      },
    },
  });
}

async function board(gh: { dir: string }) {
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

/**
 * The half that needs a person: signing in, and the board that only exists
 * once somebody has (#28).
 *
 * The linked board is what a session opened here. This is what the *user* has
 * open, anywhere — which is the thing a PR tab is expected to show, and the
 * reason it offers a sign-in at all.
 */
describe("signing in to GitHub and seeing your own pull requests", () => {
  it("reports being logged out, takes a token, and then answers as somebody", async () => {
    const gh = fakeGhWithAuth();
    writeFileSync(gh.linkedPath, body({}));
    writeFileSync(gh.viewerPath, viewerBody());
    const { client } = await board(gh);
    await settle(client, (rows) => rows.pullRequests.length > 0);

    const before = await client.githubStatus();
    expect(before.installed).toBe(true);
    expect(before.authenticated).toBe(false);
    expect(before.remedy).toContain("gh auth login");

    // Logged out, the user's own board is not an empty one — it says why.
    const refused = await client.myPullRequests();
    expect(refused.pullRequests).toEqual([]);
    expect(refused.unavailable?.reason).toBe("gh_failed");
    expect(refused.unavailable?.detail).toContain("Bad credentials");

    // A token GitHub rejects is an error frame: somebody is waiting at the
    // dialog to be told whether their paste worked.
    await expect(
      client.githubLogin({ token: "ghp_badtokenvalue" }),
    ).rejects.toThrow(/Bad credentials/);

    const after = await client.githubLogin({ token: "ghp_goodtokenvalue" });
    expect(after.authenticated).toBe(true);
    expect(after.account).toBe("octocat");
    expect(after.remedy).toBeUndefined();

    // The property that matters most: the token went in on stdin. Nothing on
    // any command line, on any call, ever carried it.
    const argv = readFileSync(gh.argvPath, "utf8");
    expect(argv).toContain("auth login --hostname github.com --with-token");
    expect(argv).not.toContain("ghp_goodtokenvalue");
    expect(argv).not.toContain("ghp_badtokenvalue");
  });

  it("shows every open PR you wrote, with a thread only where there is one", async () => {
    const gh = fakeGhWithAuth();
    writeFileSync(gh.linkedPath, body({}));
    writeFileSync(gh.viewerPath, viewerBody());
    const { client } = await board(gh);
    await settle(client, (rows) => rows.pullRequests.length > 0);
    await client.githubLogin({ token: "ghp_goodtokenvalue" });

    const mine = await client.myPullRequests();
    expect(mine.unavailable).toBeUndefined();
    expect(mine.account).toBe("octocat");
    expect(mine.pullRequests).toHaveLength(2);

    // The one a session here opened comes back carrying that session, so the
    // row keeps its "Reopen thread".
    const linked = mine.pullRequests.find((pr) => pr.number === 23);
    expect(linked?.repo).toBe("jabreeflor/jabot");
    expect(linked?.threadId).toBe("t-auth");
    expect(linked?.threadTitle).toBe("Auth migration");
    expect(linked?.linkedId).toBeTruthy();
    expect(linked?.checkState).toBe("passing");
    expect(linked?.id).toBe("github:jabreeflor/jabot#23");

    // And the one written somewhere else is on the board with no thread at
    // all, which is exactly what it should be: a PR to look at, not a session
    // to reopen.
    const elsewhere = mine.pullRequests.find((pr) => pr.number === 7);
    expect(elsewhere?.repo).toBe("someone-else/infra");
    expect(elsewhere?.threadId).toBeUndefined();
    expect(elsewhere?.linkedId).toBeUndefined();
    expect(elsewhere?.forgeHost).toBe("github.com");
    expect(elsewhere?.url).toBe("https://github.com/someone-else/infra/pull/7");
  });
});
