/**
 * End-to-end: native notifications (#27) as far as a machine with no macOS and
 * no display can take them.
 *
 * Delivery itself cannot run here — `UNUserNotificationCenter` needs a signed
 * app bundle on a Mac — so this file pins the parts that *are* observable from
 * a client: the noise budget the host publishes, the payload a banner is built
 * from, and the promise that matters most, which is that none of it is load
 * bearing. A host that cannot notify at all must still put the card in the
 * Inbox; that is the persist-then-notify order of decision #5 seen from the
 * outside.
 */
import { tmpdir } from "node:os";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import {
  INBOX_RESURFACE,
  NOTIFY_STATUS,
  type InboxResurfaceParams,
  type JsonRpcNotification,
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

async function openThread(client: HostClient, threadId: string, mode?: string) {
  return client.openThread({
    threadId,
    title: "Auth migration",
    cwd: tmpdir(),
    harnessId: "claude",
    runtime: fakeAcpRuntime(mode),
  });
}

async function settle(
  client: HostClient,
  threadId: string,
  predicate: (state: Awaited<ReturnType<HostClient["threadState"]>>) => boolean,
  timeoutMs = 15_000,
) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const state = await client.threadState({ threadId });
    if (predicate(state)) return state;
    if (Date.now() > deadline) {
      throw new Error(`${threadId} never settled; last: ${JSON.stringify(state)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 30));
  }
}

describe("notify/status", () => {
  it("is advertised on the method surface", async () => {
    const { hello } = await connected({});
    expect(hello.methods).toContain(NOTIFY_STATUS);
  });

  /**
   * The noise budget, over the wire. `stuck` is the interesting absence: it is
   * a real Inbox kind that deliberately never rings, because the process behind
   * a stuck card is still working and the honest ask is patience.
   */
  it("publishes exactly the kinds that ring", async () => {
    const { client } = await connected({});

    const status = await client.notifyStatus();
    expect(status.kinds).toEqual(["needs_you", "done", "failed"]);
    expect(status.kinds).not.toContain("stuck");
    expect(status.kinds).not.toContain("folded");
  });

  /**
   * CI is Linux, so this is the "no Notification Center anywhere" answer. It
   * has to be `unsupported` rather than `denied`: denied is a permission a user
   * can go and change, unsupported is a platform with nowhere to put a banner,
   * and a settings screen that confuses the two sends people to a pane that
   * does not exist.
   */
  it("says unsupported rather than denied off macOS", async () => {
    const { client } = await connected({});

    const status = await client.notifyStatus();
    if (process.platform === "darwin") {
      expect(["granted", "denied", "notDetermined", "unsupported"]).toContain(
        status.authorization,
      );
    } else {
      expect(status.supported).toBe(false);
      expect(status.authorization).toBe("unsupported");
    }
  });

  it("refuses to answer before hello, like every other method", async () => {
    const host = new HostdProcess({});
    running.push(host);
    const client = new HostClient(host);
    await client.connect();

    await expect(client.notifyStatus()).rejects.toMatchObject({
      name: "HostRpcError",
    });
  });
});

describe("the card a banner is built from", () => {
  /**
   * A notification has to be able to name the thread it opens, and the only
   * place that copy exists is the `inbox_events` row the host just wrote. This
   * is the frame carrying it — the exact input `notify::plan` turns into a
   * title and a body.
   */
  it("carries the Inbox card's title and summary on the resurface", async () => {
    const { host, client } = await connected();
    await openThread(client, "t-notify");
    await client.prompt({ threadId: "t-notify", content: "hi" });
    await settle(client, "t-notify", (s) => s.latestRun?.state === "succeeded");

    const arrived = host.waitFor(
      (n: JsonRpcNotification) =>
        n.method === INBOX_RESURFACE &&
        (n.params as InboxResurfaceParams).threadId === "t-notify",
    );
    await client.fold({ threadId: "t-notify" });

    const params = (await arrived).params as InboxResurfaceParams;
    expect(params.reason).toBe("done");
    expect(params.title).toBe("Auth migration finished");
    expect(params.summary).toBeTruthy();

    // The same copy is on the durable row. Two sources that could disagree
    // would mean a banner that says something the Inbox does not.
    const inbox = await client.inbox();
    const card = inbox.events.find((event) => event.threadId === "t-notify");
    expect(card?.title).toBe(params.title);
    expect(card?.summary).toBe(params.summary);
  });

  /**
   * The whole point of persist-then-notify, stated from the client: this host
   * cannot deliver a single banner, and the card is there anyway. A refused OS
   * permission is the same shape of nothing.
   */
  it("keeps the card when nothing can be notified", async () => {
    const { client } = await connected();
    const status = await client.notifyStatus();

    await openThread(client, "t-degrade");
    await client.prompt({ threadId: "t-degrade", content: "hi" });
    await settle(client, "t-degrade", (s) => s.latestRun?.state === "succeeded");
    await client.fold({ threadId: "t-degrade" });

    const inbox = await client.inbox();
    const card = inbox.events.find((event) => event.threadId === "t-degrade");
    expect(card).toBeDefined();
    expect(card?.kind).toBe("done");
    // Stated rather than assumed: the assertion above held on a host whose
    // answer to "can you notify?" was no.
    if (process.platform !== "darwin") {
      expect(status.supported).toBe(false);
    }
  });
});
