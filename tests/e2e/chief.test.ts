/**
 * End-to-end: Chief's host tools, over the wire the agent actually uses (#24).
 *
 * `src-tauri/src/host/chief/` proves the rules in-process. This file makes the
 * claims a running Chief depends on, through the production `HostClient`, a
 * live `jabot-hostd`, a real SQLite store, a real ACP adapter subprocess and a
 * real HTTP client:
 *
 * - Chief's session is handed a host-implemented MCP server — the ids #17
 *   ships as chips, reachable as tools, and nothing else;
 * - **the test then behaves like the agent**: it takes the endpoint and the
 *   bearer out of the `session/new` the adapter received and calls the tools
 *   itself. That is the only place "the host implements these tools" can be
 *   checked honestly, because everything else only proves a row changed;
 * - a handoff opens the receiving bot's standing thread, prompts it, and the
 *   thread records where the work came from;
 * - a code session gets its own worktree and branch (#23) in a registered
 *   folder (#16) — decision #6's "a worker gets a repo cwd no other way";
 * - and the port is useless to anything that does not have the token.
 */
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import { CREW_THREAD, type BotView } from "../../src/host/protocol";
import { fakeAcpAgentPath, HostdProcess, type HostdOptions } from "../support/hostd";

const running: HostdProcess[] = [];

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
});

/**
 * A tier-3 harness (#13) that is the scriptable ACP agent.
 *
 * Written before the host starts, because the catalog is synced at load and
 * `bots.harness_id` is a foreign key. This is what lets a crew member actually
 * spawn on a machine with no `claude` on it — without it every dispatch in
 * this file would be the "recorded but nobody heard it" path, and the half
 * that matters most would go untested.
 */
function dataDirWithFakeHarness(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-chief-"));
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

/** The `session/new` params the adapter received — the agent's side of the wire. */
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

interface HttpMcpServer {
  type: string;
  name: string;
  url: string;
  headers: Array<{ name: string; value: string }>;
}

/** Just enough of MCP to assert on, typed rather than cast away. */
interface JsonRpcAnswer {
  result?: Record<string, unknown>;
  error?: { code: number; message: string };
}

interface McpToolResult {
  isError?: boolean;
  content?: Array<{ type: string; text?: string }>;
  structuredContent?: Record<string, unknown>;
}

interface CrewStatus {
  crew: Array<{
    botId: string;
    name: string;
    idle: boolean;
    threads: Array<{ threadId: string; state: string }>;
  }>;
}

interface HandoffResult {
  handoffId: string;
  bot: string;
  threadId: string;
  dispatched: boolean;
}

interface SpawnResult {
  handoffId: string;
  threadId: string;
  folderId: string;
}

interface FoldResult {
  threadId: string;
  state: string;
  foldPolicy: string;
}

/** Bring Chief up on the fake adapter and hand back its host-tool server. */
async function chiefAtWork(dataDir: string) {
  const { host, client } = await connected({ dataDir });
  const chief = named((await client.listCrew()).bots, "Chief");
  await client.updateBot({ botId: chief.botId, harnessId: "fake-acp" });
  const thread = await client.botThread({ botId: chief.botId });
  await client.prompt({ threadId: thread.threadId, content: "who is free?" });

  const params = await sessionNewParams(host, thread.threadId);
  const server = params.mcpServers.find(
    (entry) => entry.name === "jabot",
  ) as unknown as HttpMcpServer | undefined;
  if (!server) {
    throw new Error(`no host tool server on session/new: ${JSON.stringify(params.mcpServers)}`);
  }
  return { host, client, chief, thread, server, params };
}

let nextId = 1;

/** One MCP call, as the adapter's MCP client would make it. */
async function mcp(
  server: HttpMcpServer,
  method: string,
  params: unknown = {},
  bearer?: string,
): Promise<{ status: number; body: JsonRpcAnswer }> {
  const authorization =
    bearer ?? server.headers.find((header) => header.name === "Authorization")!.value;
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
  // A refusal is plain text; only a JSON-RPC answer is JSON.
  const isJson = response.headers.get("content-type")?.startsWith("application/json") ?? false;
  return {
    status: response.status,
    body: isJson && text ? (JSON.parse(text) as JsonRpcAnswer) : {},
  };
}

/** `tools/call`, unwrapped to the structured result or the refusal text. */
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

function git(cwd: string, ...args: string[]): string {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_SYSTEM: "/dev/null" },
  }).trim();
}

function repository(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-chief-repo-"));
  git(dir, "init", "--initial-branch=main");
  git(dir, "config", "user.email", "test@example.com");
  git(dir, "config", "user.name", "Test");
  writeFileSync(path.join(dir, "README.md"), "# project\n");
  git(dir, "add", "-A");
  git(dir, "commit", "-m", "first");
  return dir;
}

describe("a bot's standing thread", () => {
  it("is one thread, in the bot's memory directory, with no worktree", async () => {
    const dataDir = dataDirWithFakeHarness();
    const { client, hello } = await connected({ dataDir });
    expect(hello.methods).toContain(CREW_THREAD);

    const writer = named((await client.listCrew()).bots, "Writer");
    const thread = await client.botThread({ botId: writer.botId });

    // Decision #6: `cwd` is the bot's memory/workspace directory, and a worker
    // does not get a git worktree.
    expect(thread.cwd).toBe(writer.memoryDir);
    expect(thread.botId).toBe(writer.botId);
    expect(thread.worktreePath).toBeUndefined();
    expect(thread.repo).toBeUndefined();

    // One thread, not one per call — and the same one after a restart, which
    // is what "standing" has to mean for a bot's memory to be worth anything.
    expect((await client.botThread({ botId: writer.botId })).threadId).toBe(thread.threadId);
    await running.splice(0)[0].stop();

    const second = await connected({ dataDir });
    const again = await second.client.botThread({ botId: writer.botId });
    expect(again.threadId).toBe(thread.threadId);
    expect(again.cwd).toBe(thread.cwd);
  });
});

describe("Chief's host tools reach the session", () => {
  it("arrive as a host-implemented MCP server, carrying exactly Chief's chips", async () => {
    const { chief, server } = await chiefAtWork(dataDirWithFakeHarness());

    // Loopback, ephemeral port, bearer token — see `chief/bridge.rs`.
    expect(server.type).toBe("http");
    expect(server.url).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/mcp$/);
    expect(server.headers[0].name).toBe("Authorization");

    const initialized = await mcp(server, "initialize", {});
    const info = initialized.body.result?.serverInfo as { name: string } | undefined;
    expect(info?.name).toBe("jabot");

    const listed = await mcp(server, "tools/list");
    const tools = (listed.body.result?.tools ?? []) as Array<{ name: string }>;
    const names = tools.map((tool) => tool.name);
    // Exactly the ids #17 compiled in as Chief's chips — no more, no fewer.
    expect(names).toEqual(chief.tools);
    expect(names).toEqual([
      "handoff_to_bot",
      "spawn_code_session",
      "fold_thread",
      "list_crew_status",
    ]);
  });

  it("is useless to anything on this machine that does not have the token", async () => {
    const { server } = await chiefAtWork(dataDirWithFakeHarness());

    const guessed = await mcp(server, "tools/list", {}, "Bearer not-the-token");
    expect(guessed.status).toBe(401);

    const none = await fetch(server.url, { method: "POST", body: "{}" });
    expect(none.status).toBe(401);
  });
});

describe("handoff_to_bot", () => {
  it("opens the receiving bot's thread, prompts it, and records where the work came from", async () => {
    const dataDir = dataDirWithFakeHarness();
    const { host, client, thread: chiefThread, server } = await chiefAtWork(dataDir);
    const writer = named((await client.listCrew()).bots, "Writer");
    await client.updateBot({ botId: writer.botId, harnessId: "fake-acp" });

    const handed = await callTool<HandoffResult>(server, "handoff_to_bot", {
      bot: "Writer",
      task: "Draft the launch note",
      context: "Plain, short, no exclamation marks",
    });
    expect(handed.ok, handed.text).toBe(true);
    expect(handed.value.bot).toBe("Writer");
    expect(handed.value.dispatched).toBe(true);

    // The receiving thread is Writer's standing thread, in Writer's own
    // directory — a worker with no repo, exactly as decision #6 has it.
    const received = await client.threadState({ threadId: handed.value.threadId });
    expect(received.botId).toBe(writer.botId);
    expect(received.cwd).toBe(writer.memoryDir);
    expect(received.worktreePath).toBeUndefined();

    // Traceable: the thread says who asked and what for.
    expect(received.handoff?.kind).toBe("handoff");
    expect(received.handoff?.task).toBe("Draft the launch note");
    expect(received.handoff?.fromBotName).toBe("Chief");
    expect(received.handoff?.fromThreadId).toBe(chiefThread.threadId);
    expect(received.handoff?.dispatched).toBe(true);

    // And an agent really was started and really was told: a `session/new`
    // reached Writer's adapter, and the task is in Writer's transcript.
    await sessionNewParams(host, received.threadId);
    const transcript = await client.threadTranscript({ threadId: received.threadId });
    const said = JSON.stringify(transcript.events);
    expect(said).toContain("Draft the launch note");
    expect(said).toContain("Plain, short, no exclamation marks");

    // Chief's own thread is untouched by the handoff — routing is a host
    // action, not a nested conversation.
    const chiefState = await client.threadState({ threadId: chiefThread.threadId });
    expect(chiefState.handoff).toBeUndefined();
  });

  it("names the crew when asked for a bot nobody has, and writes nothing", async () => {
    const { server } = await chiefAtWork(dataDirWithFakeHarness());

    const refused = await callTool<Record<string, never>>(server, "handoff_to_bot", {
      bot: "Gardener",
      task: "water the plants",
    });
    // A refusal has to come back as a readable tool result, not a transport
    // error the model never sees.
    expect(refused.ok).toBe(false);
    expect(refused.text).toContain("Gardener");
    expect(refused.text).toContain("Inbox Mgr");

    const status = await callTool<CrewStatus>(server, "list_crew_status");
    const working = status.value.crew.filter((bot) => !bot.idle).map((bot) => bot.name);
    expect(working).toEqual(["Chief"]);
  });
});

describe("spawn_code_session", () => {
  it("is how a worker gets a repo: its own worktree, on its own branch", async () => {
    const repo = repository();
    const { client, server } = await chiefAtWork(dataDirWithFakeHarness());
    const folder = await client.registerFolder({ path: repo, name: "Project" });

    const spawned = await callTool<SpawnResult>(server, "spawn_code_session", {
      folder: "Project",
      task: "Add a --version flag",
    });
    expect(spawned.ok, spawned.text).toBe(true);
    expect(spawned.value.folderId).toBe(folder.folderId);

    const thread = await client.threadState({ threadId: spawned.value.threadId });
    expect(thread.title).toBe("Add a --version flag");
    expect(thread.folderId).toBe(folder.folderId);
    // #23's worktree, not the folder the user has open in their editor.
    expect(thread.worktreePath).toBeDefined();
    expect(thread.cwd).toBe(thread.worktreePath);
    expect(thread.cwd.startsWith(repo)).toBe(false);
    expect(thread.branch).toMatch(/^jabot\//);
    // git agrees, which is the only opinion that counts.
    expect(git(thread.worktreePath!, "branch", "--show-current")).toBe(thread.branch);
    // …and the trail says Chief started it, not the user.
    expect(thread.handoff?.kind).toBe("code_session");
    expect(thread.handoff?.fromBotName).toBe("Chief");

    // Folding it is the other half of the gesture: the job disappears from the
    // sidebar and stays on the roster, so Chief does not double-book the bot.
    const folded = await callTool<FoldResult>(server, "fold_thread", {
      threadId: thread.threadId,
      policy: "wait_for_inbox",
    });
    expect(folded.value.state).toBe("folded");
    expect(folded.value.foldPolicy).toBe("wait_for_inbox");
    expect((await client.inbox()).sleeping.map((row) => row.threadId)).toContain(
      thread.threadId,
    );
    const status = await callTool<CrewStatus>(server, "list_crew_status");
    const code = status.value.crew.find((bot) => bot.name === "Code");
    expect(code?.idle).toBe(false);
    expect(code?.threads[0].state).toBe("folded");
  });

  it("says which folders exist when asked for one that does not", async () => {
    const { client, server } = await chiefAtWork(dataDirWithFakeHarness());
    await client.registerFolder({ path: repository(), name: "Project" });

    const refused = await callTool<Record<string, never>>(server, "spawn_code_session", {
      folder: "Elsewhere",
      task: "fix it",
    });
    expect(refused.ok).toBe(false);
    expect(refused.text).toContain("Project");
  });
});
