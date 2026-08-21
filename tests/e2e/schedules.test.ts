/**
 * End-to-end: recurring jobs, over the wire the app uses (#25).
 *
 * `host/schedule/` unit-tests the cron and the catch-up ruling, and
 * `src-tauri/tests/schedule.rs` drives a fire in-process. What only this file
 * can check is the pair of claims a user actually depends on, made through the
 * production `HostClient` against a live `jabot-hostd` with a real SQLite store
 * and a real ACP adapter subprocess:
 *
 * - a fire runs as a crew member on its standing thread (#24), opens a run of
 *   kind `schedule` on #15's ledger, and lands in `inbox/list` (#5);
 * - and a schedule whose time passed **while the host was not running** — a
 *   real quit, a real relaunch, the same data directory — produces exactly one
 *   catch-up run, however many occurrences it missed.
 *
 * The second one is the reason the whole module exists, and it cannot be
 * simulated: the host has to actually stop.
 */
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import {
  SCHEDULE_CREATE,
  SCHEDULE_LIST,
  SCHEDULE_REMOVE,
  SCHEDULE_RUN,
  SCHEDULE_UPDATE,
  type BotView,
  type ScheduleFireView,
  type ScheduleView,
} from "../../src/host/protocol";
import { fakeAcpAgentPath, HostdProcess } from "../support/hostd";

const running: HostdProcess[] = [];
const dataDirs: string[] = [];

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.stop()));
  for (const dir of dataDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

/**
 * A data directory whose crew can actually spawn: the scriptable ACP agent is
 * registered as a tier-3 harness (#13) before the host opens the store, because
 * `bots.harness_id` is a foreign key and the catalog is synced at load.
 *
 * Owned by the test rather than by `HostdProcess`, because both halves of the
 * catch-up case are the *same* directory across two host processes.
 */
function dataDirWithFakeHarness(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-schedule-"));
  dataDirs.push(dir);
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

async function connected(dataDir: string, env?: Record<string, string>) {
  const host = new HostdProcess({ dataDir, env });
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

/** Poll `schedule/list` until `predicate` holds, or explain what it saw. */
async function until(
  client: HostClient,
  scheduleId: string,
  predicate: (row: ScheduleView) => boolean,
  timeoutMs = 20_000,
): Promise<ScheduleView> {
  const deadline = Date.now() + timeoutMs;
  let last: ScheduleView | undefined;
  for (;;) {
    const listed = await client.listSchedules();
    last = listed.schedules.find((row) => row.scheduleId === scheduleId);
    if (last && predicate(last)) return last;
    if (Date.now() > deadline) {
      throw new Error(`schedule never settled; last: ${JSON.stringify(last)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

const caughtUp = (fires: readonly ScheduleFireView[]) =>
  fires.filter((fire) => fire.caughtUp);

describe("schedules on the wire", () => {
  it("advertises its methods and refuses a cron that could never run", async () => {
    const { client, hello } = await connected(dataDirWithFakeHarness());
    for (const method of [
      SCHEDULE_LIST,
      SCHEDULE_CREATE,
      SCHEDULE_UPDATE,
      SCHEDULE_REMOVE,
      SCHEDULE_RUN,
    ]) {
      expect(hello.methods).toContain(method);
    }
    expect((await client.listSchedules()).schedules).toEqual([]);

    const writer = named((await client.listCrew()).bots, "Writer");
    await expect(
      client.createSchedule({
        botId: writer.botId,
        name: "Nightly",
        cron: "0 99 * * *",
        prompt: "go",
      }),
    ).rejects.toThrow(/hour/);
    // Refused means refused: no half-written schedule to find later.
    expect((await client.listSchedules()).schedules).toEqual([]);
  });

  it("runs as the bot, on its standing thread, and delivers to the Inbox", async () => {
    const dataDir = dataDirWithFakeHarness();
    const { client } = await connected(dataDir);
    const writer = named((await client.listCrew()).bots, "Writer");
    await client.updateBot({ botId: writer.botId, harnessId: "fake-acp" });

    const created = await client.createSchedule({
      botId: writer.botId,
      name: "Morning triage",
      cron: "0 9 * * 1-5",
      prompt: "Summarise overnight mail.",
    });
    expect(created.botName).toBe("Writer");
    expect(created.catchUp).toBe("once");
    // Armed from now: a schedule made at 10am does not owe this morning's 9am.
    expect(new Date(created.nextRunAt!).getTime()).toBeGreaterThan(Date.now());

    const run = await client.runSchedule({ scheduleId: created.scheduleId });
    // Decision #6: a worker's work happens on its one standing thread (#24).
    expect(run.fire.threadId).toBe(`bot-${writer.botId}`);

    const settled = await until(
      client,
      created.scheduleId,
      (row) => row.lastFire?.state === "delivered",
    );
    // Run now is its own occurrence and must not have consumed 9am.
    expect(settled.nextRunAt).toBe(created.nextRunAt);

    // #15's ledger, with the kind the schema has always accepted and nothing
    // had ever written.
    const thread = await client.threadState({ threadId: run.fire.threadId! });
    const scheduled = thread.runs.filter((entry) => entry.kind === "schedule");
    expect(scheduled).toHaveLength(1);
    expect(scheduled[0].state).toBe("succeeded");
    expect(settled.lastFire?.runId).toBe(scheduled[0].id);

    // …and decision #5's projection: the card, carrying the schedule's name
    // rather than the thread's, on a thread that never left the sidebar.
    const inbox = await client.inbox({ limit: 50 });
    const card = inbox.events.find(
      (event) => event.runId === scheduled[0].id,
    );
    expect(card).toBeDefined();
    expect(card!.kind).toBe("done");
    expect(card!.title).toContain("Morning triage");
    expect(card!.payload).toMatchObject({
      source: "schedule",
      scheduleId: created.scheduleId,
    });
    expect(thread.state).toBe("active");
  });

  /**
   * The failure the issue names, with a host that really stops.
   *
   * A two-second schedule stands in for a daily one: what is being tested is
   * the *ruling*, and the ruling only depends on how many occurrences went by
   * while nothing was running.
   */
  it("collapses a backlog accrued while the host was down to one run", async () => {
    const dataDir = dataDirWithFakeHarness();
    const first = await connected(dataDir);
    const writer = named((await first.client.listCrew()).bots, "Writer");
    await first.client.updateBot({ botId: writer.botId, harnessId: "fake-acp" });
    const created = await first.client.createSchedule({
      botId: writer.botId,
      name: "Frequent sweep",
      cron: "*/2 * * * * *",
      prompt: "Check the wire.",
    });

    // Quit. Decision #4: no launchd, no daemon — nothing is running now, and
    // the occurrences keep coming due on paper.
    await running.splice(0)[0].stop();
    await new Promise((resolve) => setTimeout(resolve, 8_000));

    const second = await connected(dataDir);
    const back = await until(
      second.client,
      created.scheduleId,
      (row) => caughtUp(row.recentFires).length > 0,
    );
    // Stop the clock before asserting, so the on-time fires that follow cannot
    // race the assertions below.
    await second.client.updateSchedule({
      scheduleId: created.scheduleId,
      enabled: false,
    });

    const catchUps = caughtUp(back.recentFires);
    expect(catchUps).toHaveLength(1);
    // Four occurrences went by; one ran and the rest were dropped, rather than
    // four agents starting at once on a laptop that just woke up.
    expect(catchUps[0].skippedCount).toBeGreaterThanOrEqual(2);
    expect(catchUps[0].detail).toMatch(/skipped/);

    const parked = await second.client.listSchedules();
    const row = parked.schedules.find(
      (candidate) => candidate.scheduleId === created.scheduleId,
    )!;
    expect(row.enabled).toBe(false);
    expect(row.nextRunAt).toBeUndefined();
  }, 60_000);

  it("survives a restart, and a removed schedule stays removed", async () => {
    const dataDir = dataDirWithFakeHarness();
    const { client } = await connected(dataDir);
    const writer = named((await client.listCrew()).bots, "Writer");
    const created = await client.createSchedule({
      botId: writer.botId,
      name: "Morning triage",
      cron: "0 9 * * 1-5",
      prompt: "Summarise overnight mail.",
      catchUp: "skip",
    });
    await running.splice(0)[0].stop();

    const second = await connected(dataDir);
    const listed = await second.client.listSchedules();
    expect(listed.schedules).toHaveLength(1);
    expect(listed.schedules[0]).toMatchObject({
      scheduleId: created.scheduleId,
      name: "Morning triage",
      catchUp: "skip",
      cron: "0 9 * * 1-5",
    });

    const removed = await second.client.removeSchedule({
      scheduleId: created.scheduleId,
    });
    expect(removed.removed).toBe(true);
    await running.splice(0)[0].stop();

    const third = await connected(dataDir);
    expect((await third.client.listSchedules()).schedules).toEqual([]);
  }, 30_000);
});
