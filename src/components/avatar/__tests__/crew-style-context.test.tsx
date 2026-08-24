/**
 * Where "which crew is on screen" lives (#44).
 *
 * The switch is a context rather than a prop because the avatar turns up in
 * the sidebar, the chat header, the Inbox, the crew page and the bot editor,
 * and threading a style through all five would make every component in between
 * carry a setting it has no opinion about. Three things follow from that and
 * are what this file pins: reading the setting has to work with no provider
 * mounted at all, changing it has to reach every avatar at once, and the
 * choice has to outlive the process or nobody can live with a style for a day.
 *
 * The picker's own buttons are tested in `src/__tests__/crew-style.test.tsx`;
 * this is the machinery underneath them.
 */
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Avatar } from "../Avatar";
import {
  CrewStyleProvider,
  loadCrewStyle,
  useCrewStyle,
  useSetCrewStyle,
} from "../CrewStyleContext";
import { CREW_STYLE_KEY, DEFAULT_CREW_STYLE, type CrewStyle } from "../crew";

afterEach(() => {
  vi.restoreAllMocks();
});

const BOT = { id: "bot.mira", name: "Mira", color: "b-teal" } as const;

/** The style a drawing is in, read off the wrapper the way the CSS reads it. */
function styleOnScreen(): string {
  const el = document.querySelector(".av");
  if (!el) throw new Error("nothing drawn");
  const style = [...el.classList].find(
    (name) => name !== "av" && !name.startsWith("b-"),
  );
  if (!style) throw new Error("drawn in no style at all");
  return style;
}

/** An avatar and a control that changes the setting out from under it. */
function Switcher({ to }: { to: CrewStyle }) {
  const setStyle = useSetCrewStyle();
  return (
    <>
      <Avatar {...BOT} />
      <button onClick={() => setStyle(to)}>switch</button>
    </>
  );
}

describe("reading the setting", () => {
  it("draws the default with no provider anywhere in the tree", () => {
    // Deliberately reachable outside the provider: a dozen suites render an
    // Inbox row or a bot tile in isolation, and a context that threw would
    // turn "this component renders" into "this component renders inside a tree
    // it does not otherwise need".
    render(<Avatar {...BOT} />);
    expect(styleOnScreen()).toBe(DEFAULT_CREW_STYLE);
  });

  it("hands the setter a no-op rather than pretending the write happened", async () => {
    // There is nowhere to put the value with no provider mounted. Silently
    // succeeding would leave the caller waiting on a re-render that is never
    // coming, so the honest answer is that nothing moves.
    render(<Switcher to="pixels" />);
    await userEvent.click(screen.getByRole("button", { name: "switch" }));
    expect(styleOnScreen()).toBe(DEFAULT_CREW_STYLE);
  });

  it("reads a stored choice on the way in", () => {
    window.localStorage.setItem(CREW_STYLE_KEY, "watchers");
    render(
      <CrewStyleProvider>
        <Avatar {...BOT} />
      </CrewStyleProvider>,
    );
    expect(styleOnScreen()).toBe("watchers");
  });

  it("falls back to the default on a value it does not recognise", () => {
    // The store is shared with whatever else this origin has ever written, and
    // a stale key from an earlier build must not put an undefined renderer on
    // screen.
    window.localStorage.setItem(CREW_STYLE_KEY, "blobs");
    expect(loadCrewStyle()).toBe(DEFAULT_CREW_STYLE);

    render(
      <CrewStyleProvider>
        <Avatar {...BOT} />
      </CrewStyleProvider>,
    );
    expect(styleOnScreen()).toBe(DEFAULT_CREW_STYLE);
  });

  it("still draws when the store will not answer", () => {
    // A private window or a webview with site data off. The app has a
    // perfectly good default and the only cost is that the choice does not
    // survive a restart, so this is not an error worth surfacing.
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("site data is off");
    });
    render(
      <CrewStyleProvider>
        <Avatar {...BOT} />
      </CrewStyleProvider>,
    );
    expect(styleOnScreen()).toBe(DEFAULT_CREW_STYLE);
  });
});

describe("changing the setting", () => {
  it("redraws every avatar under the provider at once", async () => {
    render(
      <CrewStyleProvider>
        <Switcher to="critters" />
        <Avatar id="bot.other" name="Other" color="b-pink" />
      </CrewStyleProvider>,
    );

    const styles = () =>
      [...document.querySelectorAll(".av")].map(
        (el) => [...el.classList].find((c) => c !== "av" && !c.startsWith("b-")),
      );
    expect(styles()).toEqual([DEFAULT_CREW_STYLE, DEFAULT_CREW_STYLE]);

    await userEvent.click(screen.getByRole("button", { name: "switch" }));
    expect(styles()).toEqual(["critters", "critters"]);
  });

  it("writes the choice and reads it back on a fresh mount", async () => {
    render(
      <CrewStyleProvider>
        <Switcher to="moodblob" />
      </CrewStyleProvider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "switch" }));
    expect(window.localStorage.getItem(CREW_STYLE_KEY)).toBe("moodblob");

    // A fresh mount is what a restart looks like from in here: the provider
    // reads the store once, lazily, and never writes on the way in.
    cleanup();
    render(
      <CrewStyleProvider>
        <Avatar {...BOT} />
      </CrewStyleProvider>,
    );
    expect(styleOnScreen()).toBe("moodblob");
  });

  it("keeps the session's choice when the store refuses the write", async () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    render(
      <CrewStyleProvider>
        <Switcher to="pixels" />
      </CrewStyleProvider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "switch" }));
    expect(styleOnScreen()).toBe("pixels");
  });
});

describe("the picker's override", () => {
  it("draws a named style without touching the setting", () => {
    // Six previews that all obey the setting are not a picker. This prop is
    // that one caller's, and it goes out with the switch.
    render(
      <CrewStyleProvider>
        <Avatar {...BOT} crewStyle="classic" />
        <Avatar id="bot.other" name="Other" color="b-pink" />
      </CrewStyleProvider>,
    );
    const [preview, live] = [...document.querySelectorAll(".av")];
    expect(preview).toHaveClass("classic");
    expect(live).toHaveClass(DEFAULT_CREW_STYLE);
    expect(window.localStorage.getItem(CREW_STYLE_KEY)).toBeNull();
  });

  it("stays on its own style when the setting changes underneath it", async () => {
    render(
      <CrewStyleProvider>
        <Avatar {...BOT} crewStyle="classic" />
        <Switcher to="watchers" />
      </CrewStyleProvider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "switch" }));
    const [preview, live] = [...document.querySelectorAll(".av")];
    expect(preview).toHaveClass("classic");
    expect(live).toHaveClass("watchers");
  });
});

describe("the hook itself", () => {
  it("reports the style the tree is currently drawing in", async () => {
    function Readout() {
      const style = useCrewStyle();
      const setStyle = useSetCrewStyle();
      return (
        <button onClick={() => setStyle("pixels")}>{style}</button>
      );
    }
    render(
      <CrewStyleProvider>
        <Readout />
      </CrewStyleProvider>,
    );
    expect(screen.getByRole("button")).toHaveTextContent(DEFAULT_CREW_STYLE);
    await act(async () => {
      await userEvent.click(screen.getByRole("button"));
    });
    expect(screen.getByRole("button")).toHaveTextContent("pixels");
  });
});
