/**
 * The bot editor: name, colour, instructions, tools — and a harness.
 *
 * The harness picker is the one thing the prototype's editor did not have.
 * Decision #6 made every bot an ACP harness session, so "which engine runs
 * this bot" is part of the bot, not a hidden default.
 *
 * Templates are the same fields without an id, so picking one just fills the
 * form; nothing is created until Save.
 */

import { useId, useState } from "react";

import { Blob } from "./Blob";
import { FieldLabel, Modal } from "./Modal";
import { HarnessPicker } from "./HarnessPicker";
import {
  BOT_COLORS,
  type Bot,
  type BotColor,
  type BotDraft,
  type BotTemplate,
  type HarnessCard,
  type ToolOption,
} from "./types";

export function BotEditorModal({
  bot,
  templates,
  tools,
  harnesses,
  onSave,
  onRemove,
  onCancel,
}: {
  /** null = adding a bot. */
  bot: Bot | null;
  templates: readonly BotTemplate[];
  tools: readonly ToolOption[];
  harnesses: readonly HarnessCard[];
  onSave: (draft: BotDraft) => void;
  onRemove?: (botId: string) => void;
  onCancel: () => void;
}) {
  const adding = bot === null;
  const templateFieldId = useId();
  const nameId = useId();
  const instructionsId = useId();

  const [templateId, setTemplateId] = useState("");
  const [name, setName] = useState(bot?.name ?? "");
  const [color, setColor] = useState<BotColor>(bot?.color ?? "b-green");
  const [instructions, setInstructions] = useState(bot?.instructions ?? "");
  const [selectedTools, setSelectedTools] = useState<string[]>(bot?.tools ?? []);
  const [harnessId, setHarnessId] = useState(
    bot?.harnessId ?? harnesses[0]?.id ?? "",
  );

  function applyTemplate(id: string) {
    setTemplateId(id);
    const template = templates.find((t) => t.templateId === id);
    if (!template) return;
    setName(template.name);
    setColor(template.color);
    setInstructions(template.instructions);
    setSelectedTools([...template.tools]);
    setHarnessId(template.harnessId);
  }

  function toggleTool(id: string) {
    setSelectedTools((current) =>
      current.includes(id)
        ? current.filter((t) => t !== id)
        : [...current, id],
    );
  }

  return (
    <Modal
      title={adding ? "Add a bot" : `Customize ${bot.name}`}
      onClose={onCancel}
    >
      {adding && (
        <>
          <FieldLabel htmlFor={templateFieldId}>
            START FROM A TEMPLATE
          </FieldLabel>
          <select
            id={templateFieldId}
            value={templateId}
            onChange={(event) => applyTemplate(event.target.value)}
          >
            <option value="">Blank bot</option>
            {templates.map((template) => (
              <option key={template.templateId} value={template.templateId}>
                {template.name}
              </option>
            ))}
          </select>
        </>
      )}

      <FieldLabel htmlFor={nameId}>NAME</FieldLabel>
      <input
        id={nameId}
        type="text"
        value={name}
        placeholder="e.g. Expense Manager"
        onChange={(event) => setName(event.target.value)}
      />

      <FieldLabel>COLOR</FieldLabel>
      <div className="swatches" role="group" aria-label="Color">
        {BOT_COLORS.map((swatch) => (
          <button
            key={swatch}
            type="button"
            className="swatch"
            aria-label={swatch.replace("b-", "")}
            aria-pressed={color === swatch}
            onClick={() => setColor(swatch)}
          >
            <Blob color={swatch} />
          </button>
        ))}
      </div>

      <FieldLabel htmlFor={instructionsId}>WHAT IT DOES</FieldLabel>
      <textarea
        id={instructionsId}
        value={instructions}
        placeholder="Instructions, tone, boundaries — how this bot should work for you"
        onChange={(event) => setInstructions(event.target.value)}
      />

      <FieldLabel>HARNESS</FieldLabel>
      <HarnessPicker
        harnesses={harnesses}
        value={harnessId}
        onChange={setHarnessId}
        label="Harness"
      />

      <FieldLabel>TOOLS</FieldLabel>
      <div className="toolrow" role="group" aria-label="Tools">
        {tools.map((tool) => (
          <button
            key={tool.id}
            type="button"
            className="toolchip"
            aria-pressed={selectedTools.includes(tool.id)}
            onClick={() => toggleTool(tool.id)}
          >
            {tool.label}
          </button>
        ))}
      </div>

      <div className="macts">
        {!adding && !bot.isChief && onRemove && (
          <button
            type="button"
            className="btn danger"
            onClick={() => onRemove(bot.id)}
          >
            Remove
          </button>
        )}
        <button type="button" className="btn" onClick={onCancel}>
          Cancel
        </button>
        <button
          type="button"
          className="btn primary"
          onClick={() =>
            onSave({
              name: name.trim() || "Unnamed bot",
              color,
              instructions: instructions.trim(),
              tools: selectedTools,
              harnessId,
              templateId: adding ? templateId || null : bot.templateId,
            })
          }
        >
          Save
        </button>
      </div>
    </Modal>
  );
}
