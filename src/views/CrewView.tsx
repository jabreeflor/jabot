//! Crew management. Every bot is editable and removable except Chief, which is
//! the one seat the product assumes exists (`bots_one_chief` in the schema).
//!
//! Each card shows the bot's harness next to its tools, because after #6 the
//! engine is part of who a bot is — not a preference buried in a settings pane.

import { Avatar } from "../components/avatar";
import { HarnessChip } from "../components/HarnessChip";
import { PlusIcon } from "../components/Icon";
import type { Bot, HarnessCard, ToolOption } from "../components/types";

export function CrewView({
  bots,
  harnesses,
  tools,
  onEdit,
  onAdd,
  onRemove,
  onRunSetup,
}: {
  bots: readonly Bot[];
  harnesses: readonly HarnessCard[];
  tools: readonly ToolOption[];
  onEdit: (botId: string) => void;
  onAdd: () => void;
  onRemove: (botId: string) => void;
  /** Re-run first-run setup — the one in-app way to change your name. */
  onRunSetup?: () => void;
}) {
  return (
    <div className="view">
      <div className="page-scroll">
        <div className="page">
          <div className="page-top">
            <h1>Your Crew</h1>
            <p>Edit, add, or remove bots — each one is yours to customize</p>
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
                  <div>
                    <div className="nm">{bot.name}</div>
                  </div>
                  {bot.isChief && <span className="chief-badge">CHIEF</span>}
                </div>
                <div className="role">{bot.instructions}</div>
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
