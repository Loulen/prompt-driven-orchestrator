/**
 * Cron-preset utilities (#160). Friendly presets compile to a 5-field cron
 * expression on the frontend; the daemon stores and schedules on the raw cron.
 * A raw-expression escape hatch covers everything the presets don't.
 */

export type CronPresetId = "every_15_min" | "hourly" | "daily" | "custom";

export interface CronPreset {
  id: Exclude<CronPresetId, "custom">;
  label: string;
}

/** The presets offered in the New Run modal's Trigger mode. */
export const CRON_PRESETS: CronPreset[] = [
  { id: "every_15_min", label: "Every 15 min" },
  { id: "hourly", label: "Hourly" },
  { id: "daily", label: "Daily" },
];

/** Time-of-day for the daily preset. */
export interface DailyTime {
  hour: number;
  minute: number;
}

/** Compile a preset (plus optional daily time) into a 5-field cron expression. */
export function presetToCron(
  id: Exclude<CronPresetId, "custom">,
  time: DailyTime = { hour: 9, minute: 0 },
): string {
  switch (id) {
    case "every_15_min":
      return "*/15 * * * *";
    case "hourly":
      return "0 * * * *";
    case "daily":
      return `${time.minute} ${time.hour} * * *`;
  }
}

/** Identify which preset a cron expression corresponds to, or `custom`. */
export function cronToPreset(cron: string): CronPresetId {
  const norm = cron.trim().replace(/\s+/g, " ");
  if (norm === "*/15 * * * *") return "every_15_min";
  if (norm === "0 * * * *") return "hourly";
  // Daily: `<min> <hour> * * *`.
  if (/^\d{1,2} \d{1,2} \* \* \*$/.test(norm)) return "daily";
  return "custom";
}

/** A short human-readable label for a cron expression (UI rows). */
export function humanizeCron(cron: string): string {
  const norm = cron.trim().replace(/\s+/g, " ");
  if (norm === "*/15 * * * *") return "every 15 min";
  if (norm === "0 * * * *") return "hourly";
  const daily = norm.match(/^(\d{1,2}) (\d{1,2}) \* \* \*$/);
  if (daily) {
    const minute = daily[1].padStart(2, "0");
    const hour = daily[2].padStart(2, "0");
    return `daily at ${hour}:${minute}`;
  }
  return norm;
}
