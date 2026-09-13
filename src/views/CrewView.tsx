//! Crew management. Every bot is editable and removable except Chief, which is
//! the one seat the product assumes exists (`bots_one_chief` in the schema).
//!
//! Each card shows the bot's harness next to its tools, because after #6 the
//! engine is part of who a bot is — not a preference buried in a settings pane.

import { Avatar } from "../components/avatar";
import { HarnessChip } from "../components/HarnessChip";
import { PlusIcon } from "../components/Icon";
import type { BotDraftView } from "../host";
import type { Bot, HarnessCard, ToolOption } from "../components/types";

export function CrewView({
  bots,
  harnesses,
  tools,
  drafts = [],
  error = null,
  onEdit,
  onAdd,
  onRemove,
  onReviewDraft,
  onRunSetup,
}: {
  bots: readonly Bot[];
  harnesses: readonly HarnessCard[];
  tools: readonly ToolOption[];
  drafts?: readonly BotDraftView[];
  /** Why the last chat-first create was refused. */
  error?: string | null;
  onEdit: (botId: string) => void;
  onAdd: () => void;
  onRemove: (botId: string) => void;
  onReviewDraft?: (draftId: string) => void;
  /** Re-run first-run setup — also offered from Settings. */
  onRunSetup?: () => void;
}) {
  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page">
          <div className="page-top">
            <h1>Your Crew</h1>
            <p>
              Each bot is a chat. Add one by talking — Edit opens advanced
              settings.
            </p>
            {onRunSetup && (
              <button
                type="button"
                className="btn setup-again"
                onClick={onRunSetup}
              >
                Run setup again
              </button>
            )}
          </div>

          {error && (
            <p className="modal-error" role="alert">
              {error}
            </p>
          )}

          {drafts.length > 0 && (
            <div className="pending-drafts" aria-label="Pending bot drafts">
              <h2>Pending proposals</h2>
              <p>
                Close does not dismiss. Review and Save to add a crew member.
              </p>
              <ul>
                {drafts.map((draft) => (
                  <li key={draft.draftId}>
                    <span>
                      {draft.name}
                      {draft.sourceBotName
                        ? ` — proposed by ${draft.sourceBotName}`
                        : ""}
                      {draft.status === "stale" ? " (stale)" : ""}
                    </span>
                    {onReviewDraft && (
                      <button
                        type="button"
                        className="btn"
                        onClick={() => onReviewDraft(draft.draftId)}
                      >
                        Review
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}

          <div className="crew-grid">
            {bots.map((bot) => (
              <div className="crew-card" key={bot.id}>
                <div className="r1">
                  <Avatar
                    name={bot.name}
                    color={bot.color}
                    image={bot.image}
                    unread={bot.unread}
                  />
                  <div className="crew-identity">
                    {bot.instructions && (
                      <div className="role">{bot.instructions}</div>
                    )}
                    <div className="nm">{bot.name}</div>
                  </div>
                  {bot.isChief && <span className="chief-badge">CHIEF</span>}
                </div>
                <div className="tools">
                  {bot.tools.map((toolId) => (
                    <span className="minichip" key={toolId}>
                      {toolLabel(tools, toolId)}
                    </span>
                  ))}
                  <HarnessChip
                    harnessId={bot.harnessId}
                    harnesses={harnesses}
                  />
                </div>
                <div className="acts">
                  <button
                    type="button"
                    className="btn"
                    onClick={() => onEdit(bot.id)}
                  >
                    Edit
                  </button>
                  {!bot.isChief && (
                    <button
                      type="button"
                      className="btn danger"
                      onClick={() => onRemove(bot.id)}
                    >
                      Remove
                    </button>
                  )}
                </div>
              </div>
            ))}

            <button type="button" className="add-card" onClick={onAdd}>
              <span className="big" aria-hidden="true">
                <PlusIcon />
              </span>
              Add a bot
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

/** Chief's host tools are not in the MCP catalog, so an unknown id shows raw. */
function toolLabel(tools: readonly ToolOption[], id: string): string {
  return tools.find((tool) => tool.id === id)?.label ?? id;
}
