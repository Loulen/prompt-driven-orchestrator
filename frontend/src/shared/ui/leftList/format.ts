/**
 * The short figures of the rows of the left panel: a duration, a cost, a date,
 * the time to a fire.
 */

/** A duration: `42s`, `14m`, `1h15`, `3d`. */
export function fmtSpan(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h${String(m % 60).padStart(2, "0")}`;
  return `${Math.floor(h / 24)}d`;
}

/** The time from `from` to `to` (or `now`), `null` without a start. */
export function spanOf(from: string | null | undefined, to: string | null | undefined, now: number): number | null {
  if (!from) return null;
  const start = Date.parse(from);
  if (Number.isNaN(start)) return null;
  const end = to ? Date.parse(to) : now;
  return Number.isNaN(end) ? null : Math.max(0, end - start);
}

/** A cost in dollars, two decimals: `$0.41`. */
export const fmtCost = (usd: number): string => `$${usd.toFixed(2)}`;

/** An edit date, short: `25 Oct`. */
export function fmtEdited(iso: string | null | undefined): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString("en-GB", { day: "numeric", month: "short" });
}

/** The time to `iso` from `now`: `5m`, `16h`, `3d`; `now` once due. */
export function fmtUntil(iso: string, now: number): string {
  const m = Math.round((Date.parse(iso) - now) / 60_000);
  if (Number.isNaN(m)) return "";
  if (m < 1) return "now";
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  return h < 24 ? `${h}h` : `${Math.round(h / 24)}d`;
}

/** The time since `iso`: `now`, `5m`, `16h`, `3d`. */
export function fmtAge(iso: string, now: number): string {
  const m = Math.round((now - Date.parse(iso)) / 60_000);
  if (Number.isNaN(m)) return "";
  if (m < 1) return "now";
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  return h < 24 ? `${h}h` : `${Math.round(h / 24)}d`;
}
