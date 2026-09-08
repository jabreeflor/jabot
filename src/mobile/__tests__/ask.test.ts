/**
 * Defensive parsing of an ACP permission ask (#20).
 *
 * The host never invents an option the agent did not offer, so the phone
 * has to drop what it cannot answer with and never substitute a decline
 * or a grant the agent did not list.
 */
import { describe, expect, it } from "vitest";

import {
  allowOption,
  askDetail,
  askTitle,
  parseAskOptions,
  rejectOption,
} from "../ask";
import type { PendingPermissionView } from "../../host/protocol";

function ask(over: Partial<PendingPermissionView> = {}): PendingPermissionView {
  return {
    requestId: "req-1",
    threadId: "t1",
    title: "Run ls",
    kind: "execute",
    subject: { title: "Run ls" },
    options: [],
    createdAt: "2026-08-20T11:00:00.000Z",
    stale: false,
    ...over,
  };
}

describe("parseAskOptions", () => {
  it("keeps the agent's order and drops anything without an id", () => {
    expect(parseAskOptions("not-an-array")).toEqual([]);
    expect(
      parseAskOptions([
        null,
        "skip",
        { name: "No id" },
        { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
        { id: "reject_once", label: "Reject", kind: "reject_once" },
        { optionId: "  ", name: "blank id" },
      ]),
    ).toEqual([
      { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
      { optionId: "reject_once", name: "Reject", kind: "reject_once" },
    ]);
  });

  it("falls back to the option id when the agent sent no name", () => {
    expect(parseAskOptions([{ optionId: "allow_always" }])).toEqual([
      { optionId: "allow_always", name: "allow_always", kind: undefined },
    ]);
  });
});

describe("rejectOption / allowOption", () => {
  it("picks a reject kind and prefers the narrowest allow", () => {
    const options = parseAskOptions([
      { optionId: "allow_always", name: "Always", kind: "allow_always" },
      { optionId: "allow_once", name: "Once", kind: "allow_once" },
      { optionId: "reject_once", name: "Reject", kind: "reject_once" },
    ]);
    expect(rejectOption(options)?.optionId).toBe("reject_once");
    expect(allowOption(options)?.optionId).toBe("allow_once");
  });

  it("returns undefined when the agent offered no matching kind", () => {
    const options = parseAskOptions([
      { optionId: "other", name: "Other", kind: "other" },
    ]);
    expect(rejectOption(options)).toBeUndefined();
    expect(allowOption(options)).toBeUndefined();
    expect(
      allowOption(parseAskOptions([{ optionId: "a", kind: "allow_session" }]))
        ?.optionId,
    ).toBe("a");
  });
});

describe("askTitle / askDetail", () => {
  it("falls back to a waiting headline when the subject has no title", () => {
    expect(askTitle(null)).toBe("waiting on your answer");
    expect(askTitle({ title: "  " })).toBe("waiting on your answer");
    expect(askTitle({ title: "Run ls" })).toBe("Run ls");
  });

  it("prefers a command, then paths, then rawInput", () => {
    expect(askDetail(ask({ subject: "not-an-object" }))).toBeUndefined();
    expect(askDetail(ask({ subject: { command: "ls -la" } }))).toBe("ls -la");
    expect(askDetail(ask({ subject: { description: "list files" } }))).toBe(
      "list files",
    );
    expect(
      askDetail(
        ask({
          subject: {
            locations: [{ path: "/etc/hosts" }, { path: "/tmp/out" }, "skip"],
          },
        }),
      ),
    ).toBe("/etc/hosts, /tmp/out");
    expect(askDetail(ask({ subject: { locations: [] } }))).toBeUndefined();
    expect(
      askDetail(ask({ subject: { rawInput: { command: "rm -rf /" } } })),
    ).toBe("rm -rf /");
  });
});
