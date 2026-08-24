//! Crew management. Every bot is editable and removable except Chief, which is
//! the one seat the product assumes exists (`bots_one_chief` in the schema).
//!
//! Each card shows the bot's harness next to its tools, because after #6 the
//! engine is part of who a bot is — not a preference buried in a settings pane.

import { useId } from "react";

import { Avatar, CREW_STYLES, useCrewStyleChoice } from "../components/avatar";
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
            <CrewStyleSwitch bots={bots} />
          </div>

          <div className="crew-grid">
            {bots.map((bot) => (
              <div className="crew-card" key={bot.id}>
                <div className="r1">
                  <Avatar
                    id={bot.id}
                    name={bot.name}
                    color={bot.color}
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

/**
 * Six drawings of one bot, and clicking one changes every avatar in the app.
 *
 * Temporary, and the copy says so out loud rather than only in a comment: #44
 * produced five answers that all looked reasonable on a prototype page, and
 * the only way to tell them apart is to live with one in the sidebar for a
 * few days. The follow-up that names a winner deletes this component, its
 * rules in crew.css, `Avatar`'s `crewStyle` prop and the four losing
 * renderers.
 *
 * The preview is a real bot rather than a stock face because that is the
 * whole comparison: the drawings deal a mark from the id, so a made-up id
 * would be showing six creatures nobody is going to see.
 */
function CrewStyleSwitch({ bots }: { bots: readonly Bot[] }) {
  const { style, setStyle } = useCrewStyleChoice();
  const headingId = useId();
  const model = bots.find((bot) => bot.isChief) ?? bots[0];
  if (!model) return null;

  return (
    <section className="style-switch" aria-labelledby={headingId}>
      <div className="page-section" id={headingId}>
        CREW STYLE — TEMPORARY
      </div>
      <p className="style-switch-note">
        Five answers to #44 plus what ships today, drawn as {model.name}. Pick
        one and live with it; this row goes away once the crew is chosen.
      </p>
      <div className="style-row" role="group" aria-label="Crew style">
        {CREW_STYLES.map((option) => (
          <button
            key={option.id}
            type="button"
            className="style-opt"
            aria-pressed={style === option.id}
            title={option.blurb}
            onClick={() => setStyle(option.id)}
          >
            {/* Decorative here, unusually for an avatar: the button is
                named for the style, and the drawing is the sample rather
                than the subject. It also keeps the avatar's own `title` out
                of engines whose name-from-content picks up a descendant
                tooltip — jsdom's does not, so the test below cannot pin
                this and the attribute has to be deliberate. */}
            <span aria-hidden="true">
              <Avatar
                id={model.id}
                name={model.name}
                color={model.color}
                crewStyle={option.id}
              />
            </span>
            {option.label}
          </button>
        ))}
      </div>
    </section>
  );
}

/** Chief's host tools are not in the MCP catalog, so an unknown id shows raw. */
function toolLabel(tools: readonly ToolOption[], id: string): string {
  return tools.find((tool) => tool.id === id)?.label ?? id;
}
