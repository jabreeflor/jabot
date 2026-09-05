import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { PrWorkspaceView, diffLines } from "../views/PrWorkspaceView";
import {
  workspacePr,
  workspaceFixture,
} from "../../tests/support/pr-workspace-fixture";
import type { HostClient } from "../host";
function mount(fixture = workspaceFixture) {
  const client = {
    pullRequestDetail: vi.fn().mockResolvedValue(fixture),
    pullRequestAction: vi.fn().mockResolvedValue({}),
  };
  render(
    <PrWorkspaceView
      pr={workspacePr}
      client={client as unknown as HostClient}
      onBack={vi.fn()}
      onOpenThread={vi.fn()}
    />,
  );
  return client;
}
describe("PR workspace", () => {
  it("posts a review against the displayed head and clears only on success", async () => {
    const client = mount();
    const user = userEvent.setup();
    await user.type(
      await screen.findByLabelText(/Comment or review/),
      "Please cover the disabled case.",
    );
    await user.selectOptions(
      screen.getByLabelText("Review decision"),
      "REQUEST_CHANGES",
    );
    await user.click(screen.getByRole("button", { name: "Submit review" }));
    expect(client.pullRequestAction).toHaveBeenCalledWith(
      expect.objectContaining({
        action: "REQUEST_CHANGES",
        body: "Please cover the disabled case.",
        sha: "abc123456789",
        repo: "acme/workspace",
      }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText(/Comment or review/)).toHaveValue(""),
    );
  });
  it("preserves feedback and explains failed writes", async () => {
    const client = mount();
    client.pullRequestAction.mockRejectedValue(new Error("Permission denied"));
    const user = userEvent.setup();
    await user.type(
      await screen.findByLabelText(/Comment or review/),
      "Keep my draft",
    );
    await user.click(screen.getByRole("button", { name: "Comment" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Permission denied",
    );
    expect(screen.getByLabelText(/Comment or review/)).toHaveValue(
      "Keep my draft",
    );
  });
  it("requires confirmation before merge and sends the selected strategy and head", async () => {
    const client = mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "Merge pull request…" }),
    );
    expect(client.pullRequestAction).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Confirm merge" }));
    expect(client.pullRequestAction).toHaveBeenCalledWith(
      expect.objectContaining({
        action: "merge",
        method: "squash",
        sha: "abc123456789",
      }),
    );
  });
  it("blocks merge for a draft or unknown mergeability", async () => {
    mount({
      ...workspaceFixture,
      pr: { ...workspaceFixture.pr, draft: true, mergeable: null },
    });
    expect(
      await screen.findByRole("button", { name: "Merge pull request…" }),
    ).toBeDisabled();
  });
  it("posts a removed-line comment on the LEFT side", async () => {
    const client = mount();
    const user = userEvent.setup();
    await screen.findByLabelText(/Comment or review/);
    await user.click(screen.getByRole("tab", { name: /Files changed/ }));
    const lines = screen.getAllByRole("button", {
      name: "Comment on src/notifications.ts line 12",
    });
    await user.click(lines[0]);
    await user.type(
      screen.getByLabelText("Inline comment"),
      "Why remove this?",
    );
    await user.click(screen.getByRole("button", { name: "Post line comment" }));
    expect(client.pullRequestAction).toHaveBeenCalledWith(
      expect.objectContaining({
        action: "inline",
        path: "src/notifications.ts",
        line: 12,
        side: "LEFT",
        body: "Why remove this?",
      }),
    );
  });
  it("tracks both sides across multiple hunks", () => {
    expect(
      diffLines("@@ -5,2 +8,2 @@\n-old\n+new\n same\n@@ -20 +24 @@\n+last").map(
        (l) => [l.left, l.right],
      ),
    ).toEqual([
      [null, null],
      [5, null],
      [null, 8],
      [6, 9],
      [null, null],
      [null, 24],
    ]);
  });
});
