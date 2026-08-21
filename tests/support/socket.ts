/**
 * A [`LineChannel`] over a Unix domain socket, for the second client.
 *
 * This is the *test's* rung of the reach ladder in
 * `docs/research/remote-and-mobile/protocol-and-reach.md`: rung 0, a socket on
 * the same machine. A real phone would supply a WebSocket instead. Neither the
 * mobile client nor the host knows the difference, which is the claim — the
 * transport is a duplex of lines and the protocol is unchanged.
 */
import { createConnection, type Socket } from "node:net";

import type { LineChannel } from "../../src/mobile/transport";

export interface SocketChannel extends LineChannel {
  /** Resolves once the socket is connected, or rejects if it never gets there. */
  ready: Promise<void>;
}

export function connectUnixSocket(socketPath: string): SocketChannel {
  const socket: Socket = createConnection({ path: socketPath });
  socket.setEncoding("utf8");
  let buffer = "";
  let lineHandler: ((line: string) => void) | null = null;
  let closeHandler: ((reason?: Error) => void) | null = null;

  const ready = new Promise<void>((resolve, reject) => {
    socket.once("connect", () => resolve());
    socket.once("error", (err: Error) => reject(err));
  });

  socket.on("data", (chunk: string) => {
    buffer += chunk;
    let newline: number;
    while ((newline = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      lineHandler?.(line);
    }
  });
  socket.on("close", () => closeHandler?.());
  socket.on("error", (err: Error) => closeHandler?.(err));

  return {
    ready,
    send(line) {
      socket.write(`${line}\n`);
    },
    onLine(handler) {
      lineHandler = handler;
    },
    onClose(handler) {
      closeHandler = handler;
    },
    close() {
      socket.destroy();
    },
  };
}
