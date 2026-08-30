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

import { useCallback, useEffect, useMemo, useState } from "react";

import type {
  BotTemplateView,
  BotView,
  CrewHostToolView,
  CrewListResult,
  HarnessCardView,
  HarnessReport,
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
    image: bot.image ?? null,
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
    answer comes from the Doctor probe (#13), and "not asked" is not "missing".
    `withReadiness` is what fills it in once the probe answers. */
export function harnessCard(card: HarnessCardView): HarnessCard {
  return {
    id: card.id,
    label: card.label,
    blurb: card.blurb,
    accent: card.accent,
    installHint: card.installHint,
  };
}

/**
 * Fold the Doctor's answer into the catalog cards.
 *
 * `harness/list` is the cheap call — it reads the catalog and probes nothing,
 * which is what lets the picker open without waiting on a vendor CLI.
 * `harness/doctor` is the expensive one, and it is the only thing that knows
 * whether the engine on a card can actually be run. So the list paints first
 * and this narrows it afterwards.
 *
 * A card the report set does not mention keeps `available: undefined`. That is
 * deliberate and is not the same as `false`: the picker greys out a card it
 * has been told is missing, and a card nobody asked about has not earned that.
 *
 * `remedy` wins over `installHint` because it is the more specific of the two —
 * the Doctor writes it knowing what it actually found on this machine, where
 * `installHint` is a constant in the catalog.
 */
export function withReadiness(
  cards: readonly HarnessCard[],
  reports: readonly HarnessReport[],
): HarnessCard[] {
  const byId = new Map(reports.map((report) => [report.id, report]));
  return cards.map((card) => {
    const report = byId.get(card.id);
    if (!report) return card;
    return {
      ...card,
      available: report.ready,
      installHint: report.remedy ?? report.installHint ?? card.installHint,
    };
  });
}

/**
 * The editor's icon field, in the shape `crew/update` reads it.
 *
 * Three states have to survive the trip and JSON only carries two of them
 * usefully: a key that is absent means "leave the picture alone", and
 * `undefined` is how a JSON body spells absent. So "take the picture away" has
 * to be a value, and the empty string is the one the host already uses for
 * clearing a text column.
 */
function patchImage(image: string | null | undefined): string | undefined {
  if (image === undefined) return undefined;
  return image ?? "";
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
  // Held apart from `harnesses` rather than folded into it on arrival, because
  // the two calls race and either can land first: the catalog is three fields
  // out of SQLite, the probe is vendor CLIs on disk, and neither ordering is
  // guaranteed. Merging on arrival meant whichever came second won and the
  // other was silently dropped. Kept separate, the merge below is the same
  // answer whatever the order.
  const [reports, setReports] = useState<readonly HarnessReport[] | null>(null);
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
    // The Doctor runs on its own, deliberately, and its failure is swallowed.
    //
    // Two reasons it is not in the Promise.all above. It probes vendor CLIs on
    // disk, so it is the slow one, and joining it would make the picker wait on
    // it for no reason — the blurbs are worth painting immediately. And it is
    // the one call here that is allowed to fail: a probe that times out, or a
    // transport too old to know the method, must leave the cards saying what
    // they can rather than replacing the crew view with an error. Not knowing
    // whether an engine is installed is a smaller loss than not showing it.
    if (typeof client.harnessDoctor === "function") {
      client
        .harnessDoctor({})
        .then((doctor) => {
          if (cancelled) return;
          setReports(doctor.reports);
        })
        .catch(() => {
          // Deliberately silent: see above. The cards keep `available`
          // undefined, which the picker renders as the blurb.
        });
    }
    return () => {
      cancelled = true;
    };
  }, [client, generation]);

  const reload = useCallback(() => setGeneration((n) => n + 1), []);

  const save = useCallback(
    async (botId: string | null, draft: BotDraft) => {
      if (!client) throw new Error("No host connection.");
      const saved = botId
        ? await client.updateBot({
            botId,
            ...draft,
            image: patchImage(draft.image),
          })
        : await client.createBot({
            ...draft,
            templateId: draft.templateId ?? undefined,
            // No three-way spelling on create: a new bot has no icon to keep,
            // so "none" and "leave it alone" are the same instruction.
            image: draft.image ?? undefined,
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

  // Memoised so the picker is handed the same array identity between renders;
  // rebuilding it every render would defeat any memo below it.
  const readyHarnesses = useMemo(
    () => (harnesses && reports ? withReadiness(harnesses, reports) : harnesses),
    [harnesses, reports],
  );

  return {
    bots: crew ? crew.bots.map(botRow) : null,
    templates: crew ? crew.templates.map(templateRow) : null,
    tools,
    hostTools: crew ? crew.hostTools.map(hostToolOption) : null,
    harnesses: readyHarnesses,
    error,
    reload,
    save,
    remove,
  };
}
