/**
 * The mark hands its bot to open-bot-avatar and keeps it in step: the right
 * definition and color for the persisted slot, the app's ring state in the
 * engine's vocabulary, and a clean teardown.
 */
import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const update = vi.fn();
const destroy = vi.fn();
const mountAvatar = vi.fn(() => ({ update, destroy }));
vi.mock("open-bot-avatar/dom", () => ({ mountAvatar }));

const { BotMark } = await import("../BotMark");
const { botIcon } = await import("../bots");

beforeEach(() => {
  update.mockClear();
  destroy.mockClear();
  mountAvatar.mockClear();
});

describe("BotMark", () => {
  it("mounts the slot's character, decorative, into its own span", () => {
    const { container } = render(<BotMark color="b-green" />);
    const host = container.querySelector(".bot-mark");
    const sprout = botIcon("b-green");
    expect(mountAvatar).toHaveBeenCalledTimes(1);
    expect(mountAvatar).toHaveBeenCalledWith(
      host,
      expect.objectContaining({
        definition: sprout.definition,
        color: sprout.color,
        state: "idle",
        label: "",
      }),
    );
  });

  it.each([
    ["running", "working"],
    ["waiting", "waiting"],
    ["failed", "blocked"],
  ] as const)("shows %s as the engine's %s", (state, engine) => {
    const { rerender } = render(<BotMark color="b-teal" />);
    rerender(<BotMark color="b-teal" state={state} />);
    expect(mountAvatar).toHaveBeenCalledTimes(1);
    expect(update).toHaveBeenLastCalledWith(
      expect.objectContaining({ state: engine }),
    );
  });

  it("swaps the character in place when the slot changes", () => {
    const { rerender, container } = render(<BotMark color="b-teal" />);
    rerender(<BotMark color="b-pink" />);
    expect(mountAvatar).toHaveBeenCalledTimes(1);
    expect(update).toHaveBeenLastCalledWith(
      expect.objectContaining({ definition: botIcon("b-pink").definition }),
    );
    expect(container.querySelector(".bot-mark")).toHaveAttribute(
      "data-character",
      "brain",
    );
  });

  it("stops animating when it leaves the page", () => {
    const { unmount } = render(<BotMark color="b-teal" />);
    unmount();
    expect(destroy).toHaveBeenCalledTimes(1);
  });
});
