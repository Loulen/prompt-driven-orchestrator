import type { RefPair } from "../../run/model/runRefs";

// --- « Viewed »: a per-browser mark on a file of a pair, never a Run event -----

export const VIEWED_KEY_PREFIX = "pdo.review.viewed.";

export function viewedKey(runId: string): string {
  return `${VIEWED_KEY_PREFIX}${runId}`;
}

/** The marks are kept per pair: a file viewed between two refs says nothing of
 *  the same file between two others. */
function pairKey(pair: RefPair): string {
  return `${pair.from}..${pair.to}`;
}

function readAll(runId: string, storage: Pick<Storage, "getItem">): Record<string, string[]> {
  try {
    const raw = storage.getItem(viewedKey(runId));
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: Record<string, string[]> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (Array.isArray(v)) out[k] = v.filter((p): p is string => typeof p === "string");
    }
    return out;
  } catch {
    return {};
  }
}

/** The paths marked « Viewed » for this pair of this Run. */
export function readViewed(runId: string, pair: RefPair, storage: Pick<Storage, "getItem"> = localStorage): Set<string> {
  return new Set(readAll(runId, storage)[pairKey(pair)] ?? []);
}

export function writeViewed(
  runId: string,
  pair: RefPair,
  viewed: Set<string>,
  storage: Pick<Storage, "getItem" | "setItem" | "removeItem"> = localStorage,
): void {
  try {
    const all = readAll(runId, storage);
    if (viewed.size === 0) delete all[pairKey(pair)];
    else all[pairKey(pair)] = [...viewed];
    if (Object.keys(all).length === 0) storage.removeItem(viewedKey(runId));
    else storage.setItem(viewedKey(runId), JSON.stringify(all));
  } catch {
    // Private mode / quota: the marks stay in memory for the session.
  }
}

/** The set with `path` flipped. */
export function toggleViewed(viewed: Set<string>, path: string): Set<string> {
  const next = new Set(viewed);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  return next;
}
