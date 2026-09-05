import type { PrWorkspace } from "../../src/host/prWorkspace";
import type { PullRequest } from "../../src/components/types";
export const workspacePr: PullRequest = {
  id: "review-42",
  repo: "acme/workspace",
  number: 42,
  provider: "github",
  url: "https://github.com/acme/workspace/pull/42",
  title: "Add workspace activity notifications",
  status: "open",
  checkState: "passing",
  updatedAt: "2026-09-04T18:00:00Z",
  additions: 38,
  deletions: 4,
  headRef: "feat/notifications",
  baseRef: "main",
};
export const workspaceFixture: PrWorkspace = {
  pr: {
    title: workspacePr.title,
    body: "Keep the team up to date when a workspace changes.\n\nThis adds activity notifications with a per-workspace preference and preserves the existing email settings.\n\nValidation\n• Notification delivery and opt-out covered by tests\n• Keyboard navigation checked",
    state: "open",
    merged: false,
    draft: false,
    mergeable: true,
    mergeable_state: "clean",
    html_url: workspacePr.url,
    user: { login: "alex" },
    head: { sha: "abc123456789", ref: "feat/notifications" },
    base: { ref: "main" },
    additions: 38,
    deletions: 4,
    changed_files: 2,
    requested_reviewers: [{ login: "sam" }],
    labels: [{ name: "enhancement" }],
  },
  comments: [
    {
      id: 1,
      user: { login: "sam" },
      body: "The preference behavior looks good. Can we also confirm the default for existing workspaces?",
      created_at: "2026-09-04T18:05:00Z",
      html_url: "https://github.com/acme/workspace/pull/42#issuecomment-1",
    },
  ],
  reviews: [],
  inline: [],
  files: [
    {
      filename: "src/notifications.ts",
      status: "modified",
      additions: 4,
      deletions: 1,
      blob_url: "https://github.com/acme/workspace",
      patch:
        "@@ -12,3 +12,6 @@ export function notify(workspace) {\n-  send(workspace.members);\n+  if (!workspace.notificationsEnabled) {\n+    return;\n+  }\n+  send(workspace.members, { channel: 'activity' });\n }",
    },
    {
      filename: "src/preferences.ts",
      status: "modified",
      additions: 1,
      deletions: 0,
      blob_url: "https://github.com/acme/workspace",
      patch:
        "@@ -4,2 +4,3 @@ export const defaults = {\n   emailEnabled: true,\n+  notificationsEnabled: true,\n };",
    },
  ],
  commits: [
    {
      sha: "abc123456789",
      html_url: "https://github.com/acme/workspace",
      commit: {
        message: "Add workspace notification preferences",
        author: { name: "Alex Morgan" },
      },
    },
  ],
  checks: {
    total_count: 2,
    check_runs: [
      {
        id: 1,
        name: "Tests",
        status: "completed",
        conclusion: "success",
        html_url: "https://github.com/acme/workspace",
      },
      {
        id: 2,
        name: "Build",
        status: "completed",
        conclusion: "success",
        html_url: "https://github.com/acme/workspace",
      },
    ],
  },
  statuses: { state: "success", statuses: [], total_count: 0 },
  checksError: null,
};
