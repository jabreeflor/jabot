/**
 * One Chromium-required smoke: Chief → composer → fake reply → reload.
 *
 * Actions under test go through visible controls. RPC is used only to put
 * Chief on `fake-acp` before load (the standing thread takes the harness at
 * first open) and to read the durable transcript after reload.
 */
import { chiefTranscript } from "./host";
import { test, expect } from "./fixtures";
import {
  agentBubble,
  captureEvidence,
  chiefRow,
  composer,
  openConnectedApp,
  sendComposer,
  userBubble,
  waitForConnected,
} from "./ui";

const USER_TEXT = "hello from the browser smoke";
const AGENT_TEXT = "hello from fake-acp";

test(
  "Chief composer send persists through reload",
  { tag: "@smoke" },
  async ({ page, jabot }) => {
    await openConnectedApp(page, jabot.baseURL);

    await chiefRow(page).click();
    await expect(
      page.getByRole("heading", { level: 2, name: "Chief" }),
    ).toBeVisible();
    await expect(composer(page)).toBeVisible();
    await captureEvidence(page, "connected-chief");

    await sendComposer(page, USER_TEXT);
    await expect(userBubble(page)).toContainText(USER_TEXT);
    await expect(agentBubble(page)).toContainText(AGENT_TEXT);
    await captureEvidence(page, "fake-reply");

    await page.reload({ waitUntil: "domcontentloaded" });
    await waitForConnected(page);
    await chiefRow(page).click();
    await expect(userBubble(page)).toContainText(USER_TEXT);
    await expect(agentBubble(page)).toContainText(AGENT_TEXT);
    await captureEvidence(page, "reload-persisted");

    const stored = await chiefTranscript(jabot.baseURL);
    const dumped = JSON.stringify(stored.events);
    expect(dumped).toContain(USER_TEXT);
    expect(dumped).toContain(AGENT_TEXT);
  },
);
