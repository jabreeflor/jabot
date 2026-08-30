/**
 * The mascot every surface draws, the picture a user can substitute, and the
 * state chrome shared by both. The colour classes supply the variations; the
 * mascot asset supplies one recognizable product identity.
 */
import { cleanup, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { BOT_COLORS, type BotColor } from "../../types";
import { Avatar, CrewAvatar } from "../Avatar";
import type { AvatarState } from "../state";

const STATES: readonly AvatarState[] = ["idle", "running", "waiting", "failed"];

const CREW: readonly { name: string; color: BotColor }[] = Array.from(
  { length: 12 },
  (_, i) => ({
    name: `Bot ${i}`,
    color: BOT_COLORS[i % BOT_COLORS.length],
  }),
);

/** A 1×1 PNG, as small as a real `data:` URL gets. */
const PIXEL =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

function avatar(props: Parameters<typeof Avatar>[0]): HTMLElement {
  const { container } = render(<Avatar {...props} />);
  return container.firstElementChild as HTMLElement;
}

describe("the mascot variations", () => {
  it("draws the shared mascot in all eight colours", () => {
    for (const color of BOT_COLORS) {
      const el = avatar({ name: "Probe", color });
      expect(el).toHaveClass("av", color);
      expect(el.querySelector(".mascot-stage")).not.toBeNull();
      expect(el.querySelector(".mascot")).toHaveAttribute("src");
      cleanup();
    }
  });

  it("draws the mascot even when a bot has no usable name", () => {
    for (const name of ["", "   "]) {
      const el = avatar({ name, color: "b-blue" });
      expect(el.querySelector(".mascot")).not.toBeNull();
      cleanup();
    }
  });
});

describe("the uploaded picture", () => {
  it("replaces the mark rather than sitting beside it", () => {
    const el = avatar({ name: "Mira", color: "b-pink", image: PIXEL });
    const picture = el.querySelector(".pic");
    expect(picture).toHaveAttribute("src", PIXEL);
    expect(el.querySelector(".mascot")).toBeNull();
  });

  it("keeps the chrome, so a bot with a picture can still be unread and busy", () => {
    const el = avatar({
      name: "Mira",
      color: "b-pink",
      image: PIXEL,
      unread: true,
      state: "running",
    });
    expect(screen.getByTestId("unread-dot")).toBeInTheDocument();
    expect(screen.getByTestId("state-ring")).toBeInTheDocument();
    expect(el.querySelector(".pic")).not.toBeNull();
  });

  it("falls back to the mark when the row holds something that is not an icon", () => {
    // The value has been through the host and back, and it goes straight into
    // a `src`. A `javascript:` or an `http:` in that field is either an attack
    // or a bug, and either way the answer is the built-in mascot.
    for (const bad of [
      "javascript:alert(1)",
      "http://example.com/avatar.png",
      "data:image/svg+xml;base64,PHN2Zy8+",
      "data:text/html;base64,PGI+",
      "",
    ]) {
      const el = avatar({ name: "Mira", color: "b-pink", image: bad });
      expect(el.querySelector(".pic"), bad).toBeNull();
      expect(el.querySelector(".mascot"), bad).not.toBeNull();
      cleanup();
    }
  });

  it("leaves the picture out of the accessible name", () => {
    // `labelled` names the whole avatar; an alt on the image inside it would
    // announce the bot twice.
    render(<Avatar name="Mira" color="b-pink" image={PIXEL} labelled />);
    const named = screen.getByRole("img", { name: "Mira" });
    expect(named.tagName).toBe("SPAN");
    expect(named.querySelector("img")).toHaveAttribute("alt", "");
  });
});

describe("the ring, which is the whole state vocabulary now", () => {
  it("puts the state on the wrapper and draws a ring for all but idle", () => {
    for (const state of STATES) {
      const el = avatar({ ...CREW[0], state });
      expect(el).toHaveAttribute("data-state", state);
      expect(el.querySelector(".ring") !== null, state).toBe(state !== "idle");
      cleanup();
    }
  });

  it("is idle by default, so a bot nobody asked about is quiet", () => {
    const el = avatar({ ...CREW[0] });
    expect(el).toHaveAttribute("data-state", "idle");
    expect(el.querySelector(".ring")).toBeNull();
  });
});

describe("the chrome", () => {
  it("draws the unread dot only when there is something unread", () => {
    render(<Avatar {...CREW[0]} unread />);
    expect(screen.getByTestId("unread-dot")).toBeInTheDocument();
    cleanup();

    render(<Avatar {...CREW[0]} />);
    expect(screen.queryByTestId("unread-dot")).not.toBeInTheDocument();
  });

  it("keeps the dot outside the drawing, so it cannot be clipped away", () => {
    const el = avatar({ ...CREW[0], unread: true });
    const dot = screen.getByTestId("unread-dot");
    expect(dot.parentElement).toBe(el);
    expect(dot.closest("svg")).toBeNull();
  });

  it("names the bot with a tooltip and stays out of the accessible name", () => {
    // Every call site already prints the name in text beside the avatar, so an
    // unconditional aria-label makes the sidebar button announce "Bot 0 Bot 0"
    // and breaks getByRole("button", { name }).
    const el = avatar({ ...CREW[0] });
    expect(el).toHaveAttribute("title", "Bot 0");
    expect(el).not.toHaveAttribute("aria-label");
    expect(el.querySelector(".mascot-stage")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });

  it("takes a name when the avatar is the only thing in its control", () => {
    render(<Avatar {...CREW[0]} labelled />);
    expect(screen.getByRole("img", { name: "Bot 0" })).toBeInTheDocument();
  });

  it("drops the tooltip when the caller has one of its own to show", () => {
    const el = avatar({ ...CREW[0], titled: false });
    expect(el).not.toHaveAttribute("title");
  });

  it("keeps any class the call site asks for", () => {
    const el = avatar({ ...CREW[0], className: "setup-cluster" });
    expect(el).toHaveClass("setup-cluster");
  });
});

describe("the crew's own avatar", () => {
  it("draws three mascot portraits in three slots", () => {
    const { container } = render(<CrewAvatar />);
    const cluster = container.firstElementChild as HTMLElement;
    expect(cluster).toHaveClass("cluster", "av-cluster");
    expect(cluster).toHaveAttribute("aria-hidden", "true");

    const slots = cluster.querySelectorAll(":scope > i");
    expect(slots).toHaveLength(3);
    // The slot classes, not :nth-child. A drawing has children of its own, and
    // an index selector eventually matches one of them and rearranges it.
    expect([...slots].map((slot) => slot.className)).toEqual([
      "s1",
      "s2",
      "s3",
    ]);
  });

  it("draws three different colours, so the tile reads as a crew", () => {
    const { container } = render(<CrewAvatar />);
    const colours = [...container.querySelectorAll(".av")].map(
      (el) => [...el.classList].find((c) => c.startsWith("b-")) ?? "",
    );
    expect(new Set(colours).size).toBe(3);
  });

  it("draws the mascot in all three slots", () => {
    const { container } = render(<CrewAvatar />);
    expect(container.querySelectorAll(".mascot")).toHaveLength(3);
  });
});
