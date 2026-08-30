/**
 * A bot's icon: the colour mark's fallbacks, and the upload that replaces it.
 *
 * The interesting half of an upload is everything that happens before the row
 * is written — a photo has to become a small square, a file that is not an
 * image has to say so, and "remove this picture" has to be distinguishable
 * from "I did not touch the picture". None of that is visible in the avatar's
 * own suite, which is handed a finished `data:` URL.
 *
 * The canvas is stubbed rather than the module: jsdom has no 2D context, so
 * the encode would throw wherever it ran, and stubbing `readBotImage` instead
 * would leave the crop arithmetic — the part that is actually easy to get
 * wrong — checked by nothing.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BotEditorModal } from "../components/BotEditorModal";
import {
  coverCrop,
  imageBytes,
  isBotImage,
  MAX_IMAGE_BYTES,
} from "../components/avatar/image";
import type { Bot } from "../components/types";
import { BOT_TEMPLATES, HARNESSES, TOOL_CATALOG } from "../views/mock-host";

const PIXEL =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
const WEBP = "data:image/webp;base64,AAAAAAAA";

const WRITER: Bot = {
  id: "writer",
  name: "Writer",
  color: "b-orange",
  instructions: "Draft in my voice: plain, short, no filler.",
  tools: ["gmail"],
  harnessId: "claude",
  isChief: false,
};

describe("what may be stored as an icon", () => {
  it("takes the three the encoder produces and nothing else", () => {
    expect(isBotImage(PIXEL)).toBe(true);
    expect(isBotImage("data:image/jpeg;base64,AAAA")).toBe(true);
    expect(isBotImage(WEBP)).toBe(true);

    // An SVG is a document with script in it, and the rest either fetch or do
    // not decode. This value ends up in a `src`.
    for (const bad of [
      "data:image/svg+xml;base64,PHN2Zy8+",
      "data:text/html;base64,PGI+",
      "http://example.com/a.png",
      "javascript:alert(1)",
      "data:image/png;base64,",
      "data:image/png;base64,not base64!",
      "",
    ]) {
      expect(isBotImage(bad), bad).toBe(false);
    }
  });

  it("measures the payload rather than the string", () => {
    // Base64 is a third bigger than what it carries, so measuring the whole
    // data URL would refuse icons a third smaller than the cap says.
    expect(imageBytes("data:image/png;base64,AAAA")).toBe(3);
    expect(imageBytes("data:image/png;base64,AAA=")).toBe(2);
    expect(imageBytes("data:image/png;base64,AA==")).toBe(1);
  });

  it("crops the largest centred square out of any shape", () => {
    expect(coverCrop(400, 200)).toEqual({ x: 100, y: 0, size: 200 });
    expect(coverCrop(200, 400)).toEqual({ x: 0, y: 100, size: 200 });
    expect(coverCrop(300, 300)).toEqual({ x: 0, y: 0, size: 300 });
  });
});

/** What `drawImage` was handed on the last upload. */
let drawn: number[] = [];
/** The encodings `toDataURL` was asked for, in order. */
let asked: string[] = [];
/** How big each encoding comes back, so the size loop can be driven. Six
    bytes is eight base64 characters, which is `WEBP` above. */
let encodedBytes = 6;

beforeEach(() => {
  drawn = [];
  asked = [];
  encodedBytes = 6;

  // A decode that always succeeds, on a landscape source so the crop has
  // something to do.
  vi.stubGlobal(
    "Image",
    class {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      naturalWidth = 400;
      naturalHeight = 200;
      width = 400;
      height = 200;
      set src(_value: string) {
        queueMicrotask(() => this.onload?.());
      }
    },
  );

  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    drawImage: (...args: number[]) => {
      drawn = args.slice(1);
    },
  } as unknown as CanvasRenderingContext2D);

  vi.spyOn(HTMLCanvasElement.prototype, "toDataURL").mockImplementation(
    (type?: string) => {
      asked.push(type ?? "image/png");
      // "A" is a valid base64 character, so what comes back is a data URL the
      // same checks accept — a stub that produced something `isBotImage`
      // rejects would hide a real mismatch between the two.
      return `data:${type};base64,${"A".repeat(Math.ceil(encodedBytes / 3) * 4)}`;
    },
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function renderEditor(
  over: Partial<Parameters<typeof BotEditorModal>[0]> = {},
) {
  const props = {
    bot: null,
    templates: BOT_TEMPLATES,
    tools: TOOL_CATALOG,
    harnesses: HARNESSES,
    onSave: vi.fn(),
    onRemove: vi.fn(),
    onCancel: vi.fn(),
    ...over,
  };
  render(<BotEditorModal {...props} />);
  return props;
}

/** The hidden input the visible button drives. */
function fileInput(): HTMLInputElement {
  return screen.getByLabelText("Upload an image") as HTMLInputElement;
}

async function upload(file: File) {
  fireEvent.change(fileInput(), { target: { files: [file] } });
  // The read and the encode are both async; the preview appears a tick later.
  await screen.findByRole("button", { name: "Remove image" });
}

const png = (name = "face.png") =>
  new File(["pretend pixels"], name, { type: "image/png" });

describe("uploading an icon", () => {
  it("shows the picture in place of the mark and saves it with the bot", async () => {
    const props = renderEditor({ bot: WRITER });

    // Before: the colour mark, drawn from the bot's initials.
    expect(document.querySelector(".iconpick .initials")).toHaveTextContent(
      "W",
    );

    await upload(png());

    const preview = document.querySelector(".iconpick img");
    expect(preview).toHaveAttribute("src", WEBP);
    expect(document.querySelector(".iconpick .initials")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(props.onSave).toHaveBeenCalledWith(
      expect.objectContaining({ image: WEBP }),
    );
  });

  it("centre-crops to a square and never scales a small image up", async () => {
    renderEditor({ bot: WRITER });
    await upload(png());

    // A 400x200 source: the middle 200x200, drawn into a 200px box rather
    // than blown up to the 256 the box allows.
    expect(drawn).toEqual([100, 0, 200, 200, 0, 0, 200, 200]);
  });

  it("tries the smaller encodings only when the first one will not fit", async () => {
    renderEditor({ bot: WRITER });
    await upload(png());
    // Small enough at the best quality, so nothing else is attempted.
    expect(asked).toEqual(["image/webp"]);

    encodedBytes = MAX_IMAGE_BYTES * 2;
    await userEvent.click(screen.getByRole("button", { name: "Remove image" }));
    fireEvent.change(fileInput(), { target: { files: [png("big.png")] } });
    expect(
      await screen.findByText(/too detailed to store/),
    ).toBeInTheDocument();
    // Every candidate was tried before giving up, and the user is told rather
    // than left with a picture that silently did not take.
    expect(asked.length).toBeGreaterThan(1);
  });

  it("says which file it could not use, and keeps the icon it had", async () => {
    renderEditor({ bot: { ...WRITER, image: PIXEL } });

    fireEvent.change(fileInput(), {
      target: {
        files: [new File(["hi"], "notes.txt", { type: "text/plain" })],
      },
    });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "notes.txt is not an image",
    );
    expect(document.querySelector(".iconpick img")).toHaveAttribute(
      "src",
      PIXEL,
    );
  });
});

describe("the icon a bot already has", () => {
  it("opens showing it, and Save alone does not disturb it", async () => {
    const props = renderEditor({ bot: { ...WRITER, image: PIXEL } });

    expect(document.querySelector(".iconpick img")).toHaveAttribute(
      "src",
      PIXEL,
    );

    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(props.onSave).toHaveBeenCalledWith(
      expect.objectContaining({ image: PIXEL }),
    );
  });

  it("goes back to the colour mark when it is removed", async () => {
    const props = renderEditor({ bot: { ...WRITER, image: PIXEL } });

    await userEvent.click(screen.getByRole("button", { name: "Remove image" }));
    expect(document.querySelector(".iconpick .initials")).toHaveTextContent(
      "W",
    );

    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    // `null` and not `undefined`: the host reads the two differently, and only
    // one of them means "take the picture away".
    expect(props.onSave).toHaveBeenCalledWith(
      expect.objectContaining({ image: null }),
    );
  });

  it("offers Replace rather than Upload once there is one", () => {
    renderEditor({ bot: { ...WRITER, image: PIXEL } });
    expect(
      screen.getByRole("button", { name: "Replace image" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Upload image" }),
    ).not.toBeInTheDocument();
  });
});
