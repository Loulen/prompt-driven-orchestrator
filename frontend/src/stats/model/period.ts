/**
 * The Stats period presets: each one's window in whole UTC days and the bucket
 * its charts fold by.
 */

export type Preset = "7d" | "30d" | "90d" | "all";

export const PRESETS: { id: Preset; label: string }[] = [
  { id: "7d", label: "7 days" },
  { id: "30d", label: "30 days" },
  { id: "90d", label: "90 days" },
  { id: "all", label: "All time" },
];

function utcDayStart(date: Date): string {
  return new Date(
    Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()),
  ).toISOString();
}

export function presetPeriod(preset: Preset) {
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
