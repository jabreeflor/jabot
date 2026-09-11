import { describe, expect, it } from "vitest";

import {
  jsonRecordFromLog,
  logHasPermissionReply,
  permissionReplyFromLog,
} from "./hostd";

describe("adapter permission-reply log records", () => {
  const complete =
    'permission_reply={"jsonrpc":"2.0","id":1,"result":{"outcome":{"outcome":"selected","optionId":"allow_once"}}}\n';

  it("ignores a prefix whose JSON has not flushed yet", () => {
    expect(
      permissionReplyFromLog('permission_reply={"jsonrpc"'),
    ).toBeUndefined();
    expect(logHasPermissionReply("permission_reply=", "allow_once")).toBe(
      false,
    );
  });

  it("reads the option id from a complete record", () => {
    expect(logHasPermissionReply(complete, "allow_once")).toBe(true);
    expect(logHasPermissionReply(complete, "reject_once")).toBe(false);
  });
});

describe("adapter json log records", () => {
  it("ignores a session_prompt prefix whose JSON has not flushed yet", () => {
    expect(
      jsonRecordFromLog(
        'session_prompt={"prompt":[{"type":"text"',
        "session_prompt=",
      ),
    ).toBeUndefined();
  });

  it("parses a complete session_prompt record", () => {
    const record = jsonRecordFromLog(
      'session_prompt={"prompt":[{"type":"text","text":"hi"}]}\n',
      "session_prompt=",
    );
    expect(record).toEqual({ prompt: [{ type: "text", text: "hi" }] });
  });
});
