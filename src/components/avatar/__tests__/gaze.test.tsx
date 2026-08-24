/**
 * The watchers' gaze: two numbers on the document root, shared by every eye.
 *
 * The arithmetic is deliberately not per-avatar. A room of characters all turn
 * to look at the same thing rather than each staring at you from its own
 * angle, and the version that measured each avatar's own position would have
 * been a layout read per eye per pointer event on a surface that is meant to
 * sit idle. That makes the listeners a module-level resource with a mount
 * count, which is the part that can leak: install twice and the easing runs
 * twice per event, forget to remove and a closed pane keeps moving eyes that
 * are no longer on screen.
 */
import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Avatar } from "../Avatar";
import { Watchers } from "../Watchers";

afterEach(() => {
  vi.restoreAllMocks();
  document.documentElement.removeAttribute("style");
});

const BOT = { name: "Mira", color: "b-teal" } as const;

/** How many listeners of a kind the window is currently carrying. */
function counts(spy: ReturnType<typeof vi.spyOn>) {
  return (type: string) =>
    spy.mock.calls.filter(([kind]) => kind === type).length;
}

/** Pretend the OS asked for less motion. */
function reduceMotion(reduced: boolean) {
  vi.spyOn(window, "matchMedia").mockImplementation(
    (query: string) =>
      ({
        matches: reduced && query.includes("prefers-reduced-motion"),
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }) as MediaQueryList,
  );
}

function pointerAt(x: number, y: number) {
  window.dispatchEvent(
    new MouseEvent("pointermove", { clientX: x, clientY: y }),
  );
}

const gaze = () => [
  document.documentElement.style.getPropertyValue("--gx"),
  document.documentElement.style.getPropertyValue("--gy"),
];

describe("useGaze", () => {
  it("installs one set of listeners however many watchers are on screen", () => {
    const add = vi.spyOn(window, "addEventListener");
    const added = counts(add);

    render(
      <>
        {Array.from({ length: 8 }, (_, i) => (
          <Avatar key={i} id={`bot.${i}`} {...BOT} crewStyle="watchers" />
        ))}
      </>,
    );

    expect(added("pointermove")).toBe(1);
    expect(added("scroll")).toBe(1);
  });

  it("keeps them while any watcher is left, and removes them with the last", () => {
    const remove = vi.spyOn(window, "removeEventListener");
    const removed = counts(remove);

    const first = render(
      <Watchers id="bot.one" name="One" color="b-teal" state="idle" />,
    );
    const second = render(
      <Watchers id="bot.two" name="Two" color="b-pink" state="idle" />,
    );

    first.unmount();
    expect(removed("pointermove")).toBe(0);
    expect(removed("scroll")).toBe(0);

    second.unmount();
    expect(removed("pointermove")).toBe(1);
    expect(removed("scroll")).toBe(1);
  });

  it("puts the eyes back when the last watcher goes", () => {
    const view = render(
      <Watchers id="bot.one" name="One" color="b-teal" state="idle" />,
    );
    pointerAt(1024, 768);
    expect(gaze()).not.toEqual(["", ""]);

    // Left behind, the next watcher to mount would be born mid-glance — and
    // in here one case would set the gaze for every case after it.
    view.unmount();
    expect(gaze()).toEqual(["", ""]);
  });

  it("installs again after the count has been back to zero", () => {
    const add = vi.spyOn(window, "addEventListener");
    const view = render(
      <Watchers id="bot.one" name="One" color="b-teal" state="idle" />,
    );
    view.unmount();
    render(<Watchers id="bot.two" name="Two" color="b-pink" state="idle" />);
    expect(counts(add)("pointermove")).toBe(2);
  });

  it("installs nothing at all under reduced motion", () => {
    reduceMotion(true);
    const add = vi.spyOn(window, "addEventListener");

    render(
      <>
        {Array.from({ length: 4 }, (_, i) => (
          <Avatar key={i} id={`quiet.${i}`} {...BOT} crewStyle="watchers" />
        ))}
      </>,
    );

    expect(counts(add)("pointermove")).toBe(0);
    expect(counts(add)("scroll")).toBe(0);
    pointerAt(1024, 768);
    expect(gaze()).toEqual(["", ""]);
  });

  it("still draws its eyes with the gaze switched off", () => {
    // Opting out by never installing, rather than by installing and declining
    // to move: with no --gx on the root the pupils fall back to the 0 in their
    // own calc, so the eyes look forward and still have pupils. A style that
    // went blank under reduced motion would be the worse accessibility story.
    reduceMotion(true);
    const { container } = render(
      <Watchers id="bot.one" name="One" color="b-teal" state="idle" />,
    );
    const eyes = container.querySelectorAll(".sclera");
    expect(eyes.length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".pupil")).toHaveLength(eyes.length);
  });

  it("eases towards the pointer instead of snapping to it", () => {
    render(<Watchers id="bot.one" name="One" color="b-teal" state="idle" />);

    // Half the remaining distance per event: enough to take the jitter off a
    // fast pointer without the eyes lagging behind it.
    pointerAt(window.innerWidth, window.innerHeight);
    const [firstX] = gaze();
    pointerAt(window.innerWidth, window.innerHeight);
    const [secondX] = gaze();

    expect(Number(firstX)).toBeGreaterThan(0);
    expect(Number(secondX)).toBeGreaterThan(Number(firstX));
    expect(Number(secondX)).toBeLessThanOrEqual(1.1);
  });

  it("moves every watcher on the page together, not one at a time", () => {
    render(
      <>
        <Watchers id="bot.one" name="One" color="b-teal" state="idle" />
        <Watchers id="bot.two" name="Two" color="b-pink" state="idle" />
      </>,
    );
    pointerAt(0, 0);
    // One pair of numbers on the root, which is what makes a crew look at the
    // same thing rather than each of them at you.
    expect(Number(gaze()[0])).toBeLessThan(0);
    expect(document.querySelectorAll(".pupil").length).toBeGreaterThan(1);
  });
});

describe("the gaze and scrolling", () => {
  /** A pane that reports a scroll offset, since jsdom does no layout. */
  function pane(top: number): HTMLElement {
    const el = document.createElement("div");
    Object.defineProperty(el, "scrollTop", { value: top, configurable: true });
    document.body.append(el);
    panes.push(el);
    return el;
  }

  // RTL only clears the containers it made, and a pane left in the body would
  // still be there for the next case to hear from.
  const panes: HTMLElement[] = [];
  afterEach(() => {
    panes.splice(0).forEach((el) => el.remove());
  });

  it("takes the first scroll from a pane as a baseline, not as a jump", () => {
    render(<Watchers id="bot.one" name="One" color="b-teal" state="idle" />);

    // Nothing in the app scrolls the window — the sidebar, the thread and the
    // Inbox each scroll themselves — so one capturing listener sees all three
    // and has to know which one it is hearing from. A pane arriving already
    // scrolled halfway down is not a scroll of half a page.
    const sidebar = pane(900);
    sidebar.dispatchEvent(new Event("scroll"));
    expect(gaze()[1]).toBe("");
  });

  it("moves on the delta once it has a baseline for that pane", () => {
    render(<Watchers id="bot.one" name="One" color="b-teal" state="idle" />);
    const thread = pane(0);
    thread.dispatchEvent(new Event("scroll"));

    Object.defineProperty(thread, "scrollTop", {
      value: 240,
      configurable: true,
    });
    thread.dispatchEvent(new Event("scroll"));
    expect(Number(gaze()[1])).toBeGreaterThan(0);
  });

  it("re-baselines rather than reporting the gap between two panes", () => {
    render(<Watchers id="bot.one" name="One" color="b-teal" state="idle" />);
    const thread = pane(0);
    thread.dispatchEvent(new Event("scroll"));

    // Switching panes: the difference between one pane's scrollTop and
    // another's is not a distance anything travelled.
    const inbox = pane(4000);
    inbox.dispatchEvent(new Event("scroll"));
    expect(gaze()[1]).toBe("");
  });
});
