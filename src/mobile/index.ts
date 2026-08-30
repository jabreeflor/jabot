//! The Mobile Inbox client (#29).
//!
//! A phone is an `approver` device (#19) talking the ordinary host protocol
//! (#8) over an ordinary connection — there is no mobile API. Start at
//! [`MobileSession`]; [`createLineTransport`] is how a device attaches
//! whatever duplex it has to the same frames the desktop uses.

export { askDetail, askTitle, allowOption, parseAskOptions, rejectOption } from "./ask";
export {
  createDeviceCredentials,
  frameHash,
  helloProof,
  verifyHostProof,
} from "./credentials";
export type {
  DeviceCredentialsOptions,
  HelloProofInput,
} from "./credentials";
export type { AskOption } from "./ask";
export {
  askCard,
  EMPTY_INBOX,
  projectInbox,
  withAsk,
  withoutAsk,
} from "./inbox";
export type {
  MobileAsk,
  MobileCard,
  MobileInbox,
  MobileSection,
} from "./inbox";
export { InboxScreen } from "./InboxScreen";
export type { InboxScreenProps } from "./InboxScreen";
export { MobileApp } from "./MobileApp";
export type { MobileAppProps } from "./MobileApp";
export { TranscriptScreen, useThreadTranscript } from "./TranscriptScreen";
export type {
  ThreadTranscript,
  TranscriptScreenProps,
} from "./TranscriptScreen";
export { allowedForApprover, APPROVER_METHODS, checkScope } from "./scope";
export { MobileSession, OutOfScopeError } from "./session";
export type {
  DeviceCredentials,
  InboxListener,
  MobileSessionOptions,
} from "./session";
export { createLineTransport, HostConnectionClosed } from "./transport";
export type { LineChannel, LineTransport } from "./transport";
