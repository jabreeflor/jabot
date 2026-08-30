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
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
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

/**
 * A `gh` that can answer the *fallback* — rungs 2 and 3 of the linkage ladder.
 *
 * [`fakeGh`] refuses `pr view` on purpose, so those two rungs have never been
 * run as subprocesses by anything: `github::pr_for_cwd` and
 * `github::pr_for_branch` were only ever parsed against JSON fixtures. That
 * gap is not bookkeeping. Both return `Ok(None)` on a non-zero exit, so a
 * renamed `--json` field or a dropped flag is indistinguishable from "no PR
 * for this branch" and would ship silently, leaving every browser-opened and
 * MCP-opened pull request unlinked forever.
 *
 * So this one answers from fixtures *read at call time* — which lets a test
 * delete the `pr view` fixture to force the drop to rung 3, exactly the way a
 * `gh` that cannot resolve a detached HEAD behaves — and logs every argv, so
 * the flag strings GitHub's CLI expects are asserted rather than assumed.
 */
function fakeGhLinkage(): {
  dir: string;
  viewPath: string;
  listPath: string;
  bodyPath: string;
  argvPath: string;
} {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-fake-gh-linkage-"));
  const viewPath = path.join(dir, "pr-view.json");
  const listPath = path.join(dir, "pr-list.json");
  const bodyPath = path.join(dir, "graphql.json");
  const argvPath = path.join(dir, "argv.log");
  // `cat` on a missing fixture exits non-zero with nothing on stdout, which is
  // what `gh` does for a branch nobody opened a PR from — the ordinary answer,
  // not a failure.
  const script = `#!/bin/sh
echo "$@" >> "${argvPath}"
if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  cat "${viewPath}" || exit 1
  exit 0
fi
if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  cat "${listPath}" || exit 1
  exit 0
fi
if [ "$1" = "api" ] && [ "$2" = "graphql" ]; then
  cat "${bodyPath}"
  exit 0
fi
echo "unknown command" >&2
exit 1
`;
  const bin = path.join(dir, "gh");
  writeFileSync(bin, script);
  chmodSync(bin, 0o755);
  return { dir, viewPath, listPath, bodyPath, argvPath };
}

/** The `--json` body `gh pr view` answers with, for the fields the host asks
    for. */
function viewedPr(over: { number: number; url: string; headRefName?: string }) {
  return JSON.stringify({
    number: over.number,
    url: over.url,
    title: "Migrate auth to sessions",
    state: "OPEN",
    isDraft: false,
    headRefName: over.headRefName ?? "jabot/t-auth",
  });
}

/** `gh pr list --json` answers with an array, and the host takes the first. */
function listedPrs(
  rows: Array<{ number: number; url: string; headRefName?: string }>,
) {
  return JSON.stringify(
    rows.map((row) => ({
      number: row.number,
      url: row.url,
      headRefName: row.headRefName ?? "jabot/t-auth",
    })),
  );
}

/** The argv log as lines, so a test can find the one call it is about. */
function argvLines(argvPath: string): string[] {
  if (!existsSync(argvPath)) return [];
  return readFileSync(argvPath, "utf8")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
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

/**
 * A host with a linked PR on its board.
 *
 * The background poll is off by default here (`JABOT_PR_POLL_MS=0`). Every
 * case below except the two that are *about* the poll is asserting what an
 * explicit `pr/refresh` does — how many cards it writes, what it does on a
 * second call with nothing changed — and a tick racing those would make them
 * about scheduling instead. The poll has its own case; it opts back in.
 */
async function board(
  gh: { dir: string },
  env: Record<string, string> = {},
  turn: { prompt?: string; mode?: string } = {},
) {
  const host = new HostdProcess({
    persistent: true,
    env: {
      PATH: `${gh.dir}${path.delimiter}${process.env.PATH ?? ""}`,
      JABOT_PR_POLL_MS: "0",
      ...env,
    },
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
    runtime: fakeAcpRuntime(turn.mode ?? "execute"),
  });
  // `execute` mode echoes the prompt as the stdout of a shell tool call —
  // which is precisely what `gh pr create` prints. `say` echoes it as a chat
  // bubble instead, for the turn that only *talks* about a pull request and so
  // has to be resolved by asking `gh`.
  await client.prompt({
    threadId: "t-auth",
    content: turn.prompt ?? "https://github.com/jabreeflor/jabot/pull/23",
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

  /**
   * The poll is the host's, not a renderer's (#28).
   *
   * `usePullRequests` owned the whole thing: a `setInterval` armed only while
   * a webview was alive and running. Since `card::transition` writes a `pr`
   * card only when a *refresh* observes a change, that meant no "checks
   * failed" card while the app sat in the Dock with its timers throttled —
   * and none at all here, under `jabot-hostd`, which has no renderer to arm
   * anything. A paired phone got zero PR polling and zero PR cards.
   *
   * So this case never calls `refreshPullRequests`. Everything it asserts has
   * to arrive from the pump, and it fails on the old code however long it
   * waits.
   */
  it("polls GitHub and cards a change with nobody asking", async () => {
    const gh = fakeGh();
    // Green to start with, so the red below is a change rather than the state
    // the first poll happens to find.
    writeFileSync(
      gh.bodyPath,
      body({
        rollup: "SUCCESS",
        checks: [{ name: "tests", status: "COMPLETED", conclusion: "SUCCESS" }],
      }),
    );
    const { client } = await board(gh, {
      JABOT_PR_POLL_MS: "200",
      JABOT_PR_POLL_IDLE_MS: "200",
    });

    // The first thing the tick does for a freshly linked row: fill it in.
    // Nothing in this test ever calls `pr/refresh`.
    await settle(
      client,
      (rows) => rows.pullRequests[0]?.checkState === "passing",
    );

    // GitHub changes its mind while nobody is looking at the board.
    writeFileSync(gh.bodyPath, body({}));

    const red = await settle(
      client,
      (rows) => rows.pullRequests[0]?.checkState === "failing",
    );
    expect(red.pullRequests[0].number).toBe(23);

    // And the card — the whole reason the poll has to be the host's. This is
    // the row that never got written while the app was in the Dock.
    const deadline = Date.now() + 15_000;
    for (;;) {
      const inbox = await client.inbox();
      const titles = inbox.events
        .filter((event) => event.kind === "pr")
        .map((event) => event.title);
      if (titles.includes("PR #23 · checks failed")) {
        // The opening card came from the same unasked-for poll.
        expect(titles).toContain("PR #23 opened");
        break;
      }
      if (Date.now() > deadline) {
        throw new Error(`no card was written: ${JSON.stringify(titles)}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  });

  /**
   * The board that must never spawn `gh`.
   *
   * A poll on the pump is a subprocess every fifteen seconds forever, and the
   * overwhelming majority of machines have no linked pull request at all. The
   * tick reads the store first and stops there.
   */
  it("does not shell out for a board with nothing linked", async () => {
    const gh = fakeGh();
    writeFileSync(gh.bodyPath, body({}));
    const host = new HostdProcess({
      persistent: true,
      env: {
        PATH: `${gh.dir}${path.delimiter}${process.env.PATH ?? ""}`,
        JABOT_PR_POLL_MS: "50",
        JABOT_PR_POLL_IDLE_MS: "50",
      },
    });
    running.push(host);
    const client = new HostClient(host);
    await client.connect();
    await client.hello();

    // Long enough for many ticks at 50ms. Nothing is linked, so nothing is
    // asked, and the board stays the empty answer `pr/list` gives without a
    // credential.
    await new Promise((resolve) => setTimeout(resolve, 600));

    const listed = await client.listPullRequests();
    expect(listed.pullRequests).toEqual([]);
    const inbox = await client.inbox();
    expect(inbox.events.filter((event) => event.kind === "pr")).toEqual([]);
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
 * Rungs 2 and 3 of the linkage ladder, as subprocesses (#28).
 *
 * `pr-linkage.md` puts three rungs under linkage: a `/pull/<n>` URL in the
 * stdout of an execute call, `gh pr view` in the session's own tree at turn
 * end, and matching the head branch against the repository's open PRs. Only
 * the first has ever been run. The other two were tested at the *parse* level,
 * against JSON fixtures, and never once spawned.
 *
 * That is worth closing because of how both of them fail. `pr_for_cwd` and
 * `pr_for_branch` read a non-zero exit as "no PR for this branch" — which is
 * the right reading, and which also means a dropped flag or a renamed `--json`
 * field is silent. The board would simply never link a pull request opened in
 * a browser or by an MCP server, and no test would go red.
 *
 * So these cases assert three things a fixture cannot: that a turn which only
 * *talks* about a pull request gets linked, that the argv is the string
 * GitHub's CLI actually expects, and that `belongs_to` still refuses a
 * stranger's repository when the URL arrives from `gh` rather than from stdout.
 */
describe("resolving a pull request the session never printed", () => {
  /** The agent says it, and says nothing else. No URL is ever printed, so
      nothing here can be linked without asking `gh`. */
  const PROSE = "Done — I opened a pull request for this branch.";

  async function until(check: () => boolean | Promise<boolean>, what: string) {
    const deadline = Date.now() + 15_000;
    for (;;) {
      if (await check()) return;
      if (Date.now() > deadline) throw new Error(`never happened: ${what}`);
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }

  function talking(gh: { dir: string }) {
    return board(gh, {}, { mode: "say", prompt: PROSE });
  }

  it("asks gh pr view, and links what it answers", async () => {
    const gh = fakeGhLinkage();
    writeFileSync(
      gh.viewPath,
      viewedPr({
        number: 23,
        url: "https://github.com/jabreeflor/jabot/pull/23",
      }),
    );
    const { client } = await talking(gh);

    const linked = await settle(client, (rows) => rows.pullRequests.length > 0);
    const pr = linked.pullRequests[0];
    expect(pr.number).toBe(23);
    expect(pr.repo).toBe("jabreeflor/jabot");
    expect(pr.threadId).toBe("t-auth");
    // Not "stdout": nothing was printed. This is the constant that had zero
    // occurrences in any test.
    expect(pr.detectedVia).toBe("gh-pr-view");

    // The argv, exactly. This is the assertion that catches the silent
    // failure — a renamed field here reads as "no PR" and nothing else moves.
    expect(argvLines(gh.argvPath)).toContain(
      "pr view --json number,url,title,state,isDraft,headRefName",
    );
    // And rung 3 was not reached, because rung 2 answered.
    expect(argvLines(gh.argvPath).some((line) => line.startsWith("pr list"))).toBe(
      false,
    );
  });

  /**
   * The drop to rung 3. `gh pr view` exits non-zero for a fork whose head it
   * spells `owner:branch` and for a detached HEAD — both of which #23's
   * worktrees can produce — and the branch is matched against the
   * repository's open pull requests instead.
   */
  it("falls back to the branch list when gh pr view has no answer", async () => {
    const gh = fakeGhLinkage();
    // No `pr view` fixture on disk, so `cat` fails and `gh` exits 1.
    writeFileSync(
      gh.listPath,
      listedPrs([
        { number: 31, url: "https://github.com/jabreeflor/jabot/pull/31" },
      ]),
    );
    const { client } = await talking(gh);

    const linked = await settle(client, (rows) => rows.pullRequests.length > 0);
    const pr = linked.pullRequests[0];
    expect(pr.number).toBe(31);
    expect(pr.repo).toBe("jabreeflor/jabot");
    expect(pr.detectedVia).toBe("head-list");

    // `--repo` carries the thread's own slug rather than being inferred from a
    // directory, which is the whole reason this rung works from a worktree
    // whose `origin` points somewhere unexpected. The branch is whatever the
    // host checked out, so the assertion is on the flags and their order.
    const listed = argvLines(gh.argvPath).find((line) =>
      line.startsWith("pr list"),
    );
    expect(listed).toMatch(
      /^pr list --repo github\.com\/jabreeflor\/jabot --head \S+ --state open --limit 1 --json number,url,headRefName$/,
    );
  });

  /**
   * The guard, on the path that had never been through a subprocess. An agent
   * that ran `gh pr view --repo somebody/else` must not attach a stranger's
   * pull request to this conversation — the link is written once and never
   * re-derived, so a wrong one is wrong forever. A *fork* is the case that
   * looks the same and is not: `gh pr create` from `me/jabot` prints the
   * upstream URL, and the repository name is all the two spellings share.
   */
  it("refuses a stranger's repository but accepts a fork's upstream", async () => {
    const stranger = fakeGhLinkage();
    writeFileSync(
      stranger.viewPath,
      viewedPr({ number: 9, url: "https://github.com/somebody/else/pull/9" }),
    );
    const refused = await talking(stranger);

    // An empty board has to be a refusal rather than a race, so wait for the
    // host to have run out of rungs: either it dropped to `pr list` (which
    // has no fixture, so it answers nothing) or it wrote a row — and a row is
    // the failure this case exists to catch.
    await until(
      async () =>
        argvLines(stranger.argvPath).some((line) => line.startsWith("pr list")) ||
        (await refused.client.listPullRequests()).pullRequests.length > 0,
      "gh ran out of rungs",
    );
    expect((await refused.client.listPullRequests()).pullRequests).toEqual([]);

    const fork = fakeGhLinkage();
    writeFileSync(
      fork.viewPath,
      viewedPr({
        number: 23,
        url: "https://github.com/upstream-org/jabot/pull/23",
      }),
    );
    const accepted = await talking(fork);

    const linked = await settle(
      accepted.client,
      (rows) => rows.pullRequests.length > 0,
    );
    expect(linked.pullRequests[0].repo).toBe("upstream-org/jabot");
    expect(linked.pullRequests[0].number).toBe(23);
    expect(linked.pullRequests[0].detectedVia).toBe("gh-pr-view");
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
