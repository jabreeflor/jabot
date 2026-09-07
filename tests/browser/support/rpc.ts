//! One JSON-RPC request against the Vite host bridge (`POST /__jabot/rpc`).

import { JSONRPC_VERSION } from "../../../src/host/protocol";

export interface HostStatus {
  running: boolean;
  pid: number | null;
  hello: { hostName: string; version: string; hostId: string } | null;
  binary: string;
  dataDir: string;
  requests: number;
  exit: { code: number | null; signal: string | null } | null;
  stderr: string[];
}

export async function hostStatus(baseUrl: string): Promise<HostStatus> {
  const response = await fetch(new URL("/__jabot/host", baseUrl));
  if (!response.ok) {
    throw new Error(`GET /__jabot/host → ${response.status}`);
  }
  return (await response.json()) as HostStatus;
}

export function hostIsReady(status: HostStatus): boolean {
  return status.running === true && status.hello != null;
}

export async function rpc<T = unknown>(
  baseUrl: string,
  method: string,
  params?: unknown,
  id: string | number = "browser",
): Promise<T> {
  const response = await fetch(new URL("/__jabot/rpc", baseUrl), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: JSONRPC_VERSION,
      id,
      method,
      ...(params === undefined ? {} : { params }),
    }),
  });
  const body = (await response.json()) as {
    result?: T;
    error?: { code: number; message: string };
  };
  if (body.error) {
    throw new Error(`rpc ${method}: ${body.error.message} (${body.error.code})`);
  }
  return body.result as T;
}

export async function waitForHost(
  baseUrl: string,
  timeoutMs = 60_000,
): Promise<HostStatus> {
  const deadline = Date.now() + timeoutMs;
  let last: HostStatus | Error | null = null;
  while (Date.now() < deadline) {
    try {
      const status = await hostStatus(baseUrl);
      last = status;
      if (hostIsReady(status)) return status;
    } catch (error) {
      last = error instanceof Error ? error : new Error(String(error));
    }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
  throw new Error(
    `host not ready at ${baseUrl} after ${timeoutMs}ms: ${
      last instanceof Error ? last.message : JSON.stringify(last)
    }`,
  );
}
