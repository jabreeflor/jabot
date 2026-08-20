/**
 * The mock host stands in for real RPC, so its transitions have to mean what
 * the decisions say they mean. These are the contracts #15/#17/#22/#26 inherit
 * when they replace it: fold hides but does not notify, delete takes the Inbox
 * cards with it, Chief cannot be removed.
 */
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  HARNESSES,
  initialMockState,
  mockHostReducer,
  needsYouCount,
  nextThreadId,
  sidebarFolders,
  type MockState,
} from "../views/mock-host";

function threadIds(state: MockState, folderId: string): string[] {
  const folder = sidebarFolders(state).find((f) => f.id === folderId);
  return folder?.threads.map((thread) => thread.id) ?? [];
}

describe("sidebarFolders", () => {
  it("hides folded threads — that is what fold promises", () => {
    const state = initialMockState();
    const nas = state.threads.find((thread) => thread.id === "nas");

    expect(nas?.state).toBe("folded");
    expect(threadIds(state, "globnet-sync")).not.toContain("nas");
    expect(threadIds(state, "globnet-sync")).toContain("retry");
  });
});

describe("the seed", () => {
  it("gives every PR the session that opened it", () => {
    const state = initialMockState();
    const threads = new Set(state.threads.map((thread) => thread.id));

    expect(state.pullRequests.length).toBeGreaterThan(0);
    for (const pr of state.pullRequests) {
      // `thread_prs.thread_id` is NOT NULL; a PR with no session is a row the
      // store cannot hold, so the mock must not invent one.
      expect(threads).toContain(pr.threadId);
    }
  });

  it("offers only harnesses the store can spawn", () => {
    const seed = readFileSync(
      new URL("../../src-tauri/src/host/store/seed.rs", import.meta.url),
      "utf8",
    );
    const builtins = [...seed.matchAll(/BuiltinHarness \{\s*id: "([^"]+)"/g)].map(
      (match) => match[1],
    );

    expect(builtins).toContain("claude");
    for (const harness of HARNESSES) {
      expect(builtins).toContain(harness.id);
    }
  });
});

describe("foldThread", () => {
  it("keeps the thread, drops the row, and writes a sleeping Inbox card", () => {
    const before = initialMockState();
    const after = mockHostReducer(before, {
      type: "foldThread",
      threadId: "auth",
    });

    expect(threadIds(after, "jabot-app")).not.toContain("auth");
    expect(after.threads.find((t) => t.id === "auth")).toMatchObject({
      state: "folded",
      foldPolicy: "wait_for_inbox",
    });

    const card = after.inbox.find((c) => c.threadId === "auth");
    expect(card).toMatchObject({ kind: "folded" });
    expect(card?.title).toBe("Auth migration");
  });

  it("does not increment the needs-you badge — sleeping is not a summons", () => {
    const before = initialMockState();
    const after = mockHostReducer(before, {
      type: "foldThread",
      threadId: "auth",
    });

    expect(needsYouCount(after)).toBe(needsYouCount(before));
  });

  it("is idempotent for a thread that is already folded", () => {
    const state = initialMockState();
    const after = mockHostReducer(state, {
      type: "foldThread",
      threadId: "nas",
    });

    expect(after).toBe(state);
  });
});

describe("startThread", () => {
  it("files the thread under its folder with the harness that was picked", () => {
    const before = initialMockState();
    const id = nextThreadId(before);
    const after = mockHostReducer(before, {
      type: "startThread",
      draft: {
        harnessId: "codex",
        folderId: "globnet-sync",
        task: "Add dark mode to settings",
      },
    });

    expect(threadIds(after, "globnet-sync")).toContain(id);
    expect(after.threads.find((t) => t.id === id)).toMatchObject({
      harnessId: "codex",
      title: "Add dark mode to settings",
      runState: "queued",
    });
    expect(after.transcripts[id][0]).toMatchObject({
      kind: "user",
      text: "Add dark mode to settings",
    });
  });

  it("keeps a folderless thread out of every folder", () => {
    const before = initialMockState();
    const id = nextThreadId(before);
    const after = mockHostReducer(before, {
      type: "startThread",
      draft: { harnessId: "claude", folderId: null, task: "Scratch" },
    });

    for (const folder of sidebarFolders(after)) {
      expect(folder.threads.map((t) => t.id)).not.toContain(id);
    }
    expect(after.threads.some((t) => t.id === id)).toBe(true);
  });
});

describe("answerNotice", () => {
  it("folding from Chief's card folds the thread it is about", () => {
    const before = initialMockState();
    const after = mockHostReducer(before, {
      type: "answerNotice",
      conversationId: "chief",
      itemId: "chief-3",
      actionId: "fold",
    });

    expect(after.threads.find((t) => t.id === "auth")?.state).toBe("folded");

    const chief = after.transcripts.chief;
    expect(chief.find((item) => item.id === "chief-3")).toMatchObject({
      resolved: true,
    });
    expect(chief[chief.length - 2]).toMatchObject({
      kind: "sys",
      text: "Thread folded — will reappear in Inbox",
    });
    expect(chief[chief.length - 1].kind).toBe("agent");
  });

  it("keeps watching without folding anything", () => {
    const before = initialMockState();
    const after = mockHostReducer(before, {
      type: "answerNotice",
      conversationId: "chief",
      itemId: "chief-3",
      actionId: "watch",
    });

    expect(after.threads.find((t) => t.id === "auth")?.state).toBe("active");
    expect(after.transcripts.chief).toHaveLength(
      before.transcripts.chief.length,
    );
  });
});

describe("removeNotice", () => {
  it("takes the answered card out of the transcript, not just out of sight", () => {
    const answered = mockHostReducer(initialMockState(), {
      type: "answerNotice",
      conversationId: "chief",
      itemId: "chief-3",
      actionId: "watch",
    });
    const after = mockHostReducer(answered, {
      type: "removeNotice",
      conversationId: "chief",
      itemId: "chief-3",
    });

    expect(after.transcripts.chief.some((item) => item.id === "chief-3")).toBe(
      false,
    );
    // The rest of the conversation is untouched — only the card goes.
    expect(after.transcripts.chief).toHaveLength(
      answered.transcripts.chief.length - 1,
    );
  });

  it("is a no-op for a card that has already gone", () => {
    const state = initialMockState();
    const after = mockHostReducer(state, {
      type: "removeNotice",
      conversationId: "chief",
      itemId: "chief-3-again",
    });

    expect(after).toBe(state);
  });
});

describe("deleteThread", () => {
  it("takes the thread's Inbox cards with it", () => {
    const before = initialMockState();
    expect(before.inbox.some((card) => card.threadId === "nas")).toBe(true);

    const after = mockHostReducer(before, {
      type: "deleteThread",
      threadId: "nas",
    });

    expect(after.threads.some((t) => t.id === "nas")).toBe(false);
    expect(after.inbox.some((card) => card.threadId === "nas")).toBe(false);
  });
});

describe("crew", () => {
  it("refuses to remove Chief — the schema allows exactly one", () => {
    const state = initialMockState();
    const after = mockHostReducer(state, { type: "removeBot", botId: "chief" });

    expect(after.bots.some((bot) => bot.isChief)).toBe(true);
  });

  it("edits a bot in place and creates a new one with its harness", () => {
    const state = initialMockState();
    const edited = mockHostReducer(state, {
      type: "saveBot",
      botId: "writer",
      draft: {
        name: "Ghostwriter",
        color: "b-pink",
        instructions: "Short and plain.",
        tools: ["gmail"],
        harnessId: "codex",
      },
    });

    expect(edited.bots.find((bot) => bot.id === "writer")).toMatchObject({
      name: "Ghostwriter",
      harnessId: "codex",
    });
    expect(edited.bots).toHaveLength(state.bots.length);

    const added = mockHostReducer(edited, {
      type: "saveBot",
      botId: null,
      draft: {
        name: "Expense Manager",
        color: "b-green",
        instructions: "Chase receipts.",
        tools: ["gmail", "drive"],
        harnessId: "pi",
      },
    });

    const created = added.bots[added.bots.length - 1];
    expect(created).toMatchObject({
      name: "Expense Manager",
      harnessId: "pi",
      isChief: false,
    });
    expect(added.transcripts[created.id]).toBeDefined();
  });
});
