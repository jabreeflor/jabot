/**
 * End-to-end: the production `HostClient` talking to the production Rust host
 * over the production NDJSON protocol. No mocks on either side — the only
 * test-only piece is the pipe between them.
 *
 * This is the spine the rest of the build hangs off. Every issue that adds a
 * host method should add a case here, so "it works" means "it works across the
 * wire", not "the Rust unit test passes".
 */
import { afterEach, describe, expect, it } from "vitest";

import { HostClient, HostRpcError } from "../../src/host/client";
import {
  HOST_HELLO,
  PROTOCOL_VERSION,
  SESSION_PROMPT,
} from "../../src/host/protocol";
import { HostdProcess } from "../support/hostd";

const running: HostdProcess[] = [];

function startHost(options?: ConstructorParameters<typeof HostdProcess>[0]) {
  const host = new HostdProcess(options);
  running.push(host);
  return host;
}

async function connected(options?: ConstructorParameters<typeof HostdProcess>[0]) {
  const host = startHost(options);
  const client = new HostClient(host);
  await client.connect();
  const hello = await client.hello();
  return { host, client, hello };
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
});

describe("host handshake", () => {
  it("completes hello and advertises its method surface", async () => {
    const { hello } = await connected();

    expect(hello.protocolVersion).toBe(PROTOCOL_VERSION);
    expect(hello.hostMode).toBe("in-process");
    expect(hello.hostId).toMatch(/^[0-9a-f-]{36}$/);
    expect(hello.device.role).toBe("full");
    // Every method the TS client can call must be one the host admits to.
    expect(hello.methods).toContain(HOST_HELLO);
    expect(hello.methods).toContain(SESSION_PROMPT);
  });

  it("reports connected state through health after hello", async () => {
    const { client } = await connected();

    const health = await client.health();
    expect(health.connected).toBe(true);
    expect(health.hostMode).toBe("in-process");
    expect(health.protocolVersion).toBe(PROTOCOL_VERSION);
  });

  it("refuses calls made before hello", async () => {
    const host = startHost();
    const client = new HostClient(host);
    await client.connect();

    await expect(
      client.prompt({ threadId: "t1", content: "hi" }),
    ).rejects.toBeInstanceOf(HostRpcError);
  });

  it("refuses a device it has never paired with", async () => {
    const host = startHost();
    const client = new HostClient(host);

    await expect(
      client.hello({ device: { deviceId: "phone-that-never-paired" } }),
    ).rejects.toMatchObject({ name: "HostRpcError" });
  });

  it("refuses a protocol version it does not speak", async () => {
    const host = startHost();
    const client = new HostClient(host);

    await expect(client.hello({ protocolVersion: 99 })).rejects.toMatchObject({
      name: "HostRpcError",
    });
  });
});

describe("framing", () => {
  it("answers a malformed frame without desyncing the stream", async () => {
    const { host, client } = await connected();

    host.writeRaw("this is not json");
    // The stream must still be usable for the next real request.
    const health = await client.health();
    expect(health.connected).toBe(true);
  });

  it("handles several frames arriving in one chunk", async () => {
    const { host, client } = await connected();

    const [a, b] = await Promise.all([client.health(), client.health()]);
    expect(a.hostId).toBe(b.hostId);
    expect(host.notifications()).toBeDefined();
  });
});

describe("store", () => {
  it("opens SQLite in WAL with the seeded catalog when given a data dir", async () => {
    const { hello } = await connected({ persistent: true });

    expect(hello.storeError).toBeUndefined();
    expect(hello.store).toBeDefined();
    expect(hello.store?.journalMode).toBe("wal");
    expect(hello.store?.schemaVersion).toBeGreaterThanOrEqual(1);
    expect(hello.store?.harnessCount).toBeGreaterThan(0);
    expect(hello.store?.botCount).toBeGreaterThan(0);
  });

  it("keeps host identity stable across a restart of the same data dir", async () => {
    const first = await connected({ persistent: true });
    const dataDir = first.host.dataDir!;
    const firstHostId = first.hello.hostId;
    await first.host.stop();

    // Decision #4: Quit persists and the next launch resumes from disk.
    const second = await connected({ dataDir });
    expect(second.hello.hostId).toBe(firstHostId);
  });
});
