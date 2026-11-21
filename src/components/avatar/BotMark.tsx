//! The built-in bot drawing: an open-bot-avatar mounted into one span.
//! The engine owns volume, blinking and state motion and shares a single
//! animation loop across every mark on the page; React only owns the span.

import { useEffect, useRef } from "react";
import type { AvatarState as EngineState } from "open-bot-avatar";
import { mountAvatar, type MountedAvatar } from "open-bot-avatar/dom";
import type { BotColor } from "../types";
import { botIcon } from "./bots";
import type { AvatarState } from "./state";

/** The app's four ring states, in the engine's vocabulary. */
const ENGINE_STATE: Record<AvatarState, EngineState> = {
  idle: "idle",
  running: "working",
  waiting: "waiting",
  failed: "blocked",
};

export function BotMark({
  color,
  state = "idle",
}: {
  color: BotColor;
  state?: AvatarState;
}) {
  const bot = botIcon(color);
  const host = useRef<HTMLSpanElement>(null);
  const avatar = useRef<MountedAvatar | null>(null);

  // Mounted once per element; every later change goes through `update`, which
  // only redraws what changed. The engine replaces the span's children, so
  // nothing React renders may live inside it.
  useEffect(() => {
    const mounted = mountAvatar(host.current!, {
      definition: bot.definition,
      color: bot.color,
      state: ENGINE_STATE[state],
      // Decorative: the surrounding avatar carries the name when it needs one.
      label: "",
    });
    avatar.current = mounted;
    return () => {
      mounted.destroy();
      avatar.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- later changes go through update()
  }, []);

  useEffect(() => {
    avatar.current?.update({
      definition: bot.definition,
      color: bot.color,
      state: ENGINE_STATE[state],
    });
  }, [bot, state]);

  return (
    <span
      ref={host}
      className="bot-mark"
      data-character={bot.id}
      aria-hidden="true"
    />
  );
}
