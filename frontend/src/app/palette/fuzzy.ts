/**
 * The command field's fuzzy match: the query's letters, in order, anywhere in
 * the text (spaces in the query ignored). The score favours word starts and
 * runs of consecutive letters, so `adps` puts « Add a product search » first.
 */

export interface Match {
  score: number;
  /** The indexes of the matched letters in the text. */
  hits: number[];
}

/** Scattered letters are noise: a match averages at least this much per query letter. */
export const MIN_SCORE_PER_CHAR = 2;

/** `query` matched against `text`; `null` when it does not match. */
export function fuzzy(query: string, text: string): Match | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return { score: 0, hits: [] };
  const t = text.toLowerCase();
  const hits: number[] = [];
  let score = 0;
  let from = 0;
  for (const ch of q) {
    const i = t.indexOf(ch, from);
    if (i < 0) return null;
    const wordStart = i === 0 || /[\s\-_:./·]/.test(t[i - 1]);
    const consecutive = hits.length > 0 && hits[hits.length - 1] === i - 1;
    score += 1 + (wordStart ? 3 : 0) + (consecutive ? 2 : 0) - Math.min(i - from, 4) * 0.25;
    hits.push(i);
    from = i + 1;
  }
  return { score: score - t.length * 0.01, hits };
}

/** Whether `match` of `query` is worth showing. */
export function isRelevant(match: Match | null, query: string): match is Match {
  return match !== null && match.score >= query.replace(/\s+/g, "").length * MIN_SCORE_PER_CHAR;
}

/** The ranges `[start, end)` where `query` (trimmed, case-insensitive) occurs in `text`. */
export function substringRanges(text: string, query: string): [number, number][] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const t = text.toLowerCase();
  const out: [number, number][] = [];
  for (let i = t.indexOf(q); i >= 0; i = t.indexOf(q, i + q.length)) out.push([i, i + q.length]);
  return out;
}
