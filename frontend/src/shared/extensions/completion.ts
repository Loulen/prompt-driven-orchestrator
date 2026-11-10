/**
 * The sources of completion of a text field (API 0.14, `review/composer`): an
 * extension declares a trigger character and a function that answers
 * suggestions; the core opens the list when the trigger is typed, asks the
 * function at each keystroke and inserts the text of the suggestion picked. It
 * never interprets what is inserted.
 *
 * This module holds the text rules: where the trigger and its query sit, and
 * what a pick makes of the text.
 */

/** The trigger typed before the caret, and the text typed after it. */
export interface ActiveQuery {
  trigger: string;
  /** What follows the trigger up to the caret; `""` right after it. */
  query: string;
  /** Where the trigger sits in the text. */
  start: number;
  /** The caret. */
  end: number;
}

/**
 * The query the caret is in: a word that starts with one of `triggers`, at
 * the start of the text or after a whitespace, and runs to the caret without
 * a whitespace. `null` otherwise (the trigger in the middle of a word, `a@b`,
 * does not open the list).
 */
export function activeQuery(text: string, caret: number, triggers: readonly string[]): ActiveQuery | null {
  if (caret < 1 || caret > text.length) return null;
  let start = caret;
  while (start > 0 && !/\s/.test(text[start - 1])) start -= 1;
  const word = text.slice(start, caret);
  const trigger = triggers.find((t) => t.length > 0 && word.startsWith(t));
  if (!trigger) return null;
  return { trigger, query: word.slice(trigger.length), start, end: caret };
}

/**
 * `text` with the trigger and its query replaced by `insert`, followed by a
 * space unless one follows already, and the caret after it.
 */
export function insertSuggestion(text: string, at: ActiveQuery, insert: string): { text: string; caret: number } {
  const after = text.slice(at.end);
  const spacer = /^\s/.test(after) ? "" : " ";
  return {
    text: `${text.slice(0, at.start)}${insert}${spacer}${after}`,
    caret: at.start + insert.length + spacer.length,
  };
}

/** One piece of a text: plain, or a match of the render `source`. */
export type RenderedPiece = { kind: "text"; text: string } | { kind: "match"; source: string; text: string };

/** A render's pattern compiled; `null` when it is no valid regular expression. */
export function compilePattern(pattern: string): RegExp | null {
  try {
    return new RegExp(pattern, "gu");
  } catch {
    try {
      return new RegExp(pattern, "g");
    } catch {
      return null;
    }
  }
}

/**
 * `value` cut into plain text and the matches of the renders, leftmost match
 * first; at the same place, the first render in the list wins. An empty match
 * is never a piece.
 */
export function splitRendered(value: string, renders: readonly { source: string; pattern: RegExp }[]): RenderedPiece[] {
  const pieces: RenderedPiece[] = [];
  let at = 0;
  while (at < value.length) {
    let best: { source: string; index: number; text: string } | null = null;
    for (const { source, pattern } of renders) {
      const found = firstMatch(pattern, value, at);
      if (found && (!best || found.index < best.index)) best = { source, ...found };
    }
    if (!best) break;
    if (best.index > at) pieces.push({ kind: "text", text: value.slice(at, best.index) });
    pieces.push({ kind: "match", source: best.source, text: best.text });
    at = best.index + best.text.length;
  }
  if (at < value.length) pieces.push({ kind: "text", text: value.slice(at) });
  return pieces;
}

function firstMatch(pattern: RegExp, value: string, from: number): { index: number; text: string } | null {
  pattern.lastIndex = from;
  let match = pattern.exec(value);
  while (match && match[0] === "") {
    pattern.lastIndex = match.index + 1;
    if (pattern.lastIndex > value.length) return null;
    match = pattern.exec(value);
  }
  return match ? { index: match.index, text: match[0] } : null;
}
