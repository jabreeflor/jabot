/**
 * PR workspace against a synthetic repo and a fixture `gh`.
 *
 * No real tokens, no live GitHub. The fixture answers GraphQL and auth; it
 * does not claim API compatibility. Vite sees the fixture on PATH before
 * jabot-hostd starts — same contract as `tests/e2e/pr.test.ts`.
 */
import { fakeAcpRuntime } from "../support/hostd";
import { browserTest, expect } from "./fixtures";
import { addOrigin, tempRepository } from "./helpers/git";
import { createFakeGh, setGhMode, writeBoardPr, type FakeGh } from "./helpers/gh";
import { captureEvidence, openConnectedApp } from "./ui";

let gh: FakeGh;
const test = browserTest(() => {
  gh = createFakeGh();
  writeBoardPr(gh);
  return { pathPrefix: [gh.dir] };
});

test.describe("PR workspace", () => {
  test("sign-in refusal, board search/filter, details, reopen, failed refresh", async ({
    page,
    jabot,
  }) => {
    const repo = tempRepository();
    addOrigin(repo.dir, "git@github.com:jabreeflor/jabot.git");
    const folder = await jabot.rpc<{ folderId: string; cwd: string }>("folder/register", {
      path: repo.dir,
      name: "jabot",
    });
    await jabot.rpc("thread/open", {
      threadId: "t-auth",
      title: "Auth migration",
      cwd: folder.cwd,
      harnessId: "fake-acp",
      folderId: folder.folderId,
      runtime: fakeAcpRuntime("execute"),
    });
    await jabot.rpc("session/prompt", {
      threadId: "t-auth",
      content: "https://github.com/jabreeflor/jabot/pull/23",
    });

    const deadline = Date.now() + 15_000;
    let listed = { pullRequests: [] as Array<{ number: number; title: string }> };
    while (Date.now() < deadline) {
      listed = await jabot.rpc("pr/list");
      if (listed.pullRequests.length > 0) break;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    expect(listed.pullRequests.length).toBeGreaterThan(0);

    await jabot.rpc("pr/refresh");
    const filled = await jabot.rpc<{ pullRequests: Array<{ title: string }> }>("pr/list");
    expect(filled.pullRequests.some((pr) => /Migrate auth/i.test(pr.title))).toBe(true);

    await openConnectedApp(page, jabot.baseURL);
    await page.getByRole("button", { name: /^Pull Requests/ }).click();
    await expect(page.getByRole("heading", { name: "Pull Requests" })).toBeVisible();
    await captureEvidence(page, "pr-board");

    await page.getByRole("button", { name: "Sign in with GitHub" }).click();
    await expect(page.getByRole("heading", { name: "Sign in to GitHub" })).toBeVisible();
    await page.getByPlaceholder("ghp_… or github_pat_…").fill("ghp_badtokenvalue");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText(/Bad credentials/i);
    await expect(page.getByRole("heading", { name: "Sign in to GitHub" })).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();

    await expect(page.getByText("Migrate auth to sessions")).toBeVisible();
    await page.getByLabel("Search pull requests").fill("toolchain");
    await expect(page.getByText("No pull requests here.")).toBeVisible();
    await page.getByLabel("Search pull requests").fill("auth");
    await expect(page.getByText("Migrate auth to sessions")).toBeVisible();

    await page.getByRole("tab", { name: /Drafts/ }).click();
    await expect(page.getByText("No pull requests here.")).toBeVisible();
    await page.getByRole("tab", { name: /Open/ }).click();
    await expect(page.getByText("Migrate auth to sessions")).toBeVisible();

    await page.getByRole("button", { name: "Open pull request →" }).click();
    await expect(
      page.getByRole("heading", { name: /Migrate auth to sessions/ }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Open coding session →" }).click();
    await expect(page.getByRole("heading", { name: "Auth migration" })).toBeVisible();

    await page.getByRole("button", { name: /^Pull Requests/ }).click();
    setGhMode(gh, "fail");
    await page.getByRole("button", { name: "Refresh" }).click();
    await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
    await expect(page.getByRole("status").filter({ hasText: /fail|unavailable|refresh/i })).toBeVisible();

    setGhMode(gh, "ok");
    await page.getByRole("button", { name: "Retry" }).click();
    await expect(page.getByRole("button", { name: "Retry" })).toHaveCount(0);
  });
});
