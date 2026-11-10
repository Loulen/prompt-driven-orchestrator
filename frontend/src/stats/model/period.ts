/**
 * The Stats period presets: each one's window in whole UTC days and the bucket
 * its charts fold by.
 */

export type Preset = "7d" | "30d" | "90d" | "all";

export const PRESETS: { id: Preset; label: string; title: string }[] = [
  { id: "7d", label: "7d", title: "7 days" },
  { id: "30d", label: "30d", title: "30 days" },
  { id: "90d", label: "90d", title: "90 days" },
  { id: "all", label: "All", title: "All time" },
];

/** The days a window covers, `to` excluded: `2026-09-29 – 2026-10-28`; none for « All time ». */
export function periodDays(preset: Preset, period: { from: string; to: string }): string | null {
  if (preset === "all") return null;
  const last = new Date(period.to);
  last.setUTCDate(last.getUTCDate() - 1);
  return `${period.from.slice(0, 10)} – ${last.toISOString().slice(0, 10)}`;
}

function utcDayStart(date: Date): string {
  return new Date(
    Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()),
  ).toISOString();
}

export type Bucket = "day" | "week" | "month";

export function presetPeriod(preset: Preset): { from: string; to: string; bucket: Bucket } {
  const now = new Date();
  const tomorrow = new Date(now);
  tomorrow.setUTCDate(tomorrow.getUTCDate() + 1);
  const daysAgo = (days: number) => {
    const date = new Date(now);
    date.setUTCDate(date.getUTCDate() - days);
    return utcDayStart(date);
  };

  if (preset === "7d") return { from: daysAgo(6), to: utcDayStart(tomorrow), bucket: "day" };
  if (preset === "30d") return { from: daysAgo(29), to: utcDayStart(tomorrow), bucket: "day" };
  if (preset === "90d") return { from: daysAgo(89), to: utcDayStart(tomorrow), bucket: "week" };
  return {
    from: "1970-01-01T00:00:00.000Z",
    to: utcDayStart(tomorrow),
    bucket: "month",
  };
}
