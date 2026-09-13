/**
 * Question and plan-review cards in the real renderer against the real host
 * (#298): the card is drawn from a live `cursor/ask_question`, answered from
 * the keyboard, the agent hears exactly the ids chosen, and the decision is
 * still on the card after a reload.
 */
import { fakeAcpRuntime } from "../support/hostd";
import { test, expect } from "./fixtures";
import { seedCodeThread } from "./host";
import {
  captureEvidence,
  expectSettledAgent,
  openConnectedApp,
  openThread,
  sendComposer,
} from "./ui";

async function waitForAdapterLog(
  jabot: { adapterLog: (threadId: string) => string },
  threadId: string,
  needle: string,
  timeoutMs = 15_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  let last = "";
  while (Date.now() < deadline) {
    last = jabot.adapterLog(threadId);
    if (last.includes(needle)) return last;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(
    `adapter log for ${threadId} never contained ${needle}; last:\n${last}`,
  );
}

test.describe("questions and plans from an agent's extensions", () => {
  test("answers a question from the keyboard, once, in the agent's own ids", async ({
    page,
    jabot,
  }) => {
    await seedCodeThread(jabot, {
      threadId: "t-ask",
      title: "Cursor question",
      runtime: fakeAcpRuntime("cursor-ask"),
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Cursor question");
    await sendComposer(page, "migrate the auth service", "Cursor question");

    const card = page.locator("[data-ask='question']");
    await expect(card).toBeVisible();
    await expect(
      card.getByRole("group", { name: "Which mode?" }),
    ).toBeVisible();
    const send = card.getByRole("button", { name: "Send answer" });
    await expect(send).toBeDisabled();
    // Its own list, never the permission one.
    const pending = await jabot.rpc<{ requests: { ask: string }[] }>(
      "interaction/pending",
      { threadId: "t-ask" },
    );
    expect(pending.requests).toHaveLength(1);
    expect(pending.requests[0].ask).toBe("question");
    const permissions = await jabot.rpc<{ requests: unknown[] }>(
      "permission/pending",
      { threadId: "t-ask" },
    );
    expect(permissions.requests).toHaveLength(0);
    await captureEvidence(page, "interaction-question");

    // Keyboard only: arrow within the radio group, Tab to Send, Enter.
    await card.getByRole("radio", { name: "Agent" }).focus();
    await page.keyboard.press("ArrowDown");
    await expect(card.getByRole("radio", { name: "Plan" })).toBeChecked();
    await expect(send).toBeEnabled();
    await page.keyboard.press("Tab");
    await expect(send).toBeFocused();
    await page.keyboard.press("Enter");

    const status = card.getByRole("status");
    await expect(status).toHaveText(/Answered: Plan\./);
    // Focus recovered onto the line that says what happened.
    await expect(status).toBeFocused();
    await expect(send).toHaveCount(0);
    await expectSettledAgent(page, /selectedOptionIds/);
    await captureEvidence(page, "interaction-question-answered");

    const log = await waitForAdapterLog(jabot, "t-ask", "permission_reply=");
    const replies = log
      .split("\n")
      .filter((line) => line.startsWith("permission_reply="));
    expect(replies).toHaveLength(1);
    expect(replies[0]).toContain('"selectedOptionIds":["plan"]');
    const after = await jabot.rpc<{ requests: unknown[] }>(
      "interaction/pending",
      { threadId: "t-ask" },
    );
    expect(after.requests).toHaveLength(0);

    // The decision is part of the conversation: still there after a reload.
    await page.reload();
    await openThread(page, "Cursor question");
    await expect(
      page.locator("[data-ask='question']").getByRole("status"),
    ).toHaveText(/Answered: Plan\./);
  });

  test("reviews a plan and rejects it with a reason", async ({
    page,
    jabot,
  }) => {
    await seedCodeThread(jabot, {
      threadId: "t-plan",
      title: "Cursor plan",
      runtime: fakeAcpRuntime("cursor-plan"),
    });
    await openConnectedApp(page, jabot.baseURL);
    await openThread(page, "Cursor plan");
    await sendComposer(page, "plan the migration", "Cursor plan");

    const card = page.locator("[data-ask='plan']");
    await expect(card).toBeVisible();
    await expect(
      card.getByText("Move session handling onto the new auth service."),
    ).toBeVisible();
    await expect(card.getByRole("list", { name: "Plan steps" })).toBeVisible();
    await expect(card.getByRole("status")).toHaveText(
      /does not change what the agent is allowed to do/,
    );
    await captureEvidence(page, "interaction-plan");

    await card.getByRole("button", { name: "Reject…" }).click();
    const why = card.getByLabel("Why? (optional)");
    await expect(why).toBeFocused();
    await why.fill("too broad");
    await page.keyboard.press("Enter");
    await expect(card.getByRole("status")).toHaveText(
      /Plan rejected — too broad/,
    );
    await expect(card.getByRole("button", { name: "Accept plan" })).toHaveCount(
      0,
    );
    await expectSettledAgent(page, /rejected/);
    await captureEvidence(page, "interaction-plan-rejected");

    const log = await waitForAdapterLog(jabot, "t-plan", "permission_reply=");
    expect(log).toContain('"rejected"');
    expect(log).toContain("too broad");
  });

  test("lists both in the Inbox as their own kinds, and settles a plan from there", async ({
    page,
    jabot,
  }) => {
    await seedCodeThread(jabot, {
      threadId: "t-inbox-q",
      title: "Waiting question",
      runtime: fakeAcpRuntime("cursor-ask"),
    });
    await seedCodeThread(jabot, {
      threadId: "t-inbox-p",
      title: "Waiting plan",
      runtime: fakeAcpRuntime("cursor-plan"),
    });
    // Prerequisite only: the asks exist before the page loads.
    await jabot.rpc("session/prompt", {
      threadId: "t-inbox-q",
      content: "go",
    });
    await jabot.rpc("session/prompt", {
      threadId: "t-inbox-p",
      content: "go",
    });
    await expect
      .poll(async () => {
        const pending = await jabot.rpc<{ requests: unknown[] }>(
          "interaction/pending",
          {},
        );
        return pending.requests.length;
      })
      .toBe(2);

    await openConnectedApp(page, jabot.baseURL);
    // The nav entry, not a thread whose title happens to start with "Inbox".
    await page.getByRole("button", { name: /^Inbox(\s—.*)?$/ }).click();
    await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();
    await expect(page.getByText("QUESTION", { exact: true })).toBeVisible();
    await expect(page.getByText("PLAN REVIEW", { exact: true })).toBeVisible();
    // Never a permission pill.
    await expect(page.getByText("PERMISSION", { exact: true })).toHaveCount(0);
    await captureEvidence(page, "interaction-inbox");

    // The card is titled by the plan's own name, not the thread's.
    await page
      .getByRole("button", { name: /Auth migration.*PLAN REVIEW/ })
      .click();
    await page.getByRole("button", { name: "Accept plan" }).click();
    await expect(page.getByText("PLAN REVIEW", { exact: true })).toHaveCount(0);
    const log = await waitForAdapterLog(
      jabot,
      "t-inbox-p",
      "permission_reply=",
    );
    expect(log).toContain('"accepted"');
    // The question is still waiting; the plan is not.
    const pending = await jabot.rpc<{ requests: { ask: string }[] }>(
      "interaction/pending",
      {},
    );
    expect(pending.requests.map((r) => r.ask)).toEqual(["question"]);
  });
});
