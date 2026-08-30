//! The bot editor: icon, name, instructions, tools — and a harness.
//!
//! The harness picker is the one thing the prototype's editor did not have.
//! Decision #6 made every bot an ACP harness session, so "which engine runs
//! this bot" is part of the bot, not a hidden default.
//!
//! The icon is the other. A bot's mark is a colour and its initials until
//! someone gives it a picture, and this is the only screen that can: the
//! upload is normalised here (centre-cropped, scaled, re-encoded) and saved as
//! part of the bot, so the picture survives a restart the same way the name
//! does.
//!
//! Templates are the same fields without an id, so picking one just fills the
//! form; nothing is created until Save.

import { useId, useRef, useState } from "react";

import { Avatar, ImageError, readBotImage } from "./avatar";
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

/**
 * What the chip's tooltip says. The host's own sentence when it has one —
 * "Connected as you@example.com", or the provider's error — because a status
 * word alone ("error") tells the user nothing they can act on.
 */
function chipTitle(tool: ToolOption): string | undefined {
  if (tool.detail) return `${tool.label} — ${tool.detail}`;
  switch (tool.status) {
    case "needs_auth":
      return `${tool.label} is not connected yet`;
    case "connecting":
      return `Waiting for ${tool.label} sign-in`;
    case "missing":
      return `${tool.label} is not installed on this Mac`;
    default:
      return undefined;
  }
}

export function BotEditorModal({
  bot,
  templates,
  tools,
  harnesses,
  error = null,
  onSave,
  onRemove,
  onCancel,
}: {
  /** null = adding a bot. */
  bot: Bot | null;
  templates: readonly BotTemplate[];
  tools: readonly ToolOption[];
  harnesses: readonly HarnessCard[];
  /** Why the last save or remove was refused. The modal stays open holding
      the form: "unknown tool" and "no such harness" are things to fix and
      retry, not reasons to lose what the user typed. */
  error?: string | null;
  onSave: (draft: BotDraft) => void;
  onRemove?: (botId: string) => void;
  onCancel: () => void;
}) {
  const adding = bot === null;
  const templateFieldId = useId();
  const nameId = useId();
  const instructionsId = useId();

  const fileId = useId();
  const fileRef = useRef<HTMLInputElement>(null);

  const [templateId, setTemplateId] = useState("");
  const [name, setName] = useState(bot?.name ?? "");
  const [color, setColor] = useState<BotColor>(bot?.color ?? "b-green");
  const [image, setImage] = useState<string | null>(bot?.image ?? null);
  /** Why the last file could not become an icon. Its own line rather than the
      modal's `error`, which belongs to the save that was refused: picking a
      12MB TIFF is not a failed save, and clearing one should not clear the
      other. */
  const [imageError, setImageError] = useState<string | null>(null);
  const [instructions, setInstructions] = useState(bot?.instructions ?? "");
  const [selectedTools, setSelectedTools] = useState<string[]>(
    bot?.tools ?? [],
  );
  const [harnessId, setHarnessId] = useState(
    bot?.harnessId ?? harnesses[0]?.id ?? "",
  );

  // The icon is untouched on purpose: a pack ships fields, not a picture, so
  // there is nothing to copy — and a user who uploaded one and then browsed
  // the templates should not lose it to a dropdown.
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

  async function pickImage(file: File | undefined) {
    // Cleared whatever happens next, so the reason on screen is always the
    // reason for the file that is on screen.
    setImageError(null);
    if (!file) return;
    try {
      setImage(await readBotImage(file));
    } catch (err) {
      setImageError(
        err instanceof ImageError ? err.message : "Could not read that image",
      );
    }
  }

  function toggleTool(id: string) {
    setSelectedTools((current) =>
      current.includes(id) ? current.filter((t) => t !== id) : [...current, id],
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

      <FieldLabel>ICON</FieldLabel>
      <div className="iconpick">
        {/* The real thing at the size the sidebar draws it, so what the user
            approves here is what they get there. Untitled: the modal already
            says whose bot this is, and a tooltip repeating a half-typed name
            under the cursor is noise. */}
        <Avatar
          name={name || "New bot"}
          color={color}
          image={image}
          titled={false}
        />
        <div className="iconacts">
          <button
            type="button"
            className="btn"
            // The input is what actually opens the picker, but a bare file
            // input cannot be styled into this row and its own button says
            // "Choose File". This is the visible control; the input below is
            // hidden and driven from here.
            onClick={() => fileRef.current?.click()}
          >
            {image ? "Replace image" : "Upload image"}
          </button>
          {image && (
            <button
              type="button"
              className="btn"
              onClick={() => {
                setImage(null);
                setImageError(null);
              }}
            >
              Remove image
            </button>
          )}
          <input
            id={fileId}
            ref={fileRef}
            type="file"
            className="iconfile"
            accept="image/*"
            aria-label="Upload an image"
            onChange={(event) => {
              const file = event.target.files?.[0];
              // Cleared so that picking the *same* file again — after a crop
              // that was not what the user wanted, say — still fires a change.
              event.target.value = "";
              void pickImage(file);
            }}
          />
          <p className="iconhint">
            Square, and scaled down to icon size. Without one, the bot wears its
            colour and initials.
          </p>
        </div>
      </div>

      {imageError && (
        <p className="modal-error" role="alert">
          {imageError}
        </p>
      )}

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
            {/* The bot's own initials in each colour, rather than eight
                anonymous discs: the row is a preview of the mark, and the
                mark is what the colour is for. Drawn even while an image is
                set — the colour is what the bot falls back to if the picture
                is ever removed. */}
            <Avatar name={name || "New bot"} color={swatch} titled={false} />
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
            // The chip's name stays the tool's name — the connection state is
            // a dot and a tooltip, not part of what the button is called.
            data-status={tool.status}
            title={chipTitle(tool)}
            onClick={() => toggleTool(tool.id)}
          >
            {tool.status && <i className="chipdot" aria-hidden="true" />}
            {tool.label}
          </button>
        ))}
      </div>

      {error && (
        <p className="modal-error" role="alert">
          {error}
        </p>
      )}

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
              image,
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
