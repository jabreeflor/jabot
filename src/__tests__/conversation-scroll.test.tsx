/**
 * End-anchored scrolling (#14).
 *
 * `Conversation` used to set `scrollTop = scrollHeight` on every change to
 * `items`, and #14's reducer rebuilds `items` on *every streamed chunk* — so
 * scrolling back through history while an agent was talking was impossible:
 * the view snapped to the bottom a few times a second. That is a live defect
 * anybody can hit, not a performance worry.
 *
 * jsdom reports zero for every layout number, so the geometry is stubbed on
 * the element. That is honest here: what is under test is the *rule* — when to
 * follow and when to hold — and the rule is arithmetic on three numbers the
 * browser supplies.
 */
import { render } from "@testing-library/react";
import { fireEvent } from "@testing-library/dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Conversation } from "../components/Conversation";
import type { TranscriptItem } from "../components/types";

const HEIGHT = 500;
const CONTENT = 5_000;

const restore: Array<() => void> = [];

afterEach(() => {
  restore.splice(0).forEach((undo) => undo());
  vi.restoreAllMocks();
});

/** A scroller with real geometry: 5000px of transcript in a 500px window. */
function measure(scroll: HTMLElement, contentHeight = CONTENT) {
  for (const [prop, value] of [
    ["scrollHeight", contentHeight],
    ["clientHeight", HEIGHT],
  ] as const) {
    const original = Object.getOwnPropertyDescriptor(scroll, prop);
    Object.defineProperty(scroll, prop, {
      configurable: true,
      get: () => value,
    });
    restore.push(() => {
      if (original) Object.defineProperty(scroll, prop, original);
      else delete (scroll as unknown as Record<string, unknown>)[prop];
    });
  }
}

const agent = (id: string, text: string): TranscriptItem => ({
  kind: "agent",
  id,
  text,
});

function draw(items: readonly TranscriptItem[]) {
  const view = render(
    <Conversation
      header={<div />}
      items={items}
      composerPlaceholder="Message"
      onSend={vi.fn()}
    />,
  );
  const scroll = view.container.querySelector(".chat-scroll") as HTMLElement;
  measure(scroll);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const top = this.matches(".msg.me") ? 4000 - scroll.scrollTop : 0;
    const bottom = this.matches(".transcript") ? 4100 - scroll.scrollTop : top;
    return { top, bottom, left: 0, right: 0, width: 0, height: bottom - top, x: 0, y: top, toJSON() {} };
  });
  return { ...view, scroll };
}

function extend(
  view: ReturnType<typeof draw>,
  items: readonly TranscriptItem[],
) {
  view.rerender(
    <Conversation
      header={<div />}
      items={items}
      composerPlaceholder="Message"
      onSend={vi.fn()}
    />,
  );
}

describe("Conversation scrolling", () => {
  const history = [agent("a1", "one"), agent("a2", "two")];

  it("follows the tail for a reader parked at the end", () => {
    const view = draw(history);
    // At the bottom: 5000 - 4500 - 500 = 0.
    view.scroll.scrollTop = CONTENT - HEIGHT;
    fireEvent.scroll(view.scroll);

    extend(view, [...history, agent("a3", "three")]);

    expect(view.scroll.scrollTop).toBe(CONTENT);
  });

  /**
   * The defect. A chunk arriving while somebody is reading history must not
   * move them — and because the reducer rebuilds `items` per chunk, the old
   * effect did exactly that several times a second.
   */
  it("holds the position of a reader who has scrolled up", () => {
    const view = draw(history);
    view.scroll.scrollTop = 1_200;
    fireEvent.scroll(view.scroll);

    extend(view, [...history, agent("a3", "three")]);

    expect(view.scroll.scrollTop).toBe(1_200);
  });

  /** Sub-pixel rounding, and a streaming bubble that grows between the scroll
      event and the read, both put the exact bottom a few pixels out of reach.
      A reader who never left must not be treated as having done so. */
  it("counts a few pixels short of the bottom as being at it", () => {
    const view = draw(history);
    view.scroll.scrollTop = CONTENT - HEIGHT - 20;
    fireEvent.scroll(view.scroll);

    extend(view, [...history, agent("a3", "three")]);

    expect(view.scroll.scrollTop).toBe(CONTENT);
  });

  /**
   * Sending re-sticks. Somebody who scrolled up to check something and then
   * typed is done reading back, and a reply that landed off-screen would be
   * the worse surprise.
   */
  it("aligns a new prompt at the top even when its reply arrives in the same update", () => {
    const view = draw(history);
    view.scroll.scrollTop = 1200;
    fireEvent.scroll(view.scroll);
    extend(view, [...history,
      { kind: "user", id: "u1", text: "go on" }, agent("a9", "reply"),
    ]);
    expect(view.scroll.scrollTop).toBe(4000);
    expect(view.container.querySelector<HTMLElement>(".turn-space")?.style.height).toBe("400px");
  });

  it("holds the prompt during streaming, including a short reply that fits", () => {
    const view = draw(history);
    const sent: TranscriptItem[] = [...history, { kind: "user", id: "u1", text: "go on" }];
    extend(view, sent);
    // Even a scroll event at the bottom of a short turn must not resume following.
    measure(view.scroll, 4500);
    fireEvent.scroll(view.scroll);
    extend(view, [...sent, agent("a9", "typing")]);
    expect(view.scroll.scrollTop).toBe(4000);
    view.scroll.scrollTop = 1200;
    fireEvent.scroll(view.scroll);
    extend(view, [...sent, agent("a9", "typing more")]);
    expect(view.scroll.scrollTop).toBe(1200);
  });

  describe("the way back", () => {
    it("is offered only while the view is deliberately held", () => {
      const view = draw(history);
      expect(view.container.querySelector(".jump-latest")).toBeNull();

      view.scroll.scrollTop = 1_200;
      fireEvent.scroll(view.scroll);

      expect(view.container.querySelector(".jump-latest")).not.toBeNull();
    });

    it("returns to the end and starts following again", () => {
      const view = draw(history);
      view.scroll.scrollTop = 1_200;
      fireEvent.scroll(view.scroll);

      fireEvent.click(
        view.container.querySelector(".jump-latest") as HTMLElement,
      );

      expect(view.scroll.scrollTop).toBe(CONTENT);
      expect(view.container.querySelector(".jump-latest")).toBeNull();
      // And the next chunk follows, which is the half a scrollTop alone would
      // not prove.
      extend(view, [...history, agent("a3", "three")]);
      expect(view.scroll.scrollTop).toBe(CONTENT);
    });
  });
});
