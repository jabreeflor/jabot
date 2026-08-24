/**
 * The temporary crew-style switch (#44).
 *
 * It exists so a direction can be chosen in the real shell rather than on a
 * prototype page, which puts three things under test: the six previews have to
 * disagree with each other (a picker where every option draws the same thing
 * is not a picker), the choice has to reach every avatar on the page and not
 * only the previews, and it has to survive a restart or nobody can live with a
 * style for a day. This file goes out with the switch.
 */
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CREW_STYLE_KEY, CrewStyleProvider } from "../components/avatar";
import { CrewView } from "../views/CrewView";
import {
  HARNESSES,
  HOST_TOOLS,
  TOOL_CATALOG,
  initialMockState,
} from "../views/mock-host";

afterEach(() => {
  window.localStorage.clear();
  vi.restoreAllMocks();
});

function renderCrew() {
  render(
    <CrewStyleProvider>
      <CrewView
        bots={initialMockState().bots}
        harnesses={HARNESSES}
        tools={[...TOOL_CATALOG, ...HOST_TOOLS]}
        onEdit={vi.fn()}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
      />
    </CrewStyleProvider>,
  );
  return screen.getByRole("group", { name: "Crew style" });
}

/** The style a drawing is in, read off the wrapper the way the CSS reads it. */
function styleOf(avatar: Element): string | undefined {
  return [...avatar.classList].find(
    (name) => name !== "av" && !name.startsWith("b-"),
  );
}

/** Every avatar on the page that is not one of the picker's own previews. */
function crewCardAvatars(): Element[] {
  return [...document.querySelectorAll(".crew-card .av")];
}

describe("the crew-style switch", () => {
  it("draws each option in its own style, not in the current one", () => {
    const picker = renderCrew();
    const previews = [...picker.querySelectorAll(".av")];

    expect(previews).toHaveLength(6);
    expect(new Set(previews.map(styleOf))).toEqual(
      new Set([
        "classic",
        "moodblob",
        "hats",
        "critters",
        "pixels",
        "watchers",
      ]),
    );
  });

  it("marks exactly the style that is switched on", async () => {
    const picker = renderCrew();
    const pressed = () =>
      within(picker)
        .getAllByRole("button", { pressed: true })
        .map((button) => button.textContent);

    // "hats" is the default an untouched install gets.
    expect(pressed()).toEqual(["Hat crew"]);

    await userEvent.click(
      within(picker).getByRole("button", { name: "Pixel pets" }),
    );
    expect(pressed()).toEqual(["Pixel pets"]);
  });

  it("redraws the crew, and not only the previews", async () => {
    const picker = renderCrew();
    expect(crewCardAvatars().map(styleOf)).toContain("hats");

    await userEvent.click(
      within(picker).getByRole("button", { name: "Watchers" }),
    );

    const drawn = crewCardAvatars();
    expect(drawn.length).toBeGreaterThan(1);
    expect(drawn.every((avatar) => styleOf(avatar) === "watchers")).toBe(true);
  });

  it("remembers the choice across a reload", async () => {
    const picker = renderCrew();
    await userEvent.click(
      within(picker).getByRole("button", { name: "Critter kit" }),
    );

    expect(window.localStorage.getItem(CREW_STYLE_KEY)).toBe("critters");

    // A fresh mount is what a restart looks like from here: the provider reads
    // the store once, lazily, and never writes on the way in.
    cleanup();
    const reloaded = renderCrew();
    expect(crewCardAvatars().every((av) => styleOf(av) === "critters")).toBe(
      true,
    );
    expect(
      within(reloaded)
        .getAllByRole("button", { pressed: true })
        .map((button) => button.textContent),
    ).toEqual(["Critter kit"]);
  });

  it("names each option by its style and never by the bot it draws", () => {
    const picker = renderCrew();

    // Six buttons that all draw Chief, so the label is the only thing telling
    // them apart and it has to be the only thing in the name.
    expect(
      within(picker).getByRole("button", { name: "Moodblob" }),
    ).toBeInTheDocument();
    expect(
      within(picker).queryByRole("button", { name: /Chief/ }),
    ).not.toBeInTheDocument();
  });
});
