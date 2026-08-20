/**
 * The mock host stands in for real RPC, so its transitions have to mean what
 * the decisions say they mean. These are the contracts #15/#17/#22/#26 inherit
 * when they replace it: fold hides but does not notify, delete takes the Inbox
 * cards with it, Chief cannot be removed.
 */
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  BOT_TEMPLATES,
  HARNESSES,
  HOST_TOOLS,
  TOOL_CATALOG,
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

  it("offers only tools the host has, with the host's own ids and labels", () => {
    // Same reason as the harness guard below: the bot editor's chips are an
    // allowlist of *host* catalog ids (#18), so a chip the host does not know
    // would save a bot a tool that can never be passed to a session.
    const catalog = readFileSync("src-tauri/src/host/tools/catalog.rs", "utf8");
    const entries = [
      ...catalog
        .slice(catalog.indexOf("pub const CATALOG:"))
        .matchAll(/id: "([^"]+)",\s*\n\s*label: "([^"]+)"/g),
    ].map(([, id, label]) => ({ id, label }));

    expect(entries.map((entry) => entry.id)).toContain("terminal");
    for (const tool of TOOL_CATALOG) {
      expect(entries).toContainEqual({ id: tool.id, label: tool.label });
    }
  });

  it("ships the same template packs the host ships", () => {
    // The packs in `src-tauri/src/host/crew/templates/` are the source of
    // truth (#17); this list is only the fallback the shell renders before
    // `crew/list` answers. A fallback that promises different tools than the
    // host would accept is a template whose Save fails.
    for (const template of BOT_TEMPLATES) {
      const pack = JSON.parse(
        readFileSync(
          `src-tauri/src/host/crew/templates/${template.templateId}.json`,
          "utf8",
        ),
      ) as unknown;
      expect(pack).toEqual({ ...template });
    }
  });

  it("names Chief's host tools exactly as the host names them", () => {
    // Not MCP and not in `tools/list`, so `crew/list` carries them (#6). The
    // ids are what Chief's seeded allowlist contains — a label the host does
    // not know would print a raw id on Chief's card.
    const crew = readFileSync("src-tauri/src/host/crew/mod.rs", "utf8");
    const host = [
      ...crew
        .slice(crew.indexOf("pub const HOST_TOOLS:"))
        .matchAll(/id: "([^"]+)",\s*\n\s*label: "([^"]+)"/g),
    ].map(([, id, label]) => ({ id, label }));

    expect(host).toHaveLength(HOST_TOOLS.length);
    for (const tool of HOST_TOOLS) {
      expect(host).toContainEqual({ id: tool.id, label: tool.label });
    }
  });

  it("offers only harnesses the host can spawn, with the host's own words", () => {
    // Read from the repo root: vitest runs there, and the point of the test is
    // that this list and the host catalog cannot drift apart unnoticed. The
    // catalog is the source of truth for tier 1 and 2 — `store/seed.rs` writes
    // its rows from the same table (#13).
    const catalog = readFileSync(
      "src-tauri/src/host/harness/catalog.rs",
      "utf8",
    );
    const shipped = catalog.slice(
      catalog.indexOf("const SHIPPED:"),
      catalog.indexOf("const PRESETS:"),
    );
    const cards = [
      ...shipped.matchAll(
        /id: "([^"]+)",\s*\n\s*label: "([^"]+)",\s*\n\s*blurb: "([^"]+)",\s*\n\s*accent: "([^"]+)"/g,
      ),
    ].map(([, id, label, blurb, accent]) => ({ id, label, blurb, accent }));

    expect(cards.map((card) => card.id)).toContain("claude");
    for (const harness of HARNESSES) {
      // Presets and custom harnesses reach the UI at runtime through
      // `harness/list`; the seeded cards are the ones the mock may hard-code,
      // and every word of them has to be the host's.
      expect(cards).toContainEqual({
        id: harness.id,
        label: harness.label,
        blurb: harness.blurb,
        accent: harness.accent,
      });
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
