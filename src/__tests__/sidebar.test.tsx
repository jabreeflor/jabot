/**
 * The sidebar is the navigation model: faces above, folder threads below. What
 * matters is that it lists what it is given, says what each thread is doing,
 * and reports the gestures — a right-click, a folder's ＋, the rail toggle —
 * rather than acting on them itself.
 */
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import {
  Sidebar,
  loadSidebarOpen,
  saveSidebarOpen,
  SIDEBAR_OPEN_KEY,
} from "../components/Sidebar";
import type {
  Bot,
  FolderWithThreads,
  Selection,
} from "../components/types";

const BOTS: Bot[] = [
  {
    id: "chief",
    name: "Chief",
    color: "b-teal",
    instructions: "Route work.",
    tools: [],
    harnessId: "claude",
    isChief: true,
  },
  {
    id: "code",
    name: "Code",
    color: "b-yellow",
    instructions: "Run coding sessions.",
    tools: ["github"],
    harnessId: "claude",
    isChief: false,
    unread: true,
    preview: "Opened PR #23 — checks are green.",
  },
];

const FOLDERS: FolderWithThreads[] = [
  {
    id: "jabot-app",
    name: "jabot-app",
    path: "~/code/jabot-app",
    threads: [
      {
        id: "auth",
        folderId: "jabot-app",
        botId: "code",
        harnessId: "claude",
        title: "Auth migration",
        state: "active",
        foldPolicy: "default",
        runState: "running",
      },
      {
        id: "sidebar",
        folderId: "jabot-app",
        botId: "code",
        harnessId: "codex",
        title: "Sidebar overflow fix",
        state: "active",
        foldPolicy: "default",
        runState: "succeeded",
      },
    ],
  },
];

function renderSidebar(over: Partial<Parameters<typeof Sidebar>[0]> = {}) {
  const props = {
    bots: BOTS,
    folders: FOLDERS,
    selection: { view: "bot", botId: "chief" } as Selection,
    inboxCount: 2,
    openPrCount: 4,
    userName: "Jabree Flor",
    hostLine: "This Mac · v0.1.0",
    onSelectBot: vi.fn(),
    onSelectThread: vi.fn(),
    onOpenCrew: vi.fn(),
    onOpenInbox: vi.fn(),
    onOpenPullRequests: vi.fn(),
    onOpenSchedules: vi.fn(),
    onNewChat: vi.fn(),
    onThreadMenu: vi.fn(),
    onToggle: vi.fn(),
    ...over,
  };
  render(<Sidebar {...props} />);
  return props;
}

describe("Sidebar", () => {
  /** Pairing is a fact about this Mac, so Devices lives under Settings rather
      than as a CODE row. A preview build still has no host to ask, which is
      why the gear itself is host-only — same as before. */
  it("does not offer Devices as its own row", () => {
    renderSidebar({ onOpenSettings: vi.fn() });
    expect(screen.queryByRole("button", { name: "Devices" })).toBeNull();
    expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
  });

  it("marks the Settings gear as current when the pane is open", () => {
    renderSidebar({
      onOpenSettings: vi.fn(),
      selection: { view: "settings" } as Selection,
    });

    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute(
      "aria-current",
      "true",
    );
  });

  it("omits the host subtitle when the connection is healthy", () => {
    renderSidebar({ hostLine: "" });

    expect(screen.queryByText("This Mac · v0.1.0")).not.toBeInTheDocument();
    expect(document.querySelector(".me-row .host")).toBeNull();
  });

  it("keeps transient host status visible", () => {
    renderSidebar({ hostLine: "Connecting to host…" });

    expect(screen.getByText("Connecting to host…")).toBeInTheDocument();
  });

  it("lists every thread it is given, with what that thread is doing", () => {
    renderSidebar();

    const running = screen.getByRole("button", {
      name: "Auth migration, running",
    });
    expect(running.querySelector(".sparkle.live")).not.toBeNull();
    expect(running.querySelectorAll("[data-testid=sparkle] > span")).toHaveLength(
      9,
    );
    expect(running).not.toHaveTextContent("running");

    const done = screen.getByRole("button", {
      name: "Sidebar overflow fix, done",
    });
    expect(done.querySelector(".sparkle.live")).toBeNull();
    expect(done.querySelector("[data-testid=sparkle]")).toHaveAttribute(
      "data-tone",
      "ok",
    );
    expect(done).not.toHaveTextContent("done");
  });

  it("shows the crew as chat rows, with the unread dot where there is news", () => {
    renderSidebar();

    expect(screen.getByRole("button", { name: /^Chief/ })).toBeInTheDocument();
    const code = screen.getByRole("button", { name: /^Code/ });
    expect(within(code).getByTestId("unread-dot")).toBeInTheDocument();
  });

  /** The row's second line is the conversation, which is the whole reason a
      face became a row. */
  it("shows the last thing said in each bot's chat", () => {
    renderSidebar();

    const code = screen.getByRole("button", { name: /^Code/ });
    expect(code).toHaveTextContent("Opened PR #23 — checks are green.");
  });

  /** A bot nobody has talked to has no last line. Saying nothing there would
      leave a blank row; saying what the bot is *for* is the only other true
      thing about a conversation that has not started. */
  it("falls back to what a bot is for until it has been talked to", () => {
    renderSidebar();

    const chief = screen.getByRole("button", { name: /^Chief/ });
    expect(chief).toHaveTextContent("Route work.");
  });

  it("counts what is waiting", () => {
    renderSidebar();

    expect(
      screen.getByRole("button", { name: "Inbox — 2 waiting" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Pull Requests — 4 open" }),
    ).toHaveTextContent("4");
  });

  it("hides the badge when nothing wants you", () => {
    renderSidebar({ inboxCount: 0 });

    expect(screen.queryByRole("button", { name: /waiting/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Inbox" })).toBeInTheDocument();
  });

  it("filters threads by search, keeping the crew visible", async () => {
    renderSidebar();

    await userEvent.type(screen.getByLabelText("Search threads"), "overflow");

    expect(
      screen.queryByRole("button", { name: /Auth migration/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Sidebar overflow fix/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Chief/ })).toBeInTheDocument();
  });

  it("says so when a search matches nothing", async () => {
    renderSidebar();

    await userEvent.type(screen.getByLabelText("Search threads"), "kubernetes");

    expect(screen.getByText(/No threads match/)).toBeInTheDocument();
  });

  it("starts a folder thread already pointed at that folder", async () => {
    const props = renderSidebar();

    await userEvent.click(
      screen.getByRole("button", { name: "New thread in jabot-app" }),
    );
    expect(props.onNewChat).toHaveBeenCalledWith("jabot-app");

    await userEvent.click(screen.getByRole("button", { name: "New Chat" }));
    expect(props.onNewChat).toHaveBeenLastCalledWith(null);
  });

  it("reports a right-click with the thread and where it happened", async () => {
    const props = renderSidebar();

    await userEvent.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: /Auth migration/ }),
    });

    expect(props.onThreadMenu).toHaveBeenCalledWith(
      expect.objectContaining({ id: "auth" }),
      expect.objectContaining({ x: expect.any(Number) }),
    );
  });

  it("collapses a folder without losing its thread count", async () => {
    renderSidebar();

    await userEvent.click(screen.getByRole("button", { name: "jabot-app" }));

    expect(
      screen.queryByRole("button", { name: /Auth migration/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^jabot-app/ }),
    ).toHaveTextContent("2");
  });

  // The glyph is state, not decoration: the front panel it draws open is the
  // same fact `aria-expanded` carries, so the two cannot be allowed to drift.
  // Only the flag is asserted — the shear itself is CSS, which jsdom does not
  // run, and a test that pinned the transform would be pinning a drawing.
  it("draws the folder open exactly while the folder is expanded", async () => {
    renderSidebar();

    const toggle = screen.getByRole("button", { name: /^jabot-app/ });
    const glyph = () => toggle.querySelector(".folder-glyph");

    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(glyph()).toHaveAttribute("data-open", "true");
    // Two paths, or there is nothing for the panel to move against.
    expect(glyph()?.querySelector(".folder-front")).toBeInTheDocument();
    expect(glyph()?.querySelector(".folder-shell")).toBeInTheDocument();

    await userEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(glyph()).toHaveAttribute("data-open", "false");
  });

  it("hides the list when closed and keeps the toggle that opens it", () => {
    const onToggle = vi.fn();
    renderSidebar({ open: false, onToggle });

    expect(
      screen.getByRole("button", { name: "Show sidebar" }),
    ).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByLabelText("Search threads")).toBeNull();
    expect(screen.queryByRole("button", { name: /Chief/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Auth migration/ })).toBeNull();
    expect(screen.queryByText("Jabree Flor")).toBeNull();
  });

  it("reports a click on the toggle rather than folding itself", async () => {
    const props = renderSidebar();

    await userEvent.click(screen.getByRole("button", { name: "Hide sidebar" }));
    expect(props.onToggle).toHaveBeenCalled();
    // Still open: the shell owns the state, the same way a right-click is
    // reported rather than acted on.
    expect(
      screen.getByRole("button", { name: /Auth migration/ }),
    ).toBeInTheDocument();
  });

  it("does not peek the list until the toggle is focused or hovered", () => {
    renderSidebar({ open: false });

    fireEvent.pointerMove(window, { clientX: 80, clientY: 40 });

    expect(screen.queryByRole("button", { name: /Chief/ })).toBeNull();
    expect(document.querySelector(".sidebar.is-peeking")).toBeNull();
  });

  it("peeks the list after the toggle is focused and the pointer enters the rail", () => {
    const onToggle = vi.fn();
    renderSidebar({ open: false, onToggle });

    fireEvent.focus(screen.getByRole("button", { name: "Show sidebar" }));
    expect(screen.queryByRole("button", { name: /Chief/ })).toBeNull();

    fireEvent.pointerMove(window, { clientX: 120, clientY: 80 });

    expect(screen.getByRole("button", { name: /Chief/ })).toBeInTheDocument();
    expect(document.querySelector(".sidebar.is-peeking")).not.toBeNull();
    expect(onToggle).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Show sidebar" }),
    ).toHaveAttribute("aria-expanded", "false");
  });

  it("closes a peek when the pointer leaves the rail and does not pin it", () => {
    const onToggle = vi.fn();
    renderSidebar({ open: false, onToggle });

    const toggle = screen.getByRole("button", { name: "Show sidebar" });
    fireEvent.pointerEnter(toggle);
    expect(screen.getByRole("button", { name: /Chief/ })).toBeInTheDocument();

    fireEvent.pointerMove(window, { clientX: 640, clientY: 80 });
    fireEvent.pointerLeave(document.querySelector(".sidebar") as HTMLElement);

    expect(screen.queryByRole("button", { name: /Chief/ })).toBeNull();
    expect(document.querySelector(".sidebar.is-peeking")).toBeNull();
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("does not peek from the click that hid the rail until the pointer moves", async () => {
    function Harness() {
      const [open, setOpen] = useState(true);
      return (
        <Sidebar
          bots={BOTS}
          folders={FOLDERS}
          selection={{ view: "bot", botId: "chief" }}
          inboxCount={0}
          openPrCount={0}
          userName="Jabree Flor"
          hostLine=""
          onSelectBot={vi.fn()}
          onSelectThread={vi.fn()}
          onOpenCrew={vi.fn()}
          onOpenInbox={vi.fn()}
          onOpenPullRequests={vi.fn()}
          onOpenSchedules={vi.fn()}
          onNewChat={vi.fn()}
          onThreadMenu={vi.fn()}
          open={open}
          onToggle={() => setOpen((value) => !value)}
        />
      );
    }
    render(<Harness />);

    await userEvent.click(screen.getByRole("button", { name: "Hide sidebar" }));
    expect(screen.queryByRole("button", { name: /Chief/ })).toBeNull();

    fireEvent.pointerMove(window, { clientX: 120, clientY: 80 });
    expect(screen.queryByRole("button", { name: /Chief/ })).toBeNull();

    const toggle = screen.getByRole("button", { name: "Show sidebar" });
    fireEvent.pointerLeave(toggle);
    fireEvent.pointerMove(window, { clientX: 120, clientY: 80 });
    expect(screen.getByRole("button", { name: /Chief/ })).toBeInTheDocument();
  });

  it("only an explicit 0 hides the rail on the next launch", () => {
    expect(loadSidebarOpen()).toBe(true);

    saveSidebarOpen(false);
    expect(window.localStorage.getItem(SIDEBAR_OPEN_KEY)).toBe("0");
    expect(loadSidebarOpen()).toBe(false);

    saveSidebarOpen(true);
    expect(loadSidebarOpen()).toBe(true);

    window.localStorage.setItem(SIDEBAR_OPEN_KEY, "garbage");
    expect(loadSidebarOpen()).toBe(true);
  });
});
