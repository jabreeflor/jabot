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
  it("goes back to the end when the reader sends", () => {
    const view = draw(history);
    view.scroll.scrollTop = 1_200;
    fireEvent.scroll(view.scroll);

    extend(view, [
      ...history,
      { kind: "user", id: "u1", text: "wait, go back" } as TranscriptItem,
    ]);

    expect(view.scroll.scrollTop).toBe(CONTENT);
  });

  /** And it stays stuck afterwards: the agent's reply follows the send. */
  it("keeps following after a send, chunk by chunk", () => {
    const view = draw(history);
    view.scroll.scrollTop = 1_200;
    fireEvent.scroll(view.scroll);
    const sent = [
      ...history,
      { kind: "user", id: "u1", text: "go on" } as TranscriptItem,
    ];
    extend(view, sent);

    view.scroll.scrollTop = 0;
    extend(view, [...sent, agent("a9", "typ")]);

    expect(view.scroll.scrollTop).toBe(CONTENT);
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
