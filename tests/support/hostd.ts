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
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type {
  HostTransport,
  NotificationHandler,
} from "../../src/host/client";
import type {
  JsonRpcNotification,
  JsonRpcRequest,
  JsonRpcResponse,
} from "../../src/host/protocol";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

export function hostdBinaryPath(): string {
  const override = process.env.JABOT_HOSTD_BIN;
  if (override) return override;
  const exe = process.platform === "win32" ? "jabot-hostd.exe" : "jabot-hostd";
  return path.join(repoRoot, "src-tauri", "target", "debug", exe);
}

export interface HostdOptions {
  /** Open a real SQLite store + identity under a temp dir instead of running ephemeral. */
  persistent?: boolean;
  /** Reuse an existing data dir — lets a test restart the host and assert resume. */
  dataDir?: string;
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
  private buffer = "";
  private stderr = "";
  private exited = false;
  private ownsDataDir = false;

  readonly dataDir?: string;

  constructor(options: HostdOptions = {}) {
    const args: string[] = [];
    if (options.dataDir) {
      this.dataDir = options.dataDir;
    } else if (options.persistent) {
      this.dataDir = mkdtempSync(path.join(tmpdir(), "jabot-hostd-"));
      this.ownsDataDir = true;
    }
    if (this.dataDir) args.push("--data-dir", this.dataDir);

    this.child = spawn(hostdBinaryPath(), args, {
      stdio: ["pipe", "pipe", "pipe"],
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
