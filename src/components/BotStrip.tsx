//! The crew as faces. Chief gets its own wider row because it is the one bot you
//! talk to about the others; the rest sit in a three-up grid with the Crew tile
//! last, so "manage the crew" is where the crew is rather than in a menu.
//!
//! Every bot here has one standing thread (#6) — clicking a face opens it.

import { Avatar, CrewAvatar } from "./avatar";
import type { Bot, Selection } from "./types";

export function BotStrip({
  bots,
  selection,
  onSelectBot,
  onOpenCrew,
}: {
  bots: readonly Bot[];
  selection: Selection;
  onSelectBot: (botId: string) => void;
  onOpenCrew: () => void;
}) {
  const chief = bots.find((bot) => bot.isChief);
  const crew = bots.filter((bot) => !bot.isChief);
  const selectedBotId = selection.view === "bot" ? selection.botId : null;

  return (
    <>
      {chief && (
        <div className="chief-row">
          <BotTile
            bot={chief}
            selected={selectedBotId === chief.id}
            onSelect={onSelectBot}
          />
        </div>
      )}
      <div className="bot-strip">
        {crew.map((bot) => (
          <BotTile
            key={bot.id}
            bot={bot}
            selected={selectedBotId === bot.id}
            onSelect={onSelectBot}
          />
        ))}
        <button
          type="button"
          className="bot-tile"
          aria-current={selection.view === "crew"}
          onClick={onOpenCrew}
        >
          <CrewAvatar />
          <small>Crew</small>
        </button>
      </div>
    </>
  );
}

function BotTile({
  bot,
  selected,
  onSelect,
}: {
  bot: Bot;
  selected: boolean;
  onSelect: (botId: string) => void;
}) {
  return (
    <button
      type="button"
      className="bot-tile"
      aria-current={selected}
      onClick={() => onSelect(bot.id)}
    >
      <Avatar id={bot.id} name={bot.name} color={bot.color} unread={bot.unread} />
      <small>{bot.name}</small>
    </button>
  );
}
