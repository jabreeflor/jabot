//! The phone, as a program: connect, read the Inbox, answer the question.
//!
//! This is a client of the *same* host API the desktop uses — same methods,
//! same frames, same `HostClient` — narrowed to what an `approver` device is
//! allowed to do (#19) and shaped around the one job that justifies carrying
//! JaBot in a pocket: something is blocked on a human, and the human is not at
//! their Mac.
//!
//! Three decisions are worth explaining.
//!
//! **The credential is injected, not implemented here.** `host/hello` from a
//! paired device carries an HMAC under the token its pairing derived, with a
//! counter that must strictly climb. Which secure enclave, keystore or file
//! that token lives in is a device question, not a protocol question, so this
//! takes a [`DeviceCredentials`] and never sees the token. The derivation a
//! real implementation has to match is documented on `PairingClaimParams` and
//! implemented, independently, in `tests/support/pairing.ts`.
//!
//! **Notifications are applied synchronously; refreshing is a separate act.**
//! `permission/ask` carries everything a card needs, so the phone can draw it
//! without a round trip — which matters when the notification is what woke the
//! device. `permission/resolved` removes the card whoever answered it, so the
//! Mac answering first makes the phone's buttons disappear rather than leaving
//! a second answer one tap away.
//!
//! **Client-side scope is a courtesy, not a boundary.** [`assertAllowed`]
//! keeps this client from offering what the host would refuse; the host refuses
//! it regardless, on every request, from the `paired_devices` row.

import { HostClient, type HostTransport } from "../host/client";
import {
  INBOX_LIST,
  PERMISSION_ASK,
  PERMISSION_PENDING,
  PERMISSION_REPLY,
  PERMISSION_RESOLVED,
  SESSION_CANCEL,
  THREAD_TRANSCRIPT,
  type DeviceAuth,
  type HelloResult,
  type JsonRpcNotification,
  type PermissionAskParams,
  type PermissionReplyResult,
  type PermissionResolvedParams,
  type ThreadTranscriptResult,
} from "../host/protocol";
import { askTitle } from "./ask";
import {
  askCard,
  EMPTY_INBOX,
  projectInbox,
  withAsk,
  withoutAsk,
  type MobileInbox,
} from "./inbox";
import { allowedForApprover } from "./scope";

/**
 * This device's identity and its ability to prove it.
 *
 * `signHello` owns the counter as well as the key: it must return a strictly
 * larger `counter` than any this device has ever sent to this host, because
 * the host refuses one it has already accepted. That is what makes a captured
 * frame worthless, and it is why the counter cannot live in this class — it
 * has to survive the app being killed.
 */
export interface DeviceCredentials {
  deviceId: string;
  name?: string;
  signHello(): Promise<DeviceAuth> | DeviceAuth;
}

export interface MobileSessionOptions {
  transport: HostTransport;
  /** Absent for the console that spawned the host; a phone always has one. */
  credentials?: DeviceCredentials;
  /** Injected so a card's timestamp is not a moving target in tests. */
  now?: () => Date;
}

export type InboxListener = (inbox: MobileInbox) => void;

/** Refused before it reached the wire, because this role cannot call it. */
export class OutOfScopeError extends Error {
  constructor(readonly method: string) {
    super(`${method} is not something an approver device may call`);
    this.name = "OutOfScopeError";
  }
}

export class MobileSession {
  private readonly client: HostClient;
  private readonly credentials?: DeviceCredentials;
  private readonly now: () => Date;
  private readonly listeners = new Set<InboxListener>();
  private unsubscribe: (() => void) | null = null;
  private snapshot: MobileInbox = EMPTY_INBOX;
  private hello: HelloResult | null = null;

  constructor(options: MobileSessionOptions) {
    this.client = new HostClient(options.transport);
    this.credentials = options.credentials;
    this.now = options.now ?? (() => new Date());
  }

  /** Say hello as this device and start listening. Returns the host's answer. */
  async connect(): Promise<HelloResult> {
    await this.client.connect();
    this.unsubscribe = this.client.onNotification((n) => this.apply(n));
    const auth = this.credentials
      ? await this.credentials.signHello()
      : undefined;
    this.hello = await this.client.hello({
      device: this.credentials
        ? { deviceId: this.credentials.deviceId, name: this.credentials.name }
        : undefined,
      auth,
    });
    return this.hello;
  }

  disconnect(): void {
    this.unsubscribe?.();
    this.unsubscribe = null;
    this.listeners.clear();
    this.client.disconnect();
  }

  /** The device the *host* bound this connection to — role included. */
  get device() {
    return this.hello?.device ?? null;
  }

  /**
   * What the host says this device may call.
   *
   * Falls back to `methods` for a host that predates `scopedMethods`; a host
   * that does not answer the question has not said "everything", it has said
   * nothing, and the local allowlist still applies on top.
   */
  get scopedMethods(): readonly string[] {
    return this.hello?.scopedMethods?.length
      ? this.hello.scopedMethods
      : (this.hello?.methods ?? []);
  }

  get inbox(): MobileInbox {
    return this.snapshot;
  }

  /** Subscribe to the projection. Fires immediately with what is known now. */
  onInbox(listener: InboxListener): () => void {
    this.listeners.add(listener);
    listener(this.snapshot);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /** Pull the whole screen: the Inbox projection plus every outstanding ask. */
  async refresh(): Promise<MobileInbox> {
    this.assertAllowed(INBOX_LIST);
    this.assertAllowed(PERMISSION_PENDING);
    const [inbox, pending] = await Promise.all([
      this.client.inbox(),
      this.client.pendingPermissions(),
    ]);
    return this.publish(projectInbox(inbox, pending));
  }

  /**
   * Answer an ask from this device.
   *
   * `deviceId` is sent because the protocol asks for it, but the host stamps
   * the record with the device *this connection* said hello as and refuses a
   * claim to be anybody else — so what shows up in `permission/resolved` on
   * the Mac is the phone, whatever this client says.
   */
  async answer(
    requestId: string,
    optionId: string,
  ): Promise<PermissionReplyResult> {
    return this.reply({ requestId, optionId });
  }

  /** Decline without choosing one of the agent's options (#20's `cancelled`). */
  async decline(requestId: string): Promise<PermissionReplyResult> {
    return this.reply({ requestId, cancelled: true });
  }

  /** Stop the turn. The narrowest destructive thing an approver may do. */
  async cancelThread(threadId: string): Promise<void> {
    this.assertAllowed(SESSION_CANCEL);
    await this.client.cancel({ threadId });
  }

  /** Enough of the thread to know what you are answering — never more. */
  async transcript(
    threadId: string,
    limit = 40,
  ): Promise<ThreadTranscriptResult> {
    this.assertAllowed(THREAD_TRANSCRIPT);
    return this.client.threadTranscript({ threadId, limit });
  }

  private async reply(params: {
    requestId: string;
    optionId?: string;
    cancelled?: boolean;
  }): Promise<PermissionReplyResult> {
    this.assertAllowed(PERMISSION_REPLY);
    const deviceId = this.device?.deviceId;
    if (!deviceId) throw new Error("answer before the host has said hello");
    const result = await this.client.replyPermission({ ...params, deviceId });
    // Optimistic, and safe to be: the host is idempotent, so a card removed
    // here and a `permission/resolved` arriving a moment later agree.
    this.publish(withoutAsk(this.snapshot, params.requestId));
    return result;
  }

  /**
   * Refuse a call this device may not make, before it reaches the wire.
   *
   * Both halves matter. The local allowlist is what lets a screen decide what
   * to draw with no host round trip. The host's `scopedMethods` is what makes
   * that decision *current*: a role narrowed on the Rust side, or a device
   * downgraded since this client was written, shows up here as a refusal with
   * a name on it rather than as a `DEVICE_SCOPE` error from a button that
   * should never have existed.
   */
  private assertAllowed(method: string): void {
    if (!allowedForApprover(method)) throw new OutOfScopeError(method);
    const scoped = this.scopedMethods;
    if (scoped.length > 0 && !scoped.includes(method)) {
      throw new OutOfScopeError(method);
    }
  }

  /** Host-initiated frames. Everything here is applied without a round trip. */
  private apply(notification: JsonRpcNotification): void {
    if (notification.method === PERMISSION_ASK) {
      const ask = notification.params as PermissionAskParams;
      this.publish(
        withAsk(
          this.snapshot,
          askCard({
            requestId: ask.requestId,
            threadId: ask.threadId,
            title: askTitle(ask.subject),
            subject: ask.subject,
            options: ask.options,
            createdAt: this.now().toISOString(),
            stale: false,
          }),
        ),
      );
      return;
    }
    if (notification.method === PERMISSION_RESOLVED) {
      const resolved = notification.params as PermissionResolvedParams;
      this.publish(withoutAsk(this.snapshot, resolved.requestId));
    }
  }

  private publish(inbox: MobileInbox): MobileInbox {
    this.snapshot = inbox;
    for (const listener of this.listeners) listener(inbox);
    return inbox;
  }
}
