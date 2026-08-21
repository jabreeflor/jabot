/**
 * Test harness that runs the real Rust host and speaks the real protocol.
 *
 * `jabot-hostd` wraps the same `HostSession` the Tauri `host_rpc` command
 * wraps, framed with the same NDJSON codec a Unix socket will use. So an
 * assertion made here is an assertion about the shipping host, not a mock.
 *
 * The transport implements `HostTransport` from `src/host/client.ts`, which
 * means `HostClient` — the same client the renderer uses — can be pointed at
 * a live host process with no production code aware it is under test.
 */
import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type {
  HostTransport,
  NotificationHandler,
} from "../../src/host/client";
import {
  JSONRPC_VERSION,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type JsonRpcResponse,
  type RuntimeSpec,
} from "../../src/host/protocol";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

export function hostdBinaryPath(): string {
  const override = process.env.JABOT_HOSTD_BIN;
  if (override) return override;
  return cargoBinaryPath("jabot-hostd");
}

/**
 * The scriptable ACP agent from `src-tauri/src/bin/fake_acp_agent.rs`.
 *
 * `src-tauri/tests/acp_adapter.rs` reaches it through `CARGO_BIN_EXE_*`; from
 * TypeScript the equivalent is the same `target/debug` path. `cargo test` and
 * `cargo build --bins` both produce it, so a missing binary means a stale tree
 * rather than a broken test — say so instead of failing on `ENOENT` later.
 */
export function fakeAcpAgentPath(): string {
  const override = process.env.JABOT_FAKE_ACP_BIN;
  if (override) return override;
  const built = cargoBinaryPath("fake-acp-agent");
  if (!existsSync(built)) {
    throw new Error(
      `fake-acp-agent is not built at ${built}; run: cargo build --manifest-path src-tauri/Cargo.toml --bins`,
    );
  }
  return built;
}

/** Directory the Rust bins land in — also what a PATH-probe test prepends. */
export function cargoDebugDir(): string {
  return path.join(repoRoot, "src-tauri", "target", "debug");
}

function cargoBinaryPath(name: string): string {
  const exe = process.platform === "win32" ? `${name}.exe` : name;
  return path.join(cargoDebugDir(), exe);
}

/**
 * A `runtime` for `session/prompt` that spawns the fake agent by absolute path.
 *
 * `mode` is the agent's first argv — `echo` (default), `permission`, or
 * `grandchild`; see `fake_acp_agent.rs`.
 */
export function fakeAcpRuntime(mode?: string): RuntimeSpec {
  return {
    command: fakeAcpAgentPath(),
    args: mode ? [mode] : [],
  };
}

export interface HostdOptions {
  /** Open a real SQLite store + identity under a temp dir instead of running ephemeral. */
  persistent?: boolean;
  /** Reuse an existing data dir — lets a test restart the host and assert resume. */
  dataDir?: string;
  /** Extra environment for the host process; adapters inherit it when spawned. */
  env?: Record<string, string>;
  /**
   * Also listen on this Unix socket path (`--listen`), so a *second* client
   * can attach to the same host (#29).
   *
   * The binary binds the socket before it reads its first byte of stdio, so
   * any answer on stdio means the socket is already accepting — there is
   * nothing to poll for.
   */
  socket?: string;
}

/**
 * A live `jabot-hostd` process exposed as a `HostTransport`.
 *
 * Notifications are both dispatched to subscribers and buffered, so a test can
 * await one that may already have arrived before it started listening.
 */
export class HostdProcess implements HostTransport {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly pending = new Map<
    string,
    { resolve: (r: JsonRpcResponse) => void; reject: (e: Error) => void }
  >();
  private readonly handlers = new Set<NotificationHandler>();
  private readonly received: JsonRpcNotification[] = [];
  private readonly waiters: Array<{
    match: (n: JsonRpcNotification) => boolean;
    resolve: (n: JsonRpcNotification) => void;
  }> = [];
  private nextId = 1;
  private buffer = "";
  private stderr = "";
  private exited = false;
  private ownsDataDir = false;

  readonly dataDir?: string;
  readonly socketPath?: string;

  constructor(options: HostdOptions = {}) {
    const args: string[] = [];
    if (options.dataDir) {
      this.dataDir = options.dataDir;
    } else if (options.persistent) {
      this.dataDir = mkdtempSync(path.join(tmpdir(), "jabot-hostd-"));
      this.ownsDataDir = true;
    }
    if (this.dataDir) args.push("--data-dir", this.dataDir);
    if (options.socket) {
      this.socketPath = options.socket;
      args.push("--listen", options.socket);
    }

    this.child = spawn(hostdBinaryPath(), args, {
      stdio: ["pipe", "pipe", "pipe"],
      env: options.env ? { ...process.env, ...options.env } : process.env,
    });
    this.child.stdout.setEncoding("utf8");
    this.child.stderr.setEncoding("utf8");
    this.child.stdout.on("data", (chunk: string) => this.onStdout(chunk));
    this.child.stderr.on("data", (chunk: string) => {
      this.stderr += chunk;
    });
    this.child.on("exit", () => {
      this.exited = true;
      for (const [, waiter] of this.pending) {
        waiter.reject(new Error(`jabot-hostd exited early: ${this.stderr}`));
      }
      this.pending.clear();
    });
    this.child.on("error", (err) => {
      this.exited = true;
      for (const [, waiter] of this.pending) waiter.reject(err);
      this.pending.clear();
    });
  }

  private onStdout(chunk: string) {
    this.buffer += chunk;
    let newline: number;
    while ((newline = this.buffer.indexOf("\n")) >= 0) {
      const line = this.buffer.slice(0, newline).trim();
      this.buffer = this.buffer.slice(newline + 1);
      if (!line) continue;
      const message = JSON.parse(line) as JsonRpcResponse | JsonRpcNotification;
      if ("id" in message && message.id !== undefined) {
        const key = String((message as JsonRpcResponse).id);
        const pending = this.pending.get(key);
        this.pending.delete(key);
        pending?.resolve(message as JsonRpcResponse);
      } else {
        this.dispatch(message as JsonRpcNotification);
      }
    }
  }

  private dispatch(notification: JsonRpcNotification) {
    this.received.push(notification);
    for (const handler of this.handlers) handler(notification);
    for (let i = this.waiters.length - 1; i >= 0; i -= 1) {
      if (this.waiters[i].match(notification)) {
        this.waiters.splice(i, 1)[0].resolve(notification);
      }
    }
  }

  /** `HostTransport` — send a request, resolve on the matching `id`. */
  request(request: JsonRpcRequest): Promise<JsonRpcResponse> {
    if (this.exited) {
      return Promise.reject(new Error(`jabot-hostd is not running: ${this.stderr}`));
    }
    return new Promise((resolve, reject) => {
      this.pending.set(String(request.id), { resolve, reject });
      this.child.stdin.write(`${JSON.stringify(request)}\n`, (err?: Error | null) => {
        if (err) {
          this.pending.delete(String(request.id));
          reject(err);
        }
      });
    });
  }

  /** `HostTransport` — subscribe to host-initiated notifications. */
  async subscribe(handler: NotificationHandler): Promise<() => void> {
    this.handlers.add(handler);
    return () => {
      this.handlers.delete(handler);
    };
  }

  /**
   * One request, one response — including the error case.
   *
   * `HostClient` throws on `error` and drops the result of `prompt`, `cancel`
   * and `replyPermission`, so a test that asserts on either needs the frame
   * itself. Ids are string-tagged so they cannot collide with the numeric ones
   * a `HostClient` on the same transport is handing out.
   */
  call<T = unknown>(method: string, params?: unknown): Promise<JsonRpcResponse<T>> {
    const request: JsonRpcRequest = {
      jsonrpc: JSONRPC_VERSION,
      id: `harness-${this.nextId++}`,
      method,
    };
    if (params !== undefined) request.params = params;
    return this.request(request) as Promise<JsonRpcResponse<T>>;
  }

  /**
   * Where the host tees an adapter's stderr. The fake agent logs what it was
   * told and when, which is how a test observes ordering on the ACP side of
   * the host rather than only on the client side.
   */
  adapterLogPath(threadId: string): string {
    if (!this.dataDir) throw new Error("adapter logs need a data dir; start the host persistent");
    return path.join(this.dataDir, "adapter-logs", `${threadId}.stderr.log`);
  }

  readAdapterLog(threadId: string): string {
    const file = this.adapterLogPath(threadId);
    return existsSync(file) ? readFileSync(file, "utf8") : "";
  }

  /** Raw line write, for protocol-level tests (malformed frames, batching). */
  writeRaw(line: string) {
    this.child.stdin.write(line.endsWith("\n") ? line : `${line}\n`);
  }

  notifications(method?: string): JsonRpcNotification[] {
    return method
      ? this.received.filter((n) => n.method === method)
      : [...this.received];
  }

  /** Resolve as soon as a matching notification arrives — or has already. */
  waitFor(
    match: string | ((n: JsonRpcNotification) => boolean),
    timeoutMs = 10_000,
  ): Promise<JsonRpcNotification> {
    const predicate =
      typeof match === "string" ? (n: JsonRpcNotification) => n.method === match : match;
    const already = this.received.find(predicate);
    if (already) return Promise.resolve(already);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const index = this.waiters.findIndex((w) => w.resolve === wrapped);
        if (index >= 0) this.waiters.splice(index, 1);
        reject(
          new Error(
            `timed out waiting for notification; saw: ${this.received
              .map((n) => n.method)
              .join(", ")}`,
          ),
        );
      }, timeoutMs);
      const wrapped = (n: JsonRpcNotification) => {
        clearTimeout(timer);
        resolve(n);
      };
      this.waiters.push({ match: predicate, resolve: wrapped });
    });
  }

  /** Close stdin and wait for a clean exit, so SQLite checkpoints on the way out. */
  async stop(): Promise<void> {
    if (this.exited) return;
    await new Promise<void>((resolve) => {
      this.child.once("exit", () => resolve());
      this.child.stdin.end();
      setTimeout(() => {
        if (!this.exited) this.child.kill("SIGKILL");
      }, 5_000);
    });
  }

  /** Stop and remove the temp data dir, if this instance created one. */
  async dispose(): Promise<void> {
    await this.stop();
    if (this.ownsDataDir && this.dataDir) {
      rmSync(this.dataDir, { recursive: true, force: true });
    }
  }
}
