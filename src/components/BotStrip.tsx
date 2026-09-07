//! The crew as chat rows. One bot per line: face, name, and the last thing
//! said in its standing thread — the list a messenger has, because that is
//! what these are (#6: every bot has one conversation that lives as long as it
//! does, and clicking a face opens it).
//!
//! It was a grid of faces, three across, for as long as the crew was small
//! enough to choose by recognition alone. Recognition still picks the *bot*;
//! it says nothing about the *conversation*, so a face on its own could not
//! answer the question the sidebar is actually asked — which of these is
//! waiting on me, and about what. A row has somewhere to put that answer.
//!
//! Chief stays first and keeps a wider face, because it is the one bot you
//! talk to about the others. The Crew row is last, so "manage the crew" is
//! where the crew is rather than in a menu.

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
    <div className="bot-list">
      {chief && (
        <BotRow
          bot={chief}
          chief
          selected={selectedBotId === chief.id}
          onSelect={onSelectBot}
        />
      )}
      {crew.map((bot) => (
        <BotRow
          key={bot.id}
          bot={bot}
          selected={selectedBotId === bot.id}
          onSelect={onSelectBot}
        />
      ))}
      <button
        type="button"
        className="bot-row"
        aria-current={selection.view === "crew"}
        onClick={onOpenCrew}
      >
        <CrewAvatar />
        <span className="who">
          <span className="nm">Crew</span>
          <span className="say">Add, edit, or remove bots</span>
        </span>
      </button>
    </div>
  );
}

function BotRow({
  bot,
  chief = false,
  selected,
  onSelect,
}: {
  bot: Bot;
  chief?: boolean;
  selected: boolean;
  onSelect: (botId: string) => void;
}) {
  return (
    <button
      type="button"
      className={chief ? "bot-row chief" : "bot-row"}
      aria-current={selected}
      onClick={() => onSelect(bot.id)}
    >
      <Avatar
        name={bot.name}
        color={bot.color}
        image={bot.image}
        unread={bot.unread}
      />
      <span className="who">
        <span className="nm">{bot.name}</span>
        {/* The chat's own words when there are any. A bot nobody has talked to
            yet has no last line, so the row says what the bot is *for* — the
            only other true thing there is to say about a conversation that has
            not started — and marks it as the standing description rather than
            something that was said. */}
        <span className={bot.preview ? "say" : "say persona"}>
          {bot.preview ?? bot.instructions}
        </span>
      </span>
    </button>
  );
}
