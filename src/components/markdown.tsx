//! Markdown inside an agent bubble, in about a hundred lines and no dependency.
//!
//! An agent replies in markdown because that is what agents do, and the bubble
//! rendered it as a text node — so a fenced diff arrived as a wall of
//! backticks and a bulleted plan arrived as a paragraph starting with a
//! hyphen. This is the smallest thing that fixes that without pulling a parser
//! and a sanitiser into a desktop app to render text it produced itself.
//!
//! **React nodes, never `dangerouslySetInnerHTML`.** Every string here reaches
//! the DOM as a text child, so there is no markup path at all — not a
//! sanitiser that has to be right, an escape hatch that does not exist. An
//! agent that echoes a user's `<script>` back is drawing five characters.
//!
//! Deliberately not CommonMark. Block quotes, links, headings, nested lists and
//! setext anything are all absent, because each is a rule that can misfire on
//! prose an agent wrote for a human. What is here is what a coding agent's
//! replies are actually made of: fences, lists, GFM pipe tables, `code`, and
//! emphasis. A table needs its `| --- |` delimiter row right under the header,
//! so a sentence with a stray pipe in it never becomes one (#305).
//! Anything unrecognised is left as the characters that were typed, which is
//! exactly what shipped before.
//!
//! The `user` bubble stays literal. A person's asterisks are their own, and a
//! renderer that ate them would be editing what somebody said.

import type { ReactNode } from "react";

/** A fence's own line: ``` or ~~~, optionally with a language after it. */
const FENCE = /^(\s*)(```|~~~)(.*)$/;
/** `- `, `* ` or `+ ` — a bullet, not a horizontal rule or a bare hyphen. */
const BULLET = /^(\s*)[-*+][ \t]+(.*)$/;
/** `1. ` or `1) `. The number is not preserved: `<ol>` counts for itself. */
const ORDERED = /^(\s*)\d{1,9}[.)][ \t]+(.*)$/;
/** A GFM delimiter row: `| --- | :-: |`, at least one cell, pipes optional at
    the ends. */
const TABLE_DELIMITER = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

/**
 * One agent message, as React nodes.
 *
 * A block pass over the lines, then an inline pass on everything that is not
 * inside a fence — code is the one place where `**` is two asterisks.
 */
export function renderMarkdown(text: string): ReactNode {
  const lines = text.split("\n");
  const blocks: ReactNode[] = [];
  let key = 0;
  let i = 0;

  while (i < lines.length) {
    const fence = FENCE.exec(lines[i]);
    if (fence) {
      const marker = fence[2];
      const body: string[] = [];
      i += 1;
      // An unterminated fence closes at the end of the message rather than
      // being dropped or falling back to literal text. Mid-stream that is the
      // common case, not the edge one: a fence being typed grows a code block
      // instead of flickering between formatted and raw on every chunk.
      while (i < lines.length && !lines[i].trimStart().startsWith(marker)) {
        body.push(lines[i]);
        i += 1;
      }
      i += 1;
      blocks.push(
        <pre key={key++}>
          <code>{body.join("\n")}</code>
        </pre>,
      );
      continue;
    }

    const table = readTable(lines, i);
    if (table) {
      const { header, align, rows, next } = table;
      const cell = (row: readonly string[], index: number) => row[index] ?? "";
      blocks.push(
        <div key={key++} className="md-table-wrap">
          <table>
            <thead>
              <tr>
                {header.map((text, index) => (
                  <th key={index} style={alignStyle(align[index])}>
                    {inline(text)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, r) => (
                <tr key={r}>
                  {header.map((_, index) => (
                    <td key={index} style={alignStyle(align[index])}>
                      {inline(cell(row, index))}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      i = next;
      continue;
    }

    const list = readList(lines, i);
    if (list) {
      const { items, next, ordered } = list;
      const rendered = items.map((item, index) => (
        <li key={index}>{inline(item)}</li>
      ));
      blocks.push(
        ordered ? (
          <ol key={key++}>{rendered}</ol>
        ) : (
          <ul key={key++}>{rendered}</ul>
        ),
      );
      i = next;
      continue;
    }

    // Everything else is a run of ordinary lines, kept together so the newlines
    // inside a paragraph stay newlines — the bubble has always shown them and
    // an agent's line breaks are usually deliberate.
    const start = i;
    while (
      i < lines.length &&
      !FENCE.test(lines[i]) &&
      !BULLET.test(lines[i]) &&
      !ORDERED.test(lines[i]) &&
      !readTable(lines, i)
    ) {
      i += 1;
    }
    const paragraph = lines.slice(start, i).join("\n");
    // A blank run between two blocks is spacing the elements already provide.
    if (paragraph.trim()) {
      blocks.push(<p key={key++}>{inline(paragraph)}</p>);
    }
  }

  return blocks;
}

type Align = "left" | "center" | "right" | undefined;

/** Split one table row on unescaped pipes, dropping the optional outer ones. */
function splitRow(line: string): string[] {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|") && !row.endsWith("\\|")) row = row.slice(0, -1);
  const cells: string[] = [];
  let current = "";
  for (let i = 0; i < row.length; i += 1) {
    if (row[i] === "\\" && row[i + 1] === "|") {
      current += "|";
      i += 1;
    } else if (row[i] === "|") {
      cells.push(current.trim());
      current = "";
    } else {
      current += row[i];
    }
  }
  cells.push(current.trim());
  return cells;
}

function alignStyle(align: Align) {
  return align ? { textAlign: align } : undefined;
}

/**
 * A GFM pipe table starting here: a header row containing a pipe, then a
 * delimiter row with the same number of cells, then body rows until a line
 * without a pipe. Anything short of that is not a table.
 */
function readTable(
  lines: readonly string[],
  from: number,
): {
  header: string[];
  align: Align[];
  rows: string[][];
  next: number;
} | null {
  const head = lines[from];
  const delimiter = lines[from + 1];
  if (head === undefined || delimiter === undefined) return null;
  if (!head.includes("|") || !TABLE_DELIMITER.test(delimiter)) return null;
  if (!delimiter.includes("|") && !head.trim().startsWith("|")) return null;
  const header = splitRow(head);
  const marks = splitRow(delimiter);
  if (header.length !== marks.length) return null;
  const align: Align[] = marks.map((mark) => {
    const left = mark.startsWith(":");
    const right = mark.endsWith(":");
    if (left && right) return "center";
    if (right) return "right";
    if (left) return "left";
    return undefined;
  });
  const rows: string[][] = [];
  let i = from + 2;
  while (i < lines.length && lines[i].includes("|") && lines[i].trim()) {
    rows.push(splitRow(lines[i]));
    i += 1;
  }
  return { header, align, rows, next: i };
}

/** A run of bullets or a run of numbers, whichever starts here. */
function readList(
  lines: readonly string[],
  from: number,
): { items: string[]; next: number; ordered: boolean } | null {
  const ordered = ORDERED.test(lines[from]);
  const pattern = ordered ? ORDERED : BULLET;
  if (!ordered && !BULLET.test(lines[from])) return null;

  const items: string[] = [];
  let i = from;
  while (i < lines.length) {
    const match = pattern.exec(lines[i]);
    if (!match) break;
    items.push(match[2]);
    i += 1;
  }
  return { items, next: i, ordered };
}

/**
 * Inline spans, in precedence order: `code` first, then `**bold**`, then
 * `*em*` / `_em_`.
 *
 * Code first because a backtick span is opaque — `**` inside one is two
 * asterisks — and bold before emphasis because `**x**` would otherwise parse
 * as an empty emphasis wrapping one.
 *
 * An unmatched delimiter stays the character it is. That is the load-bearing
 * behaviour rather than the fallback: an agent writing `a * b` about
 * multiplication, or a lone underscore in a filename, must come out as typed.
 */
function inline(text: string): ReactNode {
  return emphasis(text, 0);
}

const SPANS = [
  { open: "`", close: "`", wrap: (node: ReactNode) => <code>{node}</code> },
  {
    open: "**",
    close: "**",
    wrap: (node: ReactNode) => <strong>{node}</strong>,
  },
  { open: "*", close: "*", wrap: (node: ReactNode) => <em>{node}</em> },
  { open: "_", close: "_", wrap: (node: ReactNode) => <em>{node}</em> },
] as const;

/** Letters, digits and `_` itself — what "inside a word" means for the
    intraword rule below. */
function isWord(ch: string | undefined): boolean {
  return ch !== undefined && /[\w]/.test(ch);
}

/** One pass per delimiter, recursing on what is left inside and after. */
function emphasis(text: string, level: number): ReactNode {
  if (level >= SPANS.length) return text;
  const { open, close, wrap } = SPANS[level];
  const out: ReactNode[] = [];
  let rest = text;
  let key = 0;

  for (;;) {
    const start = rest.indexOf(open);
    if (start === -1) break;
    const end = rest.indexOf(close, start + open.length);
    // An unclosed delimiter, or an empty span like `**`: neither is markup.
    if (end === -1 || end === start + open.length) break;

    const before = rest.slice(0, start);
    const body = rest.slice(start + open.length, end);
    // A delimiter with whitespace just inside it is not opening anything —
    // "2 * 3 * 4" is arithmetic, and CommonMark's flanking rule says the same.
    // The check is what keeps prose out of the parser's way.
    if (/^\s|\s$/.test(body)) break;
    // `_` never opens or closes inside a word: `run_once_at` is a field name.
    if (open === "_" && (isWord(before.slice(-1)) || isWord(rest[end + 1]))) {
      break;
    }
    if (before)
      out.push(<span key={key++}>{emphasis(before, level + 1)}</span>);
    out.push(
      <span key={key++}>
        {/* Code is opaque: nothing inside a backtick span is markup. */}
        {wrap(open === "`" ? body : emphasis(body, level + 1))}
      </span>,
    );
    rest = rest.slice(end + close.length);
  }

  if (out.length === 0) return emphasis(text, level + 1);
  if (rest) out.push(<span key={key++}>{emphasis(rest, level + 1)}</span>);
  return out;
}
