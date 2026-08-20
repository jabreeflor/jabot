/**
 * A code thread — one job in one repo, wrapped in the same chat surface as a
 * bot conversation.
 *
 * The header answers the three questions a running session raises: what am I
 * doing, what engine is doing it, and where has it got to. The harness comes
 * from the *thread*, not from the Code bot, because New Chat can override it
 * per thread (#6).
 */

import { Conversation } from "../components/Conversation";
import { HarnessChip } from "../components/HarnessChip";
import { HostPicker } from "../components/HostPicker";
import { CodeSessionIcon } from "../components/Icon";
import { threadStatus } from "../components/status";
import type {
  HarnessCard,
  HostTarget,
  ThreadSummary,
  TranscriptItem,
} from "../components/types";

export function ThreadView({
  thread,
  harnesses,
  host,
  items,
  onSend,
  onAction,
  onPickHost,
}: {
  thread: ThreadSummary;
  harnesses: readonly HarnessCard[];
  host: HostTarget;
  items: readonly TranscriptItem[];
  onSend: (text: string) => void;
  onAction?: (itemId: string, actionId: string) => void;
  onPickHost?: (hostId: string) => void;
}) {
  const status = threadStatus(thread);

  return (
    <Conversation
      header={
        <div className="chat-head">
          <div className="codeav">
            <CodeSessionIcon />
          </div>
          <h2>{thread.title}</h2>
          <HarnessChip harnessId={thread.harnessId} harnesses={harnesses} />
          <span className="status">{status.label}</span>
          <HostPicker host={host} onPick={onPickHost} />
        </div>
      }
      items={items}
      composerPlaceholder={`Message ${thread.title}`}
      onSend={onSend}
      onAction={onAction}
    />
  );
}
