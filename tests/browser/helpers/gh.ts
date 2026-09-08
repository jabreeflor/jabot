/**
 * A `gh` the host can spawn that never talks to GitHub.
 *
 * Same contract as `tests/e2e/pr.test.ts`: GraphQL and auth answers come from
 * files, tokens stay on stdin, and nothing here claims API compatibility.
 */
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

export interface FakeGh {
  dir: string;
  modePath: string;
  graphqlPath: string;
  viewerPath: string;
  argvPath: string;
  tokenPath: string;
}

export function createFakeGh(): FakeGh {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-browser-gh-"));
  const modePath = path.join(dir, "mode");
  const graphqlPath = path.join(dir, "graphql.json");
  const viewerPath = path.join(dir, "viewer.json");
  const argvPath = path.join(dir, "argv.log");
  const tokenPath = path.join(dir, "token");
  writeFileSync(modePath, "ok\n");
  writeFileSync(graphqlPath, "{}\n");
  writeFileSync(viewerPath, viewerBody());
  const script = `#!/bin/sh
echo "$@" >> "${argvPath}"
mode=$(cat "${modePath}" 2>/dev/null || echo ok)
if [ "$mode" = "fail" ]; then
  echo "refresh failed: fixture GitHub unavailable" >&2
  exit 1
fi
if [ "$1" = "api" ] && [ "$2" = "graphql" ]; then
  for arg in "$@"; do
    case "$arg" in
      *viewer*)
        if [ ! -f "${tokenPath}" ]; then
          echo '{"message":"Bad credentials","documentation_url":"https://docs.github.com/graphql"}' >&2
          exit 1
        fi
        cat "${viewerPath}"; exit 0;;
    esac
  done
  cat "${graphqlPath}"
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
if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  cat "${dir}/pr-view.json"
  exit 0
fi
if [ "$1" = "api" ]; then
  # Workspace reads use REST (\`gh api --method GET\`), not GraphQL.
  case "$*" in
    *pulls/23/files*|*pulls/23/reviews*|*pulls/23/comments*|*pulls/23/commits*|*issues/23/comments*)
      echo '[[]]'; exit 0;;
    *check-runs*)
      echo '{"check_runs":[],"total_count":0}'; exit 0;;
    *"/status"*|*status)
      echo '{"state":"success","statuses":[],"total_count":0}'; exit 0;;
    *pulls/23*)
      cat "${dir}/pr-rest.json"; exit 0;;
  esac
fi
echo "unknown command" >&2
exit 1
`;
  writeFileSync(path.join(dir, "gh"), script);
  chmodSync(path.join(dir, "gh"), 0o755);
  return { dir, modePath, graphqlPath, viewerPath, argvPath, tokenPath };
}

export function setGhMode(gh: FakeGh, mode: "ok" | "fail"): void {
  writeFileSync(gh.modePath, `${mode}\n`);
}

/** The aliased GraphQL shape `pr/refresh` reads (`pr0.pullRequest`). */
export function writeBoardPr(gh: FakeGh): void {
  writeFileSync(
    path.join(gh.dir, "pr-rest.json"),
    JSON.stringify({
      number: 23,
      title: "Migrate auth to sessions",
      state: "open",
      draft: false,
      merged: false,
      html_url: "https://github.com/jabreeflor/jabot/pull/23",
      body: "Session-opened fixture PR.",
      user: { login: "octocat" },
      head: {
        ref: "jabot/t-auth",
        sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      },
      base: { ref: "main" },
      additions: 214,
      deletions: 96,
      changed_files: 3,
      mergeable: true,
      mergeable_state: "clean",
      labels: [],
      requested_reviewers: [],
    }),
  );
  writeFileSync(
    path.join(gh.dir, "pr-view.json"),
    JSON.stringify({
      number: 23,
      url: "https://github.com/jabreeflor/jabot/pull/23",
      title: "Migrate auth to sessions",
      state: "OPEN",
      isDraft: false,
      headRefName: "jabot/t-auth",
    }),
  );
  writeFileSync(
    gh.graphqlPath,
    JSON.stringify({
      data: {
        pr0: {
          pullRequest: {
            number: 23,
            title: "Migrate auth to sessions",
            url: "https://github.com/jabreeflor/jabot/pull/23",
            isDraft: false,
            state: "OPEN",
            additions: 214,
            deletions: 96,
            changedFiles: 3,
            headRefName: "jabot/t-auth",
            baseRefName: "main",
            reviewDecision: null,
            updatedAt: "2026-08-21T09:14:02Z",
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
                            name: "tests",
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
          },
        },
      },
    }),
  );
}

function viewerBody(): string {
  return JSON.stringify({
    data: {
      viewer: {
        login: "octocat",
        pullRequests: {
          nodes: [
            {
              number: 99,
              title: "Remote-only viewer PR",
              url: "https://github.com/octocat/hello-world/pull/99",
              isDraft: false,
              state: "OPEN",
              additions: 4,
              deletions: 1,
              changedFiles: 1,
              headRefName: "work",
              baseRefName: "main",
              reviewDecision: "REVIEW_REQUIRED",
              updatedAt: "2026-08-21T10:00:00Z",
              repository: { nameWithOwner: "octocat/hello-world" },
              commits: {
                nodes: [
                  {
                    commit: {
                      statusCheckRollup: {
                        state: "SUCCESS",
                        contexts: { nodes: [] },
                      },
                    },
                  },
                ],
              },
            },
          ],
        },
      },
    },
  });
}
