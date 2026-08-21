/**
 * End-to-end: the crew store and its CRUD (#17) over the wire.
 *
 * `src-tauri/src/host/crew/` proves the rules in-process. This file makes the
 * claims a renderer and a user depend on, through the production `HostClient`,
 * a live `jabot-hostd`, a real SQLite store on disk and — for the last case —
 * a real ACP subprocess:
 *
 * - the shipped crew is there on a fresh install, with markdown memory on disk;
 * - adding from a template is a **snapshot**, and it survives a restart;
 * - Chief cannot be removed, and removing anyone else keeps their work;
 * - the bot editor **is** the record: what a save writes is what the next
 *   session is actually spawned with, asserted from the agent's side.
 */
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  CREW_CREATE,
  CREW_LIST,
  CREW_REMOVE,
  CREW_UPDATE,
  RPC_ERROR,
  type BotView,
} from "../../src/host/protocol";
import { fakeAcpRuntime, HostdProcess, type HostdOptions } from "../support/hostd";

const running: HostdProcess[] = [];

async function connected(options: HostdOptions = { persistent: true }) {
  const host = new HostdProcess(options);
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  const hello = await client.hello();
  return { host, client, hello };
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
});

const named = (bots: BotView[], name: string): BotView => {
  const bot = bots.find((candidate) => candidate.name === name);
  if (!bot) throw new Error(`no bot named ${name}: ${bots.map((b) => b.name).join(", ")}`);
  return bot;
};

/** The `session/new` params the adapter actually received — the only place
    "the model never sees a tool it may not call" can honestly be checked. */
async function sessionNewParams(
  host: HostdProcess,
  threadId: string,
): Promise<{ cwd: string; mcpServers: Array<Record<string, unknown>> }> {
  const deadline = Date.now() + 15_000;
  for (;;) {
    const line = host
      .readAdapterLog(threadId)
      .split("\n")
      .find((entry) => entry.startsWith("session_new="));
    if (line) return JSON.parse(line.slice("session_new=".length));
    if (Date.now() > deadline) {
      throw new Error(`no session/new reached the adapter for ${threadId}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 30));
  }
}

describe("the shipped crew", () => {
  it("advertises its methods and comes with Chief plus five workers", async () => {
    const { client, hello } = await connected();

    for (const method of [CREW_LIST, CREW_CREATE, CREW_UPDATE, CREW_REMOVE]) {
      expect(hello.methods).toContain(method);
    }

    const { bots, templates, hostTools } = await client.listCrew();
    expect(bots.map((bot) => bot.name)).toEqual([
      "Chief",
      "Code",
      "Inbox Mgr",
      "Scheduler",
      "Research",
      "Writer",
    ]);
    expect(bots.filter((bot) => bot.isChief)).toHaveLength(1);
    expect(bots[0].isChief).toBe(true);

    // The four shipped packs, and the host tools Chief's chips name.
    expect(templates.map((template) => template.templateId)).toEqual([
      "expense",
      "talent",
      "social",
      "ops",
    ]);
    expect(hostTools.map((tool) => tool.id)).toContain("handoff_to_bot");
    for (const chip of named(bots, "Chief").tools) {
      expect(hostTools.map((tool) => tool.id)).toContain(chip);
    }
  });

  it("gives every bot its own directory with instructions.md and MEMORY.md", async () => {
    const { host, client } = await connected();
    const { bots } = await client.listCrew();

    const dirs = bots.map((bot) => bot.memoryDir!);
    expect(new Set(dirs).size).toBe(dirs.length);
    for (const dir of dirs) {
      // Under the host's own data directory — a bot's memory is local
      // markdown, not a cloud store (#17).
      expect(dir.startsWith(host.dataDir!)).toBe(true);
      expect(existsSync(path.join(dir, "MEMORY.md"))).toBe(true);
    }

    const writer = named(bots, "Writer");
    const instructions = readFileSync(
      path.join(writer.memoryDir!, "instructions.md"),
      "utf8",
    );
    expect(instructions).toContain(writer.instructions);
  });
});

describe("templates", () => {
  it("snapshots the pack, and the snapshot survives a restart", async () => {
    const first = await connected();
    const added = await first.client.createBot({ templateId: "expense" });

    expect(added.name).toBe("Expense Manager");
    expect(added.tools).toEqual(["gmail", "drive"]);
    expect(added.templateId).toBe("expense");
    expect(added.isChief).toBe(false);

    // Editing the instance is editing a bot, not the pack.
    const edited = await first.client.updateBot({
      botId: added.botId,
      name: "Receipts",
      instructions: "File the monthly report. Ask before anything unusual.",
    });
    expect(edited.name).toBe("Receipts");
    expect(edited.templateId).toBe("expense");

    const dataDir = first.host.dataDir!;
    await first.host.stop();

    // Decision #4: quit persists, the next launch resumes from disk.
    const second = await connected({ dataDir });
    const { bots } = await second.client.listCrew();
    const reloaded = named(bots, "Receipts");
    expect(reloaded.botId).toBe(added.botId);
    expect(reloaded.instructions).toContain("File the monthly report");
    expect(reloaded.memoryDir).toBe(added.memoryDir);

    // And the pack is untouched, so the next copy is the original.
    const again = await second.client.createBot({ templateId: "expense" });
    expect(again.name).toBe("Expense Manager");
    expect(again.tools).toEqual(["gmail", "drive"]);
    expect(again.botId).not.toBe(added.botId);
    expect(again.memoryDir).not.toBe(added.memoryDir);
  });

  it("refuses a template nobody ships and writes nothing", async () => {
    const { client } = await connected();
    const before = (await client.listCrew()).bots.length;

    await expect(client.createBot({ templateId: "unicorn" })).rejects.toBeInstanceOf(
      HostRpcError,
    );
    expect((await client.listCrew()).bots).toHaveLength(before);
  });
});

describe("removing a bot", () => {
  it("refuses Chief, by name and by code", async () => {
    const { host, client } = await connected();
    const chief = named((await client.listCrew()).bots, "Chief");

    const refused = await host.call(CREW_REMOVE, { botId: chief.botId });
    expect(refused.error?.code).toBe(RPC_ERROR.CHIEF_REQUIRED);
    expect(refused.error?.message).toContain("Chief");

    const after = (await client.listCrew()).bots;
    expect(after.filter((bot) => bot.isChief)).toHaveLength(1);
  });

  it("keeps the threads it started and the notes it wrote", async () => {
    const { client } = await connected();
    const research = named((await client.listCrew()).bots, "Research");

    await client.openThread({
      threadId: "t-brief",
      title: "Brief me on ACP",
      cwd: tmpdir(),
      harnessId: "claude",
      botId: research.botId,
    });

    const removed = await client.removeBot({ botId: research.botId });
    expect(removed.removed).toBe(true);
    expect(removed.detachedThreads).toBe(1);
    expect(removed.memoryDir).toBe(research.memoryDir);

    expect(
      (await client.listCrew()).bots.map((bot) => bot.name),
    ).not.toContain("Research");

    // The work survives with everything it was stamped with at spawn.
    const thread = await client.threadState({ threadId: "t-brief" });
    expect(thread.botId).toBeUndefined();
    expect(thread.title).toBe("Brief me on ACP");
    // Removing a bot from the grid is not a licence to delete the user's
    // markdown, and the result said where it is.
    expect(existsSync(path.join(removed.memoryDir!, "MEMORY.md"))).toBe(true);
  });
});

describe("the editor is the record", () => {
  /**
   * The claim that matters most, checked from the *agent's* side: a chip
   * pressed in the bot editor is a server that reaches `session/new`, and one
   * un-pressed is a server that does not. Anything less proves only that a row
   * changed.
   */
  it("what a save writes is what the next session is spawned with", async () => {
    const { host, client } = await connected();
    const writer = named((await client.listCrew()).bots, "Writer");

    // Writer ships with Gmail and Notion, neither of which is connected on a
    // fresh install — so a session gets no servers at all.
    await client.openThread({
      threadId: "t-before",
      title: "before",
      cwd: tmpdir(),
      harnessId: "claude",
      botId: writer.botId,
      runtime: fakeAcpRuntime(),
    });
    await client.prompt({ threadId: "t-before", content: "hi" });
    expect((await sessionNewParams(host, "t-before")).mcpServers).toEqual([]);

    // Press Browser in the editor: a local MCP server that needs no grant.
    const saved = await client.updateBot({
      botId: writer.botId,
      tools: ["browser"],
      instructions: "Read the web and brief me short.",
    });
    expect(saved.tools).toEqual(["browser"]);

    await client.openThread({
      threadId: "t-after",
      title: "after",
      cwd: tmpdir(),
      harnessId: "claude",
      botId: writer.botId,
      runtime: fakeAcpRuntime(),
    });
    await client.prompt({ threadId: "t-after", content: "hi" });

    const params = await sessionNewParams(host, "t-after");
    expect(params.mcpServers.map((server) => server.name)).toEqual(["browser"]);
    // The tools that were unpressed are gone, not merely unconnected.
    expect(JSON.stringify(params.mcpServers)).not.toContain("notion");

    // And the persona the editor saved is on disk for the session to read.
    expect(
      readFileSync(path.join(saved.memoryDir!, "instructions.md"), "utf8"),
    ).toContain("Read the web and brief me short.");
  });

  it("refuses a tool no catalog knows, before anything is written", async () => {
    const { host, client } = await connected();
    const writer = named((await client.listCrew()).bots, "Writer");

    const refused = await host.call(CREW_UPDATE, {
      botId: writer.botId,
      tools: ["telepathy"],
    });
    expect(refused.error?.code).toBe(RPC_ERROR.INVALID_PARAMS);
    expect(refused.error?.message).toContain("telepathy");

    const after = named((await client.listCrew()).bots, "Writer");
    expect(after.tools).toEqual(writer.tools);
  });
});
