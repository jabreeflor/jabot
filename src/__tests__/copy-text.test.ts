/**
 * Clipboard write (#267). The clipboard API is the happy path; a denied
 * permission or a missing API still has to try the textarea fallback, and
 * both failing is the error the button surfaces.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { copyText } from "../components/copyText";

const restore: Array<() => void> = [];

afterEach(() => {
  restore.splice(0).forEach((undo) => undo());
  vi.restoreAllMocks();
});

function stubClipboard(writeText: (() => Promise<void>) | undefined) {
  const original = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: writeText ? { writeText } : undefined,
  });
  restore.push(() => {
    if (original) Object.defineProperty(navigator, "clipboard", original);
    else delete (navigator as { clipboard?: unknown }).clipboard;
  });
}

function stubExecCommand(ok: boolean) {
  const original = Object.getOwnPropertyDescriptor(document, "execCommand");
  const exec = vi.fn().mockReturnValue(ok);
  Object.defineProperty(document, "execCommand", {
    configurable: true,
    value: exec,
  });
  restore.push(() => {
    if (original) Object.defineProperty(document, "execCommand", original);
    else delete (document as { execCommand?: unknown }).execCommand;
  });
  return exec;
}

describe("copyText", () => {
  it("writes through the clipboard API when it is there", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    stubClipboard(writeText);

    await copyText("**hello**\n```ts\nconst x = 1;\n```");

    expect(writeText).toHaveBeenCalledWith(
      "**hello**\n```ts\nconst x = 1;\n```",
    );
  });

  it("falls back to execCommand when the API is missing", async () => {
    stubClipboard(undefined);
    const exec = stubExecCommand(true);

    await copyText("line one\nline two");

    expect(exec).toHaveBeenCalledWith("copy");
  });

  it("falls back when the clipboard API refuses", async () => {
    stubClipboard(vi.fn().mockRejectedValue(new Error("denied")));
    const exec = stubExecCommand(true);

    await copyText("still this");

    expect(exec).toHaveBeenCalledWith("copy");
  });

  it("throws when neither path can write", async () => {
    stubClipboard(vi.fn().mockRejectedValue(new Error("denied")));
    stubExecCommand(false);

    await expect(copyText("nope")).rejects.toThrow(
      "Couldn't copy to the clipboard",
    );
  });

  it("throws when there is no clipboard API and no execCommand", async () => {
    stubClipboard(undefined);

    await expect(copyText("nope")).rejects.toThrow(
      "Couldn't copy to the clipboard",
    );
  });
});
