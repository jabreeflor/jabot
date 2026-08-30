/**
 * End-to-end: app-wide preferences (#26).
 *
 * Three decision records parked a knob on a settings surface that did not
 * exist — the stuck backstop's threshold, a remembered permission scope, the
 * cron interval — and D-018 said plainly that naming #26 for it had been
 * optimistic. So the threshold was `JABOT_IDLE_TIMEOUT_MS` on the host
 * process, which a bundled Tauri app gives nobody.
 *
 * The load-bearing case here is the last one: a thread opened *after* the fold
 * default changes comes back from the host carrying it. A settings pane that
 * stored a preference nothing consulted would be a worse lie than no pane.
 *
 * Real `jabot-hostd`, real SQLite, the production `HostClient`.
 */
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import { HostRpcError } from "../../src/host/client";
import { HostdProcess } from "../support/hostd";

const running: HostdProcess[] = [];

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
});

async function connected(dataDir?: string) {
  const host = new HostdProcess(
    dataDir ? { dataDir } : { persistent: true },
  );
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  await client.hello();
  return { host, client };
}

function repository(): string {
  return mkdtempSync(path.join(tmpdir(), "jabot-settings-"));
}

describe("settings over the host protocol", () => {
  it("answers the shipped defaults on a fresh store", async () => {
    const { client } = await connected();

    const settings = await client.settings();

    // Ten minutes, which is `resurface.md`'s starting point for the backstop.
    expect(settings.idleTimeoutMs).toBe(600_000);
    expect(settings.defaultFoldPolicy).toBe("default");
    // Nothing in this test's environment sets the env var, so the control the
    // pane draws is the one deciding.
    expect(settings.idleTimeoutFromEnv).toBe(false);
  });

  it("persists a preference across a restart of the host", async () => {
    const dataDir = mkdtempSync(path.join(tmpdir(), "jabot-hostd-settings-"));
    const first = await connected(dataDir);

    const saved = await first.client.saveSettings({
      idleTimeoutMs: 90_000,
      defaultFoldPolicy: "wait_for_inbox",
    });
    // The whole view comes back, so the pane never merges a patch into a guess.
    expect(saved.idleTimeoutMs).toBe(90_000);
    expect(saved.defaultFoldPolicy).toBe("wait_for_inbox");

    await first.host.dispose();
    running.splice(running.indexOf(first.host), 1);

    // A second host on the same data dir: this is the trip that was missing,
    // because an env var does not survive a relaunch either.
    const second = await connected(dataDir);
    const after = await second.client.settings();
    expect(after.idleTimeoutMs).toBe(90_000);
    expect(after.defaultFoldPolicy).toBe("wait_for_inbox");
  });

  /**
   * Refused rather than clamped. A user who asked for a two-second backstop
   * and silently got ten minutes would have no way to find out, and a stored
   * fold policy the fold path rejects would break thread creation rather than
   * a pane.
   */
  it("refuses a value it cannot honour, and says which", async () => {
    const { client } = await connected();

    for (const bad of [0, 999, 86_400_001]) {
      await expect(client.saveSettings({ idleTimeoutMs: bad })).rejects.toThrow(
        /idleTimeoutMs/,
      );
    }
    await expect(
      client.saveSettings({
        defaultFoldPolicy: "whenever" as never,
      }),
    ).rejects.toBeInstanceOf(HostRpcError);

    // And nothing was written on the way to being refused.
    expect((await client.settings()).idleTimeoutMs).toBe(600_000);
  });

  /**
   * The whole point. A preference that no code path consults is a pane
   * pretending to do something.
   */
  it("gives a thread opened afterwards the stored fold default", async () => {
    const { client } = await connected();
    const folder = await client.registerFolder({ path: repository() });

    // Before: the shipped default, which is also the column default.
    const before = await client.openThread({
      threadId: "t-before",
      title: "Before",
      cwd: folder.cwd,
      harnessId: "claude",
      folderId: folder.folderId,
    });
    expect(before.foldPolicy).toBe("default");

    await client.saveSettings({ defaultFoldPolicy: "wait_for_inbox" });

    const after = await client.openThread({
      threadId: "t-after",
      title: "After",
      cwd: folder.cwd,
      harnessId: "claude",
      folderId: folder.folderId,
    });
    expect(after.foldPolicy).toBe("wait_for_inbox");
    // The thread opened before it keeps what it was opened with: this is a
    // default, not a policy applied retroactively to work already running.
    expect((await client.threadState({ threadId: "t-before" })).foldPolicy).toBe(
      "default",
    );
  });

  /** A caller that names a policy still wins — the stored value is what a
      thread starts as when nobody said. */
  it("lets an explicit policy beat the default", async () => {
    const { client } = await connected();
    const folder = await client.registerFolder({ path: repository() });
    await client.saveSettings({ defaultFoldPolicy: "wait_for_inbox" });

    const opened = await client.openThread({
      threadId: "t-explicit",
      title: "Explicit",
      cwd: folder.cwd,
      harnessId: "claude",
      folderId: folder.folderId,
      foldPolicy: "default",
    });

    expect(opened.foldPolicy).toBe("default");
  });

  /**
   * The env var wins, and the pane is told so rather than drawing a control
   * that decides nothing. Every timeout case in the e2e suite sets this on a
   * spawned host, so a stored value that beat it would make those tests wait
   * on a threshold they never wrote.
   */
  it("reports that the environment is in force, and keeps its value", async () => {
    const host = new HostdProcess({
      persistent: true,
      env: { JABOT_IDLE_TIMEOUT_MS: "1500" },
    });
    running.push(host);
    const client = new HostClient(host);
    await client.connect();
    await client.hello();

    expect(await client.settings()).toMatchObject({
      idleTimeoutMs: 1500,
      idleTimeoutFromEnv: true,
    });

    // A save still writes — the preference is for the next launch without the
    // variable — but it does not move the running host.
    const saved = await client.saveSettings({ idleTimeoutMs: 300_000 });
    expect(saved.idleTimeoutMs).toBe(1500);
    expect(saved.idleTimeoutFromEnv).toBe(true);
  });
});
