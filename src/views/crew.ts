//! The crew, live from the host (#17).
//!
//! The shape of `folders.ts`, for the same reasons. `crew/list` already
//! returns the bots with the templates and host tools beside them, so this is a
//! rename from wire shape to prop shape — no reducer, no second idea of what a
//! bot is. The bot editor saves through `crew/create` / `crew/update`, and what
//! comes back is the record, not an echo of the form.
//!
//! `bots` stays `null` until the host has answered. That is not the same as
//! "no bots": a preview build, a unit test, or a host still starting all have
//! no answer, and the shell keeps rendering its fixtures until one arrives.
//! Unlike folders there is no legitimate empty answer — Chief is seeded and
//! cannot be removed — so an empty array would be a host bug, not a fresh
//! install.
//!
//! The tool chips and the harness picker come from their own owners
//! (`tools/list` from #18, `harness/list` from #13) rather than being copied
//! into the crew payload. Chief's host tools are the exception: they are not
//! MCP and are in no `tools/list` catalog, so `crew/list` carries them.

import { useCallback, useEffect, useState } from "react";

import type {
  BotTemplateView,
  BotView,
  CrewHostToolView,
  CrewListResult,
  HarnessCardView,
  HostClient,
  ToolCardView,
} from "../host";
import {
  BOT_COLORS,
  type Bot,
  type BotColor,
  type BotDraft,
  type BotTemplate,
  type HarnessCard,
  type ToolOption,
} from "../components/types";

export function botRow(bot: BotView): Bot {
  return {
    id: bot.botId,
    name: bot.name,
    color: botColor(bot.color),
    instructions: bot.instructions,
    tools: bot.tools,
    harnessId: bot.harnessId,
    isChief: bot.isChief,
    templateId: bot.templateId ?? null,
  };
}

export function templateRow(template: BotTemplateView): BotTemplate {
  return {
    templateId: template.templateId,
    name: template.name,
    color: botColor(template.color),
    instructions: template.instructions,
    tools: template.tools,
    harnessId: template.harnessId,
  };
}

/** An MCP catalog entry as a chip. The status is the *provider grant's*, which
    is why connecting Gmail lights Calendar too (#18). */
export function toolOption(tool: ToolCardView): ToolOption {
  return { id: tool.id, label: tool.label, status: tool.status, detail: tool.detail };
}

/** Chief's host tools. No `status`: they are the host's own actions, so there
    is nothing to connect and a dot on the chip would be a lie. */
export function hostToolOption(tool: CrewHostToolView): ToolOption {
  return { id: tool.id, label: tool.label, detail: tool.blurb };
}

/** A catalog card for the harness picker. `available` stays undefined — that
    answer comes from the Doctor probe (#13), and "not asked" is not "missing". */
export function harnessCard(card: HarnessCardView): HarnessCard {
  return {
    id: card.id,
    label: card.label,
    blurb: card.blurb,
    accent: card.accent,
    installHint: card.installHint,
  };
}

/** The host keeps `bots.color` inside a closed list, so an unknown value means
    a row written by something that is not this host. Render it rather than
    crash, and pick the colour nothing else on a fresh install uses. */
function botColor(color: string): BotColor {
  return (BOT_COLORS as readonly string[]).includes(color)
    ? (color as BotColor)
    : "b-green";
}

export interface Crew {
  /** `null` until the host answers. Chief is always in a real answer. */
  bots: Bot[] | null;
  templates: BotTemplate[] | null;
  /** The MCP chips, with today's connection status. */
  tools: ToolOption[] | null;
  /** Chief's host tools, so the grid can name them instead of printing ids. */
  hostTools: ToolOption[] | null;
  harnesses: HarnessCard[] | null;
  error: string | null;
  reload: () => void;
  /** Create when `botId` is null, else patch. Resolves with the saved record
      or throws the host's error — the editor needs to be able to say *why* a
      save was refused, and "unknown tool" and "no such harness" are different
      things to fix. */
  save: (botId: string | null, draft: BotDraft) => Promise<Bot>;
  /** Throws `CHIEF_REQUIRED` for Chief. The grid hides the button, but the
      host is the one that guarantees it. */
  remove: (botId: string) => Promise<void>;
}

export function useCrew(client: HostClient | null): Crew {
  const [crew, setCrew] = useState<CrewListResult | null>(null);
  const [tools, setTools] = useState<ToolOption[] | null>(null);
  const [harnesses, setHarnesses] = useState<HarnessCard[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Bumped to re-run the load: a save or a remove changes the crew, and both
  // happen outside this effect.
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    // Guarded as a whole, method lookup included: a transport that predates
    // these methods should leave the shell on its fixtures rather than take
    // the render down.
    (async () =>
      Promise.all([client.listCrew(), client.listTools(), client.listHarnesses()]))()
      .then(([listed, toolList, harnessList]) => {
        if (cancelled) return;
        setCrew(listed);
        setTools(toolList.tools.map(toolOption));
        setHarnesses(harnessList.harnesses.map(harnessCard));
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [client, generation]);

  const reload = useCallback(() => setGeneration((n) => n + 1), []);

  const save = useCallback(
    async (botId: string | null, draft: BotDraft) => {
      if (!client) throw new Error("No host connection.");
      const saved = botId
        ? await client.updateBot({ botId, ...draft })
        : await client.createBot({
            ...draft,
            templateId: draft.templateId ?? undefined,
          });
      reload();
      return botRow(saved);
    },
    [client, reload],
  );

  const remove = useCallback(
    async (botId: string) => {
      if (!client) throw new Error("No host connection.");
      await client.removeBot({ botId });
      reload();
    },
    [client, reload],
  );

  return {
    bots: crew ? crew.bots.map(botRow) : null,
    templates: crew ? crew.templates.map(templateRow) : null,
    tools,
    hostTools: crew ? crew.hostTools.map(hostToolOption) : null,
    harnesses,
    error,
    reload,
    save,
    remove,
  };
}
