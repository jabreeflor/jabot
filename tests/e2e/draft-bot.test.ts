/**
 * End-to-end: conversational bot drafts and host-owned prompt composition (#237).
 *
 * Host unit tests prove validation and Save atomicity. This file makes the
 * claims a running Chief / Recruiter depends on, through the production
 * `HostClient`, a live `jabot-hostd`, a real SQLite store, a real ACP
 * adapter, and a real HTTP MCP client:
 *
 * - `session/prompt` carries versioned Jabot context and the current persona;
 * - the transcript stays the user's exact words;
 * - `draft_bot` / `get_bot_draft` work over the loopback bridge;
 * - Save creates one crew member and never starts a run;
 * - a bot without the grant cannot create; an approver cannot Save.
 */
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import {
  CREW_DRAFT,
  CREW_DRAFT_SAVE,
  CREW_DRAFTS,
  RPC_ERROR,
  type BotView,
} from "../../src/host/protocol";
import {
  fakeAcpAgentPath,
  HostdProcess,
  type HostdOptions,
} from "../support/hostd";

const running: HostdProcess[] = [];

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
});

function dataDirWithFakeHarness(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-draft-"));
  mkdirSync(path.join(dir, "custom_harnesses"), { recursive: true });
  writeFileSync(
    path.join(dir, "custom_harnesses", "fake-acp.json"),
    JSON.stringify({
      id: "fake-acp",
      label: "Fake ACP",
      command: fakeAcpAgentPath(),
      args: [],
    }),
  );
  return dir;
}

async function connected(options: HostdOptions) {
  const host = new HostdProcess(options);
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  const hello = await client.hello();
  return { host, client, hello };
}

const named = (bots: BotView[], name: string): BotView => {
  const bot = bots.find((candidate) => candidate.name === name);
  if (!bot) throw new Error(`no bot named ${name}`);
  return bot;
};

interface HttpMcpServer {
  type: string;
  name: string;
  url: string;
  headers: Array<{ name: string; value: string }>;
}

interface JsonRpcAnswer {
  result?: Record<string, unknown>;
  error?: { code: number; message: string };
}

interface McpToolResult {
  isError?: boolean;
  content?: Array<{ type: string; text?: string }>;
  structuredContent?: Record<string, unknown>;
}

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

async function sessionPromptParams(
  host: HostdProcess,
  threadId: string,
): Promise<{ prompt?: unknown[] } & Record<string, unknown>> {
  const log = await host.waitForAdapterLog(threadId, (text) =>
    text.includes("session_prompt="),
  );
  const line = log
    .split("\n")
    .find((entry) => entry.startsWith("session_prompt="));
  if (!line) throw new Error(`no session/prompt for ${threadId}`);
  return JSON.parse(line.slice("session_prompt=".length));
}

async function chiefWithBridge(dataDir: string) {
  const { host, client } = await connected({ dataDir });
  const chief = named((await client.listCrew()).bots, "Chief");
  await client.updateBot({ botId: chief.botId, harnessId: "fake-acp" });
  const thread = await client.botThread({ botId: chief.botId });
  await client.prompt({
    threadId: thread.threadId,
    content: "Make me a research bot that checks sources.",
  });
  const params = await sessionNewParams(host, thread.threadId);
  const server = params.mcpServers.find((entry) => entry.name === "jabot") as
    | HttpMcpServer
    | undefined;
  if (!server) {
    throw new Error(
      `no host tool server on session/new: ${JSON.stringify(params.mcpServers)}`,
    );
  }
  return { host, client, chief, thread, server };
}

let nextId = 1;

async function mcp(
  server: HttpMcpServer,
  method: string,
  params: unknown = {},
  bearer?: string,
): Promise<{ status: number; body: JsonRpcAnswer }> {
  const authorization =
    bearer ??
    server.headers.find((header) => header.name === "Authorization")!.value;
  const response = await fetch(server.url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json, text/event-stream",
      Authorization: authorization,
    },
    body: JSON.stringify({ jsonrpc: "2.0", id: nextId++, method, params }),
  });
  const text = await response.text();
  const isJson =
    response.headers.get("content-type")?.startsWith("application/json") ??
    false;
  return {
    status: response.status,
    body: isJson && text ? (JSON.parse(text) as JsonRpcAnswer) : {},
  };
}

async function callTool<T>(
  server: HttpMcpServer,
  name: string,
  args: Record<string, unknown> = {},
): Promise<{ ok: boolean; value: T; text: string }> {
  const { body } = await mcp(server, "tools/call", { name, arguments: args });
  expect(body.error, JSON.stringify(body)).toBeUndefined();
  const result = body.result as unknown as McpToolResult;
  return {
    ok: result.isError !== true,
    value: (result.structuredContent ?? {}) as T,
    text: result.content?.[0]?.text ?? "",
  };
}

describe("prompt composition", () => {
  it("prepends versioned Jabot context and keeps the transcript exact", async () => {
    const { host, client, thread } = await chiefWithBridge(
      dataDirWithFakeHarness(),
    );
    const prompted = await sessionPromptParams(host, thread.threadId);
    const blocks = (prompted.prompt ?? prompted.content) as Array<{
      type: string;
      text?: string;
    }>;
    expect(blocks[0]?.type).toBe("text");
    expect(blocks[0]?.text).toContain("Jabot context v1");
    expect(blocks[0]?.text).toContain("draft_bot");
    expect(blocks[0]?.text).toMatch(/name: Chief/);
    const last = blocks[blocks.length - 1];
    expect(last.text).toContain("Make me a research bot that checks sources.");

    const transcript = await client.threadTranscript({
      threadId: thread.threadId,
    });
    const said = JSON.stringify(transcript.events);
    expect(said).toContain("Make me a research bot that checks sources.");
    expect(said).not.toContain("Jabot context v1");
  });

  it("gives two bots their own persona on the wire", async () => {
    const dataDir = dataDirWithFakeHarness();
    const { host, client } = await connected({ dataDir });
    const researcher = await client.createBot({
      name: "Researcher",
      instructions: "Cite sources. SECRET_RESEARCHER",
      tools: [],
      harnessId: "fake-acp",
    });
    const writer = await client.createBot({
      name: "Writer",
      instructions: "Draft in my voice. SECRET_WRITER",
      tools: [],
      harnessId: "fake-acp",
    });
    const researchThread = await client.botThread({ botId: researcher.botId });
    const writerThread = await client.botThread({ botId: writer.botId });
    await client.prompt({
      threadId: researchThread.threadId,
      content: "research this",
    });
    await client.prompt({ threadId: writerThread.threadId, content: "write this" });
    const researchPrompt = await sessionPromptParams(
      host,
      researchThread.threadId,
    );
    const writerPrompt = await sessionPromptParams(host, writerThread.threadId);
    const researchText = JSON.stringify(researchPrompt);
    const writerText = JSON.stringify(writerPrompt);
    expect(researchText).toContain("SECRET_RESEARCHER");
    expect(researchText).not.toContain("SECRET_WRITER");
    expect(writerText).toContain("SECRET_WRITER");
    expect(writerText).not.toContain("SECRET_RESEARCHER");
  });
});

describe("draft_bot over the live bridge", () => {
  it("lists the tools, submits a pending draft, and Save creates one bot", async () => {
    const { host, client, server } = await chiefWithBridge(
      dataDirWithFakeHarness(),
    );
    const listed = await mcp(server, "tools/list");
    const names = (
      (listed.body.result?.tools ?? []) as Array<{ name: string }>
    ).map((tool) => tool.name);
    expect(names).toContain("draft_bot");
    expect(names).toContain("get_bot_draft");

    const before = (await client.listCrew()).bots.length;
    const submitted = await callTool<{
      draftId: string;
      status: string;
      saved: boolean;
      name: string;
    }>(server, "draft_bot", {
      requestKey: "research-1",
      name: "Researcher",
      instructions: "Check sources and summarize findings.",
      tools: ["browser"],
    });
    expect(submitted.ok, submitted.text).toBe(true);
    expect(submitted.value.status).toBe("pending_review");
    expect(submitted.value.saved).toBe(false);
    expect(submitted.value.name).toBe("Researcher");
    expect((await client.listCrew()).bots).toHaveLength(before);

    const notice = host.notifications(CREW_DRAFT)[0];
    expect(notice?.params).toMatchObject({
      draftId: submitted.value.draftId,
      status: "pending_review",
      name: "Researcher",
    });

    const listedDrafts = await client.listBotDrafts();
    expect(listedDrafts.drafts.map((draft) => draft.draftId)).toContain(
      submitted.value.draftId,
    );
    const draft = listedDrafts.drafts.find(
      (row) => row.draftId === submitted.value.draftId,
    )!;
    expect(draft.harnessId).toBe("fake-acp");
    expect(draft.tools).toEqual(["browser"]);
    expect(draft.botId).toBeUndefined();

    const lookedUp = await callTool<{
      draftId: string;
      saved: boolean;
      botId?: string;
    }>(server, "get_bot_draft", { requestKey: "research-1" });
    expect(lookedUp.value.saved).toBe(false);
    expect(lookedUp.value.botId).toBeUndefined();

    const saved = await client.saveBotDraft({
      draftId: draft.draftId,
      revision: draft.revision,
    });
    expect(saved.runStarted).toBe(false);
    expect(saved.bot.name).toBe("Researcher");
    expect(saved.bot.tools).toEqual(["browser"]);
    expect(saved.draft.botId).toBe(saved.bot.botId);
    expect((await client.listCrew()).bots).toHaveLength(before + 1);

    const again = await client.saveBotDraft({
      draftId: draft.draftId,
      revision: draft.revision,
    });
    expect(again.bot.botId).toBe(saved.bot.botId);
    expect((await client.listCrew()).bots).toHaveLength(before + 1);

    const afterSave = await callTool<{ saved: boolean; botId?: string }>(
      server,
      "get_bot_draft",
      { draftId: draft.draftId },
    );
    expect(afterSave.value.saved).toBe(true);
    expect(afterSave.value.botId).toBe(saved.bot.botId);

    const standing = await client.botThread({ botId: saved.bot.botId });
    expect(standing.botId).toBe(saved.bot.botId);
    expect(standing.cwd).toBe(saved.bot.memoryDir);
  });

  it("rejects a bad bearer, unknown tools, a botless thread, and a revoked grant", async () => {
    const { host, client, server, chief } = await chiefWithBridge(
      dataDirWithFakeHarness(),
    );

    const guessed = await mcp(
      server,
      "tools/call",
      {
        name: "draft_bot",
        arguments: {
          requestKey: "x",
          name: "X",
          instructions: "Y",
        },
      },
      "Bearer not-the-token",
    );
    expect(guessed.status).toBe(401);

    const unknown = await callTool(server, "invent_a_bot", { name: "nope" });
    expect(unknown.ok).toBe(false);

    const forbidden = await callTool(server, "draft_bot", {
      requestKey: "clone",
      name: "Clone",
      instructions: "Copy me.",
      tools: ["draft_bot"],
    });
    expect(forbidden.ok).toBe(false);
    expect(forbidden.text).toContain("draft_bot");

    const folder = await client.openThread({
      threadId: "t-botless",
      title: "code",
      cwd: tmpdir(),
      harnessId: "fake-acp",
    });
    await client.prompt({ threadId: folder.threadId, content: "hi" });
    const botlessParams = await sessionNewParams(host, folder.threadId);
    expect(
      botlessParams.mcpServers.find((entry) => entry.name === "jabot"),
    ).toBeUndefined();

    await client.updateBot({
      botId: chief.botId,
      tools: [
        "handoff_to_bot",
        "spawn_code_session",
        "fold_thread",
        "list_crew_status",
      ],
    });
    const revoked = await callTool(server, "draft_bot", {
      requestKey: "after-revoke",
      name: "Late",
      instructions: "Too late.",
    });
    expect(revoked.ok).toBe(false);
    expect(revoked.text.toLowerCase()).toMatch(/not one of this bot's tools|draft_bot/);
  });

  it("recovers pending drafts after restart and will not Save a dismissed one", async () => {
    const dataDir = dataDirWithFakeHarness();
    const first = await chiefWithBridge(dataDir);
    const submitted = await callTool<{ draftId: string }>(
      first.server,
      "draft_bot",
      {
        requestKey: "keep-me",
        name: "Keeper",
        instructions: "Survive a restart.",
      },
    );
    await first.host.stop();
    running.splice(0);

    const second = await connected({ dataDir });
    const recovered = await second.client.listBotDrafts();
    const draft = recovered.drafts.find(
      (row) => row.draftId === submitted.value.draftId,
    );
    expect(draft?.name).toBe("Keeper");
    expect(draft?.status).toBe("pending_review");

    const dismissed = await second.client.dismissBotDraft({
      draftId: draft!.draftId,
      revision: draft!.revision,
    });
    expect(dismissed.status).toBe("dismissed");
    const refused = await second.host.call(CREW_DRAFT_SAVE, {
      draftId: draft!.draftId,
      revision: draft!.revision,
    });
    expect(refused.error?.code).toBe(RPC_ERROR.INVALID_PARAMS);
  });
});

describe("Recruiter ships a creation bridge", () => {
  it("gets jabot MCP servers because it now has draft_bot", async () => {
    const dataDir = dataDirWithFakeHarness();
    const { host, client } = await connected({ dataDir });
    const recruiter = named((await client.listCrew()).bots, "Bot Recruiter");
    await client.updateBot({ botId: recruiter.botId, harnessId: "fake-acp" });
    const thread = await client.botThread({ botId: recruiter.botId });
    await client.prompt({ threadId: thread.threadId, content: "hi" });
    const params = await sessionNewParams(host, thread.threadId);
    expect(params.mcpServers.map((server) => server.name)).toContain("jabot");
    expect(helloMethods(await client.hello())).toContain(CREW_DRAFTS);
  });
});

function helloMethods(hello: { methods: string[] }): string[] {
  return hello.methods;
}
