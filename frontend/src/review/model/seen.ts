import type { ReviewComment } from "../../shared/generated/ReviewComment";

// --- Unread replies: a per-browser "seen" marker, never a Run event -----------

export const SEEN_KEY_PREFIX = "pdo.review.seen.";

/** Comment id → number of replies this browser has seen. */
export type SeenMap = Record<string, number>;

export function seenKey(runId: string): string {
  return `${SEEN_KEY_PREFIX}${runId}`;
}

export function readSeen(runId: string, storage: Pick<Storage, "getItem"> = localStorage): SeenMap {
  try {
    const raw = storage.getItem(seenKey(runId));
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: SeenMap = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "number" && Number.isFinite(v)) out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

export function writeSeen(runId: string, seen: SeenMap, storage: Pick<Storage, "setItem" | "removeItem"> = localStorage): void {
  try {
    if (Object.keys(seen).length === 0) storage.removeItem(seenKey(runId));
    else storage.setItem(seenKey(runId), JSON.stringify(seen));
  } catch {
    // Private mode / quota: unread stays in memory for the session.
  }
}

/** How many replies of `c` this browser has not seen. */
export function unreadReplies(c: ReviewComment, seen: SeenMap): number {
  return Math.max(0, (c.replies?.length ?? 0) - (seen[c.id] ?? 0));
}

/**
 * The Diff tab badge (CONTEXT.md « Réponse de review »): **sent** comments with at
 * least one unread reply. A comment the agent resolved directly is not counted
 * — nothing waits on the human there — but its replies still read as new on the
 * card until seen.
 */
export function unreadCommentCount(comments: ReviewComment[] | undefined, seen: SeenMap): number {
  return (comments ?? []).filter((c) => c.status === "sent" && unreadReplies(c, seen) > 0).length;
}

/** Everything seen — what opening the Review page writes. */
export function allSeen(comments: ReviewComment[] | undefined, prev: SeenMap = {}): SeenMap {
  const next: SeenMap = { ...prev };
  for (const c of comments ?? []) next[c.id] = Math.max(next[c.id] ?? 0, c.replies?.length ?? 0);
  return next;
}

/** One card seen (scrolled into view / hovered / acted on). */
export function markSeen(seen: SeenMap, c: ReviewComment): SeenMap {
  const n = c.replies?.length ?? 0;
  if ((seen[c.id] ?? 0) >= n) return seen;
  return { ...seen, [c.id]: n };
}

