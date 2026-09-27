import type { ReviewAnchorMapping } from "../../shared/generated/ReviewAnchorMapping";
import type { ReviewComment } from "../../shared/generated/ReviewComment";
import type { RefPair } from "../../run/model/runRefs";
import { plural } from "./commentStatus";
import { draftsForPair, sameAnchor } from "./drafts";
import type { Anchor, ReviewDraft } from "./drafts";

// The merged list the Review page renders (#750, #752): drafts and sent
// comments of one pair, in diff order, and what the re-map onto the displayed
// pair says of each sent one.

/**
 * One entry of the merged list the page renders. A sent entry's `anchor` is
 * where the card sits **on the displayed pair** (#752): the reported line when
 * the daemon mapped it, the written line otherwise. `outdated` marks a comment
 * whose line changed since (card in the file's outdated group, not inline);
 * `movedFrom` is the written line when the reported number differs.
 */
export type ReviewEntry =
  | { kind: "draft"; anchor: Anchor; draft: ReviewDraft }
  | { kind: "sent"; anchor: Anchor; comment: ReviewComment; outdated?: boolean; movedFrom?: number };

/** Comment id → its mapping onto the displayed pair (from `GET …/review/comments?from&to`). */
export type MappingMap = ReadonlyMap<string, ReviewAnchorMapping>;

export function mappingsOf(comments: (ReviewComment & Partial<ReviewAnchorMapping>)[]): Map<string, ReviewAnchorMapping> {
  const m = new Map<string, ReviewAnchorMapping>();
  for (const c of comments) {
    if (typeof c.outdated !== "boolean") continue;
    m.set(c.id, { outdated: c.outdated, mapped_line: c.mapped_line, moved: c.moved });
  }
  return m;
}

/** The sent comments written against exactly this pair. */
export function sentForPair(comments: ReviewComment[] | undefined, pair: RefPair): ReviewComment[] {
  return (comments ?? []).filter((c) => c.from_ref === pair.from && c.to_ref === pair.to);
}

/**
 * The sent entries **at home** on the displayed pair (#752): a comment the daemon
 * mapped onto the pair whose file is in the displayed diff — reported at its
 * mapped line, or outdated at its written line — and, while no mapping is known
 * (not fetched yet, or the daemon could not map it), a comment written against
 * exactly this pair, where it was written. A mapped comment whose file is not
 * in the displayed diff has no card to live in: it is listed under "other".
 */
export function sentEntriesForPair(
  comments: ReviewComment[] | undefined,
  pair: RefPair,
  mappings: MappingMap | undefined,
  pathOrder: string[],
): ReviewEntry[] {
  const paths = new Set(pathOrder);
  const out: ReviewEntry[] = [];
  for (const c of comments ?? []) {
    const m = mappings?.get(c.id);
    const exact = c.from_ref === pair.from && c.to_ref === pair.to;
    if (!m) {
      if (exact) out.push({ kind: "sent", anchor: { path: c.path, side: c.side, line: c.line }, comment: c });
      continue;
    }
    // Mapped, but its file is not in this diff: no card can host it — "other".
    if (!paths.has(c.path)) continue;
    if (m.outdated) {
      out.push({ kind: "sent", anchor: { path: c.path, side: c.side, line: c.line }, comment: c, outdated: true });
      continue;
    }
    const line = m.mapped_line ?? c.line;
    out.push({
      kind: "sent",
      anchor: { path: c.path, side: c.side, line },
      comment: c,
      movedFrom: line !== c.line ? c.line : undefined,
    });
  }
  return out;
}

/** The ids of the comments `sentEntriesForPair` places on the pair — the rest are "other". */
export function homeIds(entries: ReviewEntry[]): Set<string> {
  return new Set(entries.flatMap((e) => (e.kind === "sent" ? [e.comment.id] : [])));
}

/** `1 comment moved · 1 outdated` after a re-map; null when nothing moved or went outdated. */
export function remapSummary(entries: ReviewEntry[]): string | null {
  let moved = 0;
  let outdated = 0;
  for (const e of entries) {
    if (e.kind !== "sent") continue;
    if (e.outdated) outdated += 1;
    else if (e.movedFrom !== undefined) moved += 1;
  }
  if (moved === 0 && outdated === 0) return null;
  const parts: string[] = [];
  if (moved > 0) parts.push(`${plural(moved, "comment")} moved`);
  if (outdated > 0) parts.push(`${outdated} outdated`);
  return parts.join(" · ");
}

/** Per-path outdated count — the history glyph on file rows and headers (#752). */
export function outdatedCountByPath(entries: ReviewEntry[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const e of entries) {
    if (e.kind === "sent" && e.outdated) m.set(e.anchor.path, (m.get(e.anchor.path) ?? 0) + 1);
  }
  return m;
}

/** One line of a stored excerpt (`> 3 | text`): number, text, and whether it is the commented line. */
export interface ExcerptLine {
  no: number;
  text: string;
  marked: boolean;
}

/** Parse the hunk excerpt the daemon stored at send time (the outdated card's original hunk). */
export function parseExcerpt(excerpt: string | undefined): ExcerptLine[] {
  if (!excerpt) return [];
  const out: ExcerptLine[] = [];
  for (const raw of excerpt.split("\n")) {
    const m = /^([> ]) *(\d+) \| ?(.*)$/.exec(raw);
    if (!m) continue;
    out.push({ no: Number(m[2]), text: m[3], marked: m[1] === ">" });
  }
  return out;
}

export const SHOW_OUTDATED_KEY = "pdo.review.showOutdated";

/**
 * The merged list for one pair, in diff order: by path (patch order given), then
 * line, then side (old before new — the left column first). Sent before draft on
 * the same anchor is impossible by construction (one comment per line), but a
 * stale draft on a line just sent still resolves deterministically: sent wins.
 */
export function mergeEntries(
  drafts: ReviewDraft[],
  comments: ReviewComment[] | undefined,
  pair: RefPair,
  pathOrder: string[],
  mappings?: MappingMap,
): ReviewEntry[] {
  const order = new Map(pathOrder.map((p, i) => [p, i]));
  const rank = (p: string) => order.get(p) ?? Number.MAX_SAFE_INTEGER;
  const entries: ReviewEntry[] = [
    ...sentEntriesForPair(comments, pair, mappings, pathOrder),
    ...draftsForPair(drafts, pair).map<ReviewEntry>((d) => ({
      kind: "draft",
      anchor: { path: d.path, side: d.side, line: d.line },
      draft: d,
    })),
  ];
  return entries.sort((a, b) => {
    const r = rank(a.anchor.path) - rank(b.anchor.path);
    if (r !== 0) return r;
    if (a.anchor.path !== b.anchor.path) return a.anchor.path < b.anchor.path ? -1 : 1;
    if (a.anchor.line !== b.anchor.line) return a.anchor.line - b.anchor.line;
    if (a.anchor.side !== b.anchor.side) return a.anchor.side === "old" ? -1 : 1;
    // Same anchor: sent first, so the page shows the immutable one.
    if (a.kind !== b.kind) return a.kind === "sent" ? -1 : 1;
    return 0;
  });
}

/** The entry anchored on this exact line, if any (the "one comment per line" lookup). */
export function entryAt(entries: ReviewEntry[], anchor: Anchor): ReviewEntry | undefined {
  return entries.find((e) => sameAnchor(e.anchor, anchor));
}

/** Per-file counts for the sidebar badges and the card headers. */
export function countsByPath(entries: ReviewEntry[]): Map<string, { drafts: number; sent: number }> {
  const m = new Map<string, { drafts: number; sent: number }>();
  for (const e of entries) {
    const c = m.get(e.anchor.path) ?? { drafts: 0, sent: 0 };
    if (e.kind === "draft") c.drafts += 1;
    else c.sent += 1;
    m.set(e.anchor.path, c);
  }
  return m;
}

