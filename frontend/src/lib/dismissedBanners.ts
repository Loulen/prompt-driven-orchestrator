/**
 * Per-pipeline persistence of dismissed canvas banner rows (#268).
 *
 * Only advisory fan-out *nudges* are dismissible (ADR-0001: the off-switch is
 * for advisory signals, not correctness lint), so the stored value is the set
 * of dismissed nudge ids (`fanout:<targetNodeId>`) for one pipeline. The shape
 * is a JSON **array** of ids — a set membership with no redundant `true`, so a
 * future reset feature can `removeItem` or filter without a migration.
 *
 * Mirrors the localStorage discipline of `useResizableLayout` (parse under
 * try/catch with a type guard + safe default) but guards **both** read and
 * write: in private mode / when storage is disabled or full, a dismiss degrades
 * to in-memory-only for the session rather than throwing — the banner simply
 * stays visible across reloads instead of crashing the canvas.
 */
const keyFor = (tabId: string) => `pdo.banner.dismissed.${tabId}`;

export function loadDismissed(tabId: string): Set<string> {
  try {
    const raw = localStorage.getItem(keyFor(tabId));
    if (!raw) return new Set();
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed) || !parsed.every((x) => typeof x === "string")) {
      return new Set();
    }
    return new Set(parsed as string[]);
  } catch {
    // private mode / disabled / corrupt → treat as nothing dismissed
    return new Set();
  }
}

export function saveDismissed(tabId: string, ids: Set<string>): void {
  try {
    localStorage.setItem(keyFor(tabId), JSON.stringify([...ids]));
  } catch {
    // quota / disabled → dismiss is in-memory only for this session
  }
}
