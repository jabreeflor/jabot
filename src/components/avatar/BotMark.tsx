//! The built-in bot drawing: one monochrome silhouette per persisted color id.
//! Motion lives in CSS; this file only names the parts the keyframes target.

import type { BotColor } from "../types";
import { botIcon } from "./bots";

export function BotMark({ color }: { color: BotColor }) {
  const bot = botIcon(color);
  return (
    <svg
      className="bot-mark"
      data-character={bot.id}
      viewBox="4 3 40 40"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <g className="bot-pose">
        <g className="bot-antenna">
          <path d={bot.antenna} />
          <circle cx="24" cy={bot.tipY} r="2" />
        </g>
        <path className="bot-head" d={bot.body} />
        <g className="bot-face">
          <path className="bot-eyes" d={bot.eyes} />
          <path
            className="bot-happy"
            d="M18.5 26Q20 23.5 21.5 26M26.5 26Q28 23.5 29.5 26"
          />
          <path
            className="bot-error"
            d="M18 24L22 28M22 24L18 28M26 24L30 28M30 24L26 28"
          />
        </g>
      </g>
    </svg>
  );
}
