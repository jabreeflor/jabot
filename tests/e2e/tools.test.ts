/**
 * End-to-end: the tool / MCP framework (#18) over the wire.
 *
 * `src-tauri/src/host/tools` proves the OAuth flow against a local
 * authorization server, where a test can hold both ends. This file makes the
 * claims a renderer depends on — the catalog and its statuses, what a connect
 * actually opens, and, the important one, **which servers a session is spawned
 * with** — through the production `HostClient`, a live `jabot-hostd`, a real
 * SQLite store and a real ACP subprocess.
 *
 * Enforcement is asserted from the *agent's* side. The fake ACP agent echoes
 * the `session/new` params it received into its stderr log, so "the model never
 * sees a tool it may not call" is checked against the bytes that crossed the
 * wire rather than against a host-side accessor.
 */
import { tmpdir } from "node:os";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  RPC_ERROR,
  TOOLS_CONNECT,
  TOOLS_DISCONNECT,
  TOOLS_LIST,
  type ToolCardView,
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

const byId = (tools: ToolCardView[], id: string): ToolCardView => {
  const tool = tools.find((candidate) => candidate.id === id);
  if (!tool) throw new Error(`no ${id} in the catalog: ${tools.map((t) => t.id).join(", ")}`);
  return tool;
};

/** Spawn a real adapter for a thread, optionally owned by a seeded crew bot. */
async function promptOn(
  client: HostClient,
  threadId: string,
  botId?: string,
): Promise<void> {
  await client.openThread({
    threadId,
    title: botId ? `tools for ${botId}` : "no bot",
    cwd: tmpdir(),
    harnessId: "claude",
    botId,
    runtime: fakeAcpRuntime(),
  });
  await client.prompt({ threadId, content: "hi" });
}

/** The `session/new` params the adapter actually received. */
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

describe("tool catalog", () => {
  it("advertises its methods and lists every chip with a status", async () => {
    const { client, hello } = await connected();

    for (const method of [TOOLS_LIST, TOOLS_CONNECT, TOOLS_DISCONNECT]) {
      expect(hello.methods).toContain(method);
    }

    const { tools } = await client.listTools();
    expect(tools.map((tool) => tool.id)).toEqual([
      "gmail",
      "calendar",
      "drive",
      "github",
      "browser",
      "notion",
      "slack",
      "terminal",
    ]);

    // Nothing is signed in on a fresh store, and the three Google chips say so
    // through the one grant they share.
    for (const id of ["gmail", "calendar", "drive"]) {
      const tool = byId(tools, id);
      expect(tool.status).toBe("needs_auth");
      expect(tool.provider).toBe("google");
      expect(tool.mcp).toBe(true);
    }

    // Terminal is the harness's execute tool, not a server we could connect.
    const terminal = byId(tools, "terminal");
    expect(terminal.transport).toBe("harness_execute");
    expect(terminal.mcp).toBe(false);
    expect(terminal.provider).toBeUndefined();
    expect(terminal.status).toBe("connected");

    // Gmail asks for compose, never send: drafts are parked for the human.
    expect(byId(tools, "gmail").scopes.join(" ")).toContain("gmail.compose");
    expect(byId(tools, "gmail").scopes.join(" ")).not.toContain("gmail.send");
  });

  it("refuses to connect something that is not a remote MCP server", async () => {
    const { host } = await connected();

    for (const toolId of ["terminal", "browser"]) {
      const response = await host.call(TOOLS_CONNECT, { toolId });
      expect(response.error?.code).toBe(RPC_ERROR.INVALID_PARAMS);
    }
    const unknown = await host.call(TOOLS_CONNECT, { toolId: "everything" });
    expect(unknown.error?.message).toContain("Invalid params");
  });

  it("opens a loopback redirect for a connect and forgets it on disconnect", async () => {
    const { client } = await connected();

    const started = await client.connectTool({ toolId: "gmail" });
    expect(started.provider).toBe("google");
    expect(started.status).toBe("connecting");
    // RFC 8252: the redirect is a loopback listener on an ephemeral port, not
    // a hosted URL and not a custom scheme another app could claim.
    expect(started.redirectUri).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/callback$/);
    // One Google login covers all three Google chips, and the result says so
    // before the user wonders why Calendar lit up.
    expect(started.affects).toEqual(["gmail", "calendar", "drive"]);

    const listed = await client.listTools();
    expect(byId(listed.tools, "calendar").status).toBe("connecting");
    expect(byId(listed.tools, "notion").status).toBe("needs_auth");

    const disconnected = await client.disconnectTool({ toolId: "calendar" });
    expect(disconnected.provider).toBe("google");
    expect(disconnected.affects).toEqual(["gmail", "calendar", "drive"]);

    const after = await client.listTools();
    expect(byId(after.tools, "gmail").status).toBe("needs_auth");
  });

  /** No response on this surface may carry credential material. */
  it("never puts a token in a tool response", async () => {
    const { client, host } = await connected();
    await client.connectTool({ toolId: "notion" });
    const rendered = JSON.stringify(await client.listTools());

    expect(rendered).not.toMatch(/access[_-]?token/i);
    expect(rendered).not.toMatch(/refresh[_-]?token/i);
    expect(rendered).not.toMatch(/"authorization"/i);
    await client.disconnectTool({ toolId: "notion" });
    expect(host.notifications()).toBeDefined();
  });
});

describe("per-bot allowlist on session/new", () => {
  it("passes only the tools the bot allowlists", async () => {
    const { host, client } = await connected();

    // Research allowlists browser and notion. Notion is a remote server with
    // no grant, so it is left out; browser is local and needs none.
    await promptOn(client, "t-research", "rsrch");
    const params = await sessionNewParams(host, "t-research");
    expect(params.mcpServers.map((server) => server.name)).toEqual(["browser"]);
    expect(JSON.stringify(params.mcpServers)).not.toContain("notion");

    const browser = params.mcpServers[0] as { command: string; args: string[] };
    expect(browser.command).toContain("npx");
    // The persistent profile is JaBot-owned: the user's logged-in cookies are
    // not left in a shared Playwright default.
    expect(browser.args).toContain("--user-data-dir");
    expect(browser.args.join(" ")).toContain(host.dataDir!);
  });

  /**
   * The Code bot's chips are `github` and `terminal`. GitHub has no grant, and
   * Terminal is not a server at all — so a code session is spawned with an
   * empty array rather than with a shell behind a tool schema.
   */
  it("never turns Terminal into a server, and never passes an unconnected one", async () => {
    const { host, client } = await connected();

    await promptOn(client, "t-code", "code");
    const params = await sessionNewParams(host, "t-code");
    expect(params.mcpServers).toEqual([]);
    expect(params.cwd).toBeTruthy();
  });

  /** Deny by default: no bot, no tools. */
  it("gives a thread with no bot no servers at all", async () => {
    const { host, client } = await connected();

    await promptOn(client, "t-loose");

    const params = await sessionNewParams(host, "t-loose");
    expect(params.mcpServers).toEqual([]);
  });
});

describe("errors", () => {
  it("requires hello before the catalog", async () => {
    const host = new HostdProcess();
    running.push(host);
    const client = new HostClient(host);
    await client.connect();

    await expect(client.listTools()).rejects.toBeInstanceOf(HostRpcError);
  });

  /** Without a store there is nowhere to put a grant, so the flow is refused
      up front rather than after the user has signed in. */
  it("refuses to start a flow on a host with no store", async () => {
    const host = new HostdProcess();
    running.push(host);
    const client = new HostClient(host);
    await client.connect();
    await client.hello();

    const response = await host.call(TOOLS_CONNECT, { toolId: "gmail" });
    expect(response.error?.code).toBe(RPC_ERROR.STORE_UNAVAILABLE);
  });
});
