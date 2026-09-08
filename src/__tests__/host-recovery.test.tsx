/**
 * Mid-session host death has to be a visible, honest state — not a healthy
 * sidebar sitting on a dead process. The Vite bridge synthesizes
 * `host/disconnected`; the shell must show it and offer Reconnect.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import {
  connectHost,
  HOST_DISCONNECTED,
  HOST_RECONNECTED,
  type HelloResult,
  type HostClient,
  type JsonRpcNotification,
} from "../host";

type NotificationHandler = (notification: JsonRpcNotification) => void;

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

function mockClient(hello: HelloResult = HELLO) {
  const handlers = new Set<NotificationHandler>();
  const client = {
    disconnect: vi.fn(),
    hello: vi.fn(async () => hello),
    onNotification: (handler: NotificationHandler) => {
      handlers.add(handler);
      return () => {
        handlers.delete(handler);
      };
    },
    emit(method: string) {
      for (const handler of handlers) {
        handler({ jsonrpc: "2.0", method });
      }
    },
  };
  return client;
}

beforeEach(() => {
  connected.mockReset();
});

describe("host disconnect recovery", () => {
  it("shows Host disconnected and Reconnect, then restores after hello", async () => {
    const client = mockClient();
    connected.mockResolvedValue({
      client: client as unknown as HostClient,
      hello: HELLO,
    });

    render(<App />);
    await screen.findByRole("button", { name: "Settings" });
    expect(screen.queryByText("Host disconnected")).not.toBeInTheDocument();

    act(() => client.emit(HOST_DISCONNECTED));

    expect(await screen.findByText("Host disconnected")).toBeInTheDocument();
    const reconnect = screen.getByRole("button", { name: "Reconnect" });
    expect(reconnect).toBeInTheDocument();

    await userEvent.click(reconnect);
    await waitFor(() => expect(client.hello).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByText("Host disconnected")).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: "Reconnect" }),
    ).not.toBeInTheDocument();
  });

  it("treats a bridge reconnect notification as a successful handshake", async () => {
    const client = mockClient();
    connected.mockResolvedValue({
      client: client as unknown as HostClient,
      hello: HELLO,
    });

    render(<App />);
    await screen.findByRole("button", { name: "Settings" });
    act(() => client.emit(HOST_DISCONNECTED));
    expect(await screen.findByText("Host disconnected")).toBeInTheDocument();

    act(() => client.emit(HOST_RECONNECTED));
    await waitFor(() => expect(client.hello).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByText("Host disconnected")).not.toBeInTheDocument(),
    );
  });
});
