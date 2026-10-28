/**
 * What the command field remembers it opened (a Run, a Pipeline, a Trigger,
 * an extension's result), newest first, whatever its source: its « Recent »
 * group on an empty query. Per browser.
 */

const KEY = "pdo.palette.recent";
const MAX = 5;

export type RecentPick =
  | { kind: "run" | "pipeline" | "trigger"; id: string }
  | { kind: "result"; id: string; extension: string; title: string; ref?: string | null; link: string };

/** The key a pick is remembered under: the same item opened twice is one pick. */
export const recentKey = (pick: RecentPick) =>
  pick.kind === "result" ? `result:${pick.extension}:${pick.link}` : `${pick.kind}:${pick.id}`;

export function readRecent(): RecentPick[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((p) => p && typeof p.kind === "string" && typeof p.id === "string") : [];
  } catch {
    return [];
  }
}

export function rememberPick(pick: RecentPick): void {
  const key = recentKey(pick);
  const next = [pick, ...readRecent().filter((p) => recentKey(p) !== key)].slice(0, MAX);
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // A full or forbidden storage: nothing remembered.
  }
}
