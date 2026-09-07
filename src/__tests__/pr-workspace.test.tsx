import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { PR_DETAIL_REFRESH_MS } from "../views/prDetails";
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
  it("keeps refresh outside the scrolling detail and reloads the PR", async () => {
    const client = mount();
    const refresh = await screen.findByRole("button", { name: "Refresh" });

    expect(refresh.closest(".pr-view-chrome")).toBeInTheDocument();
    expect(refresh.closest(".page-scroll")).not.toBeInTheDocument();
    await waitFor(() => expect(client.pullRequestDetail).toHaveBeenCalledOnce());

    await userEvent.click(refresh);
    await waitFor(() => expect(client.pullRequestDetail).toHaveBeenCalledTimes(2));
  });

  it("renders GitHub-flavoured markdown in the PR description", async () => {
    const body = [
      "## Validation",
      "",
      "- [x] **Unit tests**",
      "- [ ] Manual check",
      "",
      "Use `npm test` or read the [guide](https://example.com/guide).",
      "",
      "```ts",
      "const ready = true;",
      "```",
      "",
      "| Gate | Result |",
      "| --- | --- |",
      "| Typecheck | Pass |",
      "",
      "![Rendered preview](https://example.com/preview.png)",
      "",
      "[Contributing](CONTRIBUTING.md)",
      "",
      "![Architecture](docs/img/architecture.png)",
    ].join("\n");
    const { container } = render(
      <PrWorkspaceView
        pr={workspacePr}
        client={
          {
            pullRequestDetail: vi.fn().mockResolvedValue({
              ...workspaceFixture,
              pr: { ...workspaceFixture.pr, body },
            }),
            pullRequestAction: vi.fn(),
          } as unknown as HostClient
        }
        onBack={vi.fn()}
        onOpenThread={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("heading", { name: "Validation", level: 2 }),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("checkbox")).toHaveLength(2);
    expect(
      screen.getByText("npm test", { selector: "code" }),
    ).toBeInTheDocument();
    expect(container.querySelector("pre code")?.textContent).toBe(
      "const ready = true;\n",
    );
    expect(screen.getByRole("link", { name: "guide" })).toHaveAttribute(
      "href",
      "https://example.com/guide",
    );
    expect(screen.getByRole("link", { name: "guide" })).toHaveAttribute(
      "target",
      "_blank",
    );
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "Rendered preview" }),
    ).toHaveAttribute("src", "https://example.com/preview.png");
    expect(screen.getByRole("link", { name: "Contributing" })).toHaveAttribute(
      "href",
      "https://github.com/acme/workspace/blob/abc123456789/CONTRIBUTING.md",
    );
    expect(screen.getByRole("img", { name: "Architecture" })).toHaveAttribute(
      "src",
      "https://github.com/acme/workspace/raw/abc123456789/docs/img/architecture.png",
    );
  });

  it("keeps partial markdown readable and blocks unsafe link protocols", async () => {
    const fixture = {
      ...workspaceFixture,
      pr: {
        ...workspaceFixture.pr,
        body: "Still typing [a link\n\n[unsafe](javascript:alert(1))",
      },
    };
    mount(fixture);

    expect(
      await screen.findByText(/Still typing \[a link/),
    ).toBeInTheDocument();
    expect(screen.getByText("unsafe").closest("a")).toBeNull();
  });

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
  it("uses the custom merge-method listbox with click and keyboard selection", async () => {
    const client = mount();
    const user = userEvent.setup();
    const picker = await screen.findByRole("button", { name: "Merge method" });

    expect(screen.queryByRole("combobox", { name: "Merge method" })).toBeNull();
    await user.click(picker);
    await user.click(
      screen.getByRole("option", { name: "Create a merge commit" }),
    );
    expect(picker).toHaveTextContent("Create a merge commit");

    await user.click(picker);
    await user.keyboard("{ArrowDown}{Enter}");
    expect(picker).toHaveTextContent("Rebase and merge");
    expect(picker).toHaveFocus();

    await user.tab();
    expect(
      screen.getByRole("button", { name: "Merge pull request…" }),
    ).toHaveFocus();
    await user.keyboard("{Enter}");
    await user.click(screen.getByRole("button", { name: "Confirm merge" }));
    expect(client.pullRequestAction).toHaveBeenCalledWith(
      expect.objectContaining({ action: "merge", method: "rebase" }),
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
  it("preserves a draft and viewed files on refresh, and gates actions for new commits", async () => {
    vi.useFakeTimers();
    Object.defineProperty(document, "hidden", {
      configurable: true,
      value: false,
    });
    try {
      const client = mount();
      await act(async () => {
        await Promise.resolve();
      });
      fireEvent.change(screen.getByLabelText(/Comment or review/), {
        target: { value: "Keep this feedback" },
      });
      fireEvent.change(screen.getByLabelText("Review decision"), {
        target: { value: "APPROVE" },
      });
      fireEvent.click(screen.getByRole("tab", { name: /Files changed/ }));
      fireEvent.click(screen.getAllByLabelText("Viewed")[0]);
      const update = {
        ...workspaceFixture,
        comments: [
          ...workspaceFixture.comments,
          {
            ...workspaceFixture.comments[0],
            id: 2,
            html_url: "https://github.com/comment/2",
            body: "New comment from teammate",
          },
        ],
      };
      client.pullRequestDetail.mockResolvedValue(update);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
      });
      expect(screen.getAllByLabelText("Viewed")[0]).toBeChecked();
      fireEvent.click(screen.getByRole("tab", { name: /Conversation/ }));
      expect(screen.getByText("New comment from teammate")).toBeInTheDocument();
      expect(screen.getByLabelText(/Comment or review/)).toHaveValue(
        "Keep this feedback",
      );
      client.pullRequestDetail.mockResolvedValue({
        ...update,
        pr: { ...update.pr, head: { ...update.pr.head, sha: "new-head" } },
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(PR_DETAIL_REFRESH_MS);
      });
      expect(
        screen.getByRole("button", { name: "Submit review" }),
      ).toBeDisabled();
      expect(
        screen.getByRole("button", { name: "Merge pull request…" }),
      ).toBeDisabled();
      fireEvent.click(
        screen.getByRole("button", { name: "Review new commits" }),
      );
      await act(async () => {
        await Promise.resolve();
      });
      expect(screen.getAllByLabelText("Viewed")[0]).not.toBeChecked();
      fireEvent.click(screen.getByRole("tab", { name: /Conversation/ }));
      expect(screen.getByLabelText(/Comment or review/)).toHaveValue(
        "Keep this feedback",
      );
    } finally {
      vi.useRealTimers();
    }
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
