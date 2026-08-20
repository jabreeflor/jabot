/**
 * The shell, end to end against the mock host: navigation between the five
 * views, and the two interactions the prototype was built to demonstrate —
 * starting a session from a folder, and folding a thread away so it comes back
 * through the Inbox.
 *
 * `connectHost` is stubbed because the host handshake is #8's contract, not
 * this port's; what is asserted here is that the shell reports the host's
 * answer, including when there isn't one.
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import { connectHost, type HelloResult, type HostClient } from "../host";

vi.mock("../host", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../host")>();
  return { ...actual, connectHost: vi.fn() };
});

const HELLO: HelloResult = {
  protocolVersion: 1,
  hostId: "host-1",
  hostName: "This Mac",
  hostMode: "in-process",
  version: "0.1.0",
  platform: "macos",
  device: { deviceId: "dev-1", name: "This Mac", role: "full" },
  methods: [],
  notifications: [],
};

const connected = vi.mocked(connectHost);

beforeEach(() => {
  connected.mockResolvedValue({
    client: { disconnect: vi.fn() } as unknown as HostClient,
    hello: HELLO,
  });
});

async function renderApp() {
  render(<App />);
  await screen.findByText("This Mac · v0.1.0");
}

describe("App", () => {
  it("opens on Chief's chat with the crew and the code rows", async () => {
    await renderApp();

    expect(screen.getByRole("heading", { name: "Chief" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^Inbox —/ }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Message Chief")).toBeInTheDocument();
  });

  it("moves between the crew, Inbox, and Pull Requests", async () => {
    await renderApp();

    await userEvent.click(screen.getByRole("button", { name: /Crew/ }));
    expect(
      screen.getByRole("heading", { level: 1, name: "Your Crew" }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /^Inbox —/ }));
    expect(
      screen.getByRole("heading", { level: 1, name: "Inbox" }),
    ).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: /^Pull Requests/ }),
    );
    expect(
      screen.getByRole("heading", { level: 1, name: "Pull Requests" }),
    ).toBeInTheDocument();
  });

  it("opens a code thread with its harness and its transcript", async () => {
    await renderApp();

    await userEvent.click(
      screen.getByRole("button", { name: /Auth migration/ }),
    );

    expect(
      screen.getByRole("heading", { name: "Auth migration" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Claude Code")).toBeInTheDocument();
    expect(screen.getByText(/Middleware rewritten/)).toBeInTheDocument();
    expect(screen.getByText(/npm test/)).toBeInTheDocument();
  });

  it("starts a session from a folder and opens it", async () => {
    await renderApp();

    await userEvent.click(
      screen.getByRole("button", { name: "New thread in globnet-sync" }),
    );
    await userEvent.click(screen.getByRole("button", { name: /Codex/ }));
    await userEvent.type(
      screen.getByLabelText("WHAT SHOULD IT DO?"),
      "Rotate the backup keys",
    );
    await userEvent.click(screen.getByRole("button", { name: "Start session" }));

    expect(
      screen.getByRole("heading", { name: "Rotate the backup keys" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Codex")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Rotate the backup keys/ }),
    ).toBeInTheDocument();
  });

  it("folds a thread from Chief's card and finds it asleep in the Inbox", async () => {
    await renderApp();

    expect(
      screen.getByRole("button", { name: /Auth migration/ }),
    ).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Disappear until done" }),
    );

    expect(
      screen.getByText("Thread folded — will reappear in Inbox"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Auth migration/ }),
    ).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /^Inbox —/ }));
    const sleeping = screen.getByText("Auth migration").closest("button");
    expect(within(sleeping!).getByText("SLEEPING")).toBeInTheDocument();
  });

  it("folds a thread from its right-click menu", async () => {
    await renderApp();

    await userEvent.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: /Sidebar overflow fix/ }),
    });
    await userEvent.click(
      screen.getByRole("menuitem", { name: /Wait for Inbox/ }),
    );

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /Sidebar overflow fix/ }),
      ).not.toBeInTheDocument(),
    );
  });

  it("deletes a thread and says so instead of showing a blank pane", async () => {
    await renderApp();

    await userEvent.click(
      screen.getByRole("button", { name: /Auth migration/ }),
    );
    await userEvent.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button", { name: /Auth migration/ }),
    });
    await userEvent.click(screen.getByRole("menuitem", { name: /Delete/ }));

    await waitFor(() =>
      expect(screen.getByText(/That thread is gone/)).toBeInTheDocument(),
    );
  });

  it("puts what I type into the transcript", async () => {
    await renderApp();

    await userEvent.type(
      screen.getByLabelText("Message Chief"),
      "Fold the migration{Enter}",
    );

    expect(screen.getByText("Fold the migration")).toBeInTheDocument();
    expect(screen.getByLabelText("Message Chief")).toHaveValue("");
  });

  it("adds a bot from a template and shows it in the crew", async () => {
    await renderApp();

    await userEvent.click(screen.getByRole("button", { name: /Crew/ }));
    await userEvent.click(screen.getByRole("button", { name: /Add a bot/ }));
    await userEvent.selectOptions(
      screen.getByLabelText("START FROM A TEMPLATE"),
      "expense",
    );
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    // Once as a crew card, once as a face in the sidebar strip.
    expect(screen.getAllByText("Expense Manager")).toHaveLength(2);
    expect(
      screen.getByRole("button", { name: "Expense Manager" }),
    ).toBeInTheDocument();
  });

  it("says the host is unreachable rather than pretending it is there", async () => {
    connected.mockRejectedValue(new Error("no Tauri bridge"));
    render(<App />);

    expect(await screen.findByText(/no Tauri bridge/)).toBeInTheDocument();
    // The rest of the shell still renders — the views do not depend on it yet.
    expect(screen.getByRole("heading", { name: "Chief" })).toBeInTheDocument();
  });
});
