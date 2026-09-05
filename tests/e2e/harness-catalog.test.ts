/**
 * End-to-end: the harness catalog and Doctor (#13) through the production
 * client, the production wire, and a live Rust host.
 *
 * Two things here can only be proved end to end. The first is that a tier-3
 * JSON file on disk becomes a card the picker can offer *and* a row a thread
 * can be opened on — those are two subsystems that a unit test would have to
 * fake one of. The second is the GUI-launch PATH: a Finder-launched app gets
 * launchd's `PATH=/usr/bin:/bin`, so the only honest test is to start the host
 * with exactly that and check it still finds a harness installed the way a
 * person installs one.
 */
import { chmodSync, copyFileSync, mkdirSync, writeFileSync } from "node:fs";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { HostClient } from "../../src/host/client";
import type { HarnessReport } from "../../src/host/protocol";
import { fakeAcpAgentPath, HostdProcess } from "../support/hostd";

const running: HostdProcess[] = [];
const scratch: string[] = [];

interface CustomHarness {
  id: string;
  label: string;
  command: string;
  args?: string[];
  env?: Record<string, string>;
  installHint?: string;
  installInstructionsUrl?: string;
}

/** A data dir with tier-3 files already in place, as if the user wrote them. */
function dataDirWith(harnesses: CustomHarness[]): string {
  const dir = mkdtempSync(path.join(tmpdir(), "jabot-harness-"));
  scratch.push(dir);
  const custom = path.join(dir, "custom_harnesses");
  mkdirSync(custom, { recursive: true });
  for (const harness of harnesses) {
    writeFileSync(
      path.join(custom, `${harness.id}.json`),
      JSON.stringify(harness, null, 2),
    );
  }
  return dir;
}

/** Install the fake ACP agent the way a person does: a binary under `~/.local/bin`. */
function fakeHomeWithAgent(name: string): { home: string; binary: string } {
  const home = mkdtempSync(path.join(tmpdir(), "jabot-home-"));
  scratch.push(home);
  const bin = path.join(home, ".local", "bin");
  mkdirSync(bin, { recursive: true });
  const binary = path.join(bin, name);
  copyFileSync(fakeAcpAgentPath(), binary);
  chmodSync(binary, 0o755);
  return { home, binary };
}

async function connected(options?: ConstructorParameters<typeof HostdProcess>[0]) {
  const host = new HostdProcess(options);
  running.push(host);
  const client = new HostClient(host);
  await client.connect();
  const hello = await client.hello();
  return { host, client, hello };
}

function report(reports: HarnessReport[], id: string): HarnessReport {
  const found = reports.find((r) => r.id === id);
  if (!found) throw new Error(`no report for ${id}; saw ${reports.map((r) => r.id).join(", ")}`);
  return found;
}

afterEach(async () => {
  await Promise.all(running.splice(0).map((host) => host.dispose()));
  for (const dir of scratch.splice(0)) rmSync(dir, { recursive: true, force: true });
});

describe("harness/list", () => {
  it("advertises the method and returns all three tiers", async () => {
    const dataDir = dataDirWith([
      {
        id: "my-agent",
        label: "My Agent",
        command: "my-agent-bin",
        args: ["acp"],
        installHint: "Download from example.com",
        installInstructionsUrl: "https://example.com/docs",
      },
    ]);
    const { client, hello } = await connected({ dataDir });

    expect(hello.methods).toContain("harness/list");

    const { harnesses, issues } = await client.listHarnesses();
    expect(issues).toEqual([]);

    const byId = new Map(harnesses.map((card) => [card.id, card]));
    expect(byId.get("claude")).toMatchObject({
      tier: "shipped",
      label: "Claude Code",
      reserved: true,
      sessionScope: "thread",
    });
    // Hermes multiplexes chats onto one process per profile, and the catalog
    // is where that is written down (#13).
    expect(byId.get("hermes")).toMatchObject({
      tier: "preset",
      reserved: true,
      sessionScope: "profile",
    });
    expect(byId.get("my-agent")).toMatchObject({
      tier: "custom",
      label: "My Agent",
      command: "my-agent-bin",
      args: ["acp"],
      reserved: false,
      installUrl: "https://example.com/docs",
    });
  });

  it("refuses to let a user file shadow a reserved id, and says why", async () => {
    const dataDir = dataDirWith([
      { id: "claude", label: "Not Claude", command: "totally-not-claude" },
      { id: "leaky", label: "Leaky", command: "leaky-acp", env: { OPENAI_API_KEY: "sk-live" } },
      { id: "fine", label: "Fine", command: "fine-acp" },
    ]);
    const { client } = await connected({ dataDir });

    const { harnesses, issues } = await client.listHarnesses();

    const claude = harnesses.find((card) => card.id === "claude");
    expect(claude?.label).toBe("Claude Code");
    expect(claude?.tier).toBe("shipped");

    const reasons = issues.map((issue) => `${issue.file}: ${issue.reason}`).join("\n");
    expect(reasons).toMatch(/claude.*reserved|reserved/);
    // Credentials in a plaintext catalog file get the same answer the store
    // gives `runtime_json`: no.
    expect(reasons).toContain("OPENAI_API_KEY");
    // And one rejected file does not take the rest of the catalog with it.
    expect(harnesses.some((card) => card.id === "fine")).toBe(true);
    expect(harnesses.some((card) => card.id === "leaky")).toBe(false);
  });

  it("registers a custom harness as a row a thread can actually open on", async () => {
    const { home, binary } = fakeHomeWithAgent("jabot-custom-acp");
    const dataDir = dataDirWith([
      { id: "custom-acp", label: "Custom ACP", command: binary },
    ]);
    const { client } = await connected({ dataDir, env: { HOME: home } });

    // `threads.harness_id` is a foreign key: if the catalog had not written a
    // row, this call would fail on the constraint rather than open a thread.
    const thread = await client.openThread({
      threadId: "t-custom",
      title: "Custom harness thread",
      cwd: process.cwd(),
      harnessId: "custom-acp",
    });
    expect(thread.harnessId).toBe("custom-acp");

    await client.prompt({ threadId: "t-custom", content: "hello" });
    const state = await client.threadState({ threadId: "t-custom" });
    expect(state.process.connected).toBe(true);
  });
});

describe("harness/doctor", () => {
  it("gives every card a reason and shows the PATH it searched", async () => {
    const missingDir = mkdtempSync(path.join(tmpdir(), "jabot-missing-harness-"));
    scratch.push(missingDir);
    const dataDir = dataDirWith([{
      id: "missing-agent",
      label: "Missing test agent",
      command: path.join(missingDir, "not-installed"),
      installHint: "Install the test adapter.",
    }]);
    const { client } = await connected({ dataDir });

    const doctor = await client.harnessDoctor();

    expect(doctor.reports.length).toBeGreaterThanOrEqual(5);
    for (const entry of doctor.reports) {
      expect(entry.detail).not.toBe("");
      expect(entry.ready).toBe(entry.status === "ready");
    }
    // Probe a known absent binary instead of assuming the developer has no
    // vendor CLI installed. CLI-vs-adapter diagnosis is covered by unit tests.
    expect(report(doctor.reports, "missing-agent").status).toBe("adapter_missing");
    expect(report(doctor.reports, "missing-agent").installHint).toBe("Install the test adapter.");
    expect(report(doctor.reports, "codex").installHint).toBeTruthy();
    expect(doctor.path.length).toBeGreaterThan(0);
  });

  /**
   * The bug this prevents: a harness that works in Terminal and does not work
   * in the app. The host is started with launchd's PATH — no Homebrew, no
   * `~/.local/bin`, no nvm — and still has to find an agent installed there.
   */
  it("finds a harness that only the login shell's PATH would have", async () => {
    const { home, binary } = fakeHomeWithAgent("jabot-gui-path-acp");
    const dataDir = dataDirWith([
      { id: "gui-path", label: "GUI Path", command: "jabot-gui-path-acp" },
    ]);
    const { client } = await connected({
      dataDir,
      env: { HOME: home, PATH: "/usr/bin:/bin" },
    });

    const doctor = await client.harnessDoctor({ harnessId: "gui-path" });
    const entry = report(doctor.reports, "gui-path");

    expect(entry.status).toBe("ready");
    expect(entry.command).toBe(binary);
    expect(doctor.path).toContain(path.join(home, ".local/bin"));
  });

  /**
   * "Installed" says nothing about which protocol the adapter speaks. The deep
   * probe asks it, which is the only way to catch an outdated adapter before a
   * user's first prompt does.
   */
  it("tells a working adapter from one that speaks an older ACP", async () => {
    const { home } = fakeHomeWithAgent("jabot-deep-acp");
    const dataDir = dataDirWith([
      { id: "current", label: "Current", command: "jabot-deep-acp" },
      { id: "ancient", label: "Ancient", command: "jabot-deep-acp", args: ["old-acp"] },
    ]);
    const { client } = await connected({ dataDir, env: { HOME: home } });

    const shallow = await client.harnessDoctor();
    // Without the handshake both look identical: a binary is a binary.
    expect(report(shallow.reports, "current").status).toBe("ready");
    expect(report(shallow.reports, "ancient").status).toBe("ready");

    const deep = await client.harnessDoctor({ deep: true });
    expect(report(deep.reports, "current").status).toBe("ready");
    expect(report(deep.reports, "current").detail).toContain("ACP v1");

    const ancient = report(deep.reports, "ancient");
    expect(ancient.status).toBe("adapter_outdated");
    expect(ancient.remedy).toBeTruthy();
  });
});
