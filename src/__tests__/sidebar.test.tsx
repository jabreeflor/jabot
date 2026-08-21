/**
 * The sidebar is the navigation model: faces above, folder threads below. What
 * matters is that it lists what it is given, says what each thread is doing,
 * and reports the gestures — a right-click, a folder's ＋ — rather than acting
 * on them itself.
 */
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { Sidebar } from "../components/Sidebar";
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
    ...over,
  };
  render(<Sidebar {...props} />);
  return props;
}

describe("Sidebar", () => {
  it("lists every thread it is given, with what that thread is doing", () => {
    renderSidebar();

    expect(
      screen.getByRole("button", { name: /Auth migration/ }),
    ).toHaveTextContent("running");
    expect(
      screen.getByRole("button", { name: /Sidebar overflow fix/ }),
    ).toHaveTextContent("done");
  });

  it("shows the crew as faces, with the unread dot where there is news", () => {
    renderSidebar();

    expect(screen.getByRole("button", { name: /Chief/ })).toBeInTheDocument();
    const code = screen.getByRole("button", { name: /^Code$/ });
    expect(within(code).getByTestId("unread-dot")).toBeInTheDocument();
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
});
