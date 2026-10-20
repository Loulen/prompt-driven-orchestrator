import type { RunListEntry } from "../../shared/generated/RunListEntry";

/* The Date axis of the Runs list's filters. Two modes (CONTEXT.md « Activité
   d'un Run »): « Active » (the default) keeps a Run with any activity in the
   range, « Started » one started in it. A range is a preset rolling from today
   or a custom pair of days, both read in the browser's time zone: the daemon
   ships instants (`started_at`, the spans of `activity`), never days. */

export type DateMode = "active" | "started";
export type DatePreset = "today" | "yesterday" | "7d" | "30d";
/** A preset, or a custom pair of local days (`YYYY-MM-DD`, both included). */
export type DateRange = { preset: DatePreset } | { from: string; to: string };

export interface RunDateFilter {
  mode: DateMode;
  /** `null`: any time. */
  range: DateRange | null;
}

export const ANY_TIME: RunDateFilter = { mode: "active", range: null };

export const DATE_PRESETS: { preset: DatePreset; label: string; short: string }[] = [
  { preset: "today", label: "Today", short: "Today" },
  { preset: "yesterday", label: "Yesterday", short: "Yesterday" },
  { preset: "7d", label: "Last 7 days", short: "7 days" },
  { preset: "30d", label: "Last 30 days", short: "30 days" },
];

function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

function addDays(d: Date, days: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days);
}

/** A local day as `YYYY-MM-DD`. */
export function dayString(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function parseDay(day: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  return m ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])) : null;
}

/** The local days a range covers, first and last included. */
export function rangeDays(range: DateRange, now: Date): { from: string; to: string } {
  if ("from" in range) return { from: range.from, to: range.to };
  const today = startOfDay(now);
  switch (range.preset) {
    case "today":
      return { from: dayString(today), to: dayString(today) };
    case "yesterday":
      return { from: dayString(addDays(today, -1)), to: dayString(addDays(today, -1)) };
    case "7d":
      return { from: dayString(addDays(today, -7)), to: dayString(today) };
    case "30d":
      return { from: dayString(addDays(today, -30)), to: dayString(today) };
  }
}

/** The instants a range covers, `[start, end)` in epoch milliseconds; `null`
 *  for a custom range with an unreadable day. Swapped days are put in order. */
export function rangeBounds(range: DateRange, now: Date): { start: number; end: number } | null {
  const days = rangeDays(range, now);
  const from = parseDay(days.from);
  const to = parseDay(days.to);
  if (!from || !to) return null;
  const [first, last] = from <= to ? [from, to] : [to, from];
  return { start: first.getTime(), end: addDays(last, 1).getTime() };
}

/** Whether `r` passes the Date axis of the filter at `now`. */
export function runMatchesDate(r: RunListEntry, f: RunDateFilter, now: Date): boolean {
  if (f.range === null) return true;
  const bounds = rangeBounds(f.range, now);
  if (!bounds) return true;
  const { start, end } = bounds;
  const inRange = (ts: number) => ts >= start && ts < end;
  const started = r.started_at ? Date.parse(r.started_at) : NaN;
  if (f.mode === "started") return inRange(started);
  const spans = r.activity ?? [];
  if (spans.length === 0) return inRange(started);
  return spans.some((span) => Date.parse(span.from) < end && Date.parse(span.to) >= start);
}

function shortDay(d: Date, now: Date): string {
  return d.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    ...(d.getFullYear() !== now.getFullYear() ? { year: "numeric" } : {}),
  });
}

/** The filter button's words: `null` for any time. */
export function dateFilterLabel(f: RunDateFilter, now: Date): string | null {
  if (f.range === null) return null;
  const range = f.range;
  if ("preset" in range) return DATE_PRESETS.find((p) => p.preset === range.preset)!.short;
  const from = parseDay(range.from);
  const to = parseDay(range.to);
  if (!from || !to) return `${range.from} → ${range.to}`;
  return range.from === range.to
    ? shortDay(from, now)
    : `${shortDay(from, now)} → ${shortDay(to, now)}`;
}

/** What the mode keeps, in the words of the menu's footnote. */
export function dateModeHint(mode: DateMode): string {
  return mode === "active"
    ? "Active: a Run with any event in the range, archived ones included."
    : "Started: a Run started in the range, archived ones included.";
}

/** A Run row's start date: `null` when it started today (or never), else
 *  `Oct 17`, with the year when it is not this one. */
export function startDateLabel(startedAt: string | null | undefined, now: Date): string | null {
  if (!startedAt) return null;
  const ts = Date.parse(startedAt);
  if (Number.isNaN(ts)) return null;
  const started = new Date(ts);
  return dayString(started) === dayString(now) ? null : shortDay(started, now);
}
