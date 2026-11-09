import type { Trigger } from "../../shared/generated/Trigger";
import type { StatusTone } from "../../shared/ui/kit";
import { CRON_TIME_ZONE } from "./cronPresets";

/** A fire outcome as a status tone; `null` before the first fire. */
export function outcomeTone(outcome: string | null | undefined): StatusTone | null {
  switch (outcome) {
    case "fired":
      return "done";
    case "error":
      return "failed";
    case "skipped-overlap":
    case "skipped-suspended":
    case "guard-exit-nonzero":
    case "guard-error":
      return "blocked";
    default:
      return null;
  }
}

/**
 * Map a fire outcome (a Trigger's `last_outcome`, or one history row's) to a
 * status-dot color (reuses the run status tokens). No outcome yet ⇒ a neutral
 * "pending" dot.
 */
export function outcomeDot(outcome: string | null | undefined): string {
  switch (outcome) {
    case "fired":
      return "bg-st-done";
    case "error":
      return "bg-st-failed";
    case "skipped-overlap":
    case "skipped-suspended":
    case "guard-exit-nonzero":
      return "bg-st-blocked";
    case "guard-error":
      return "bg-st-blocked";
    default:
      return "bg-st-skipped";
  }
}

/** Why Run now is greyed on a disabled Trigger, which the daemon would refuse. */
export const DISABLED_FIRE_REFUSAL = "Disabled: enable it first";

/**
 * Why Run now is refused on `t`, if it is: the caller's rights first, then a disabled
 * Trigger, which the daemon refuses with a 409 (CORE-232) — said before the click.
 */
export function fireRefusalOf(t: Pick<Trigger, "enabled">, rightsRefusal: string | undefined): string | undefined {
  return rightsRefusal ?? (t.enabled ? undefined : DISABLED_FIRE_REFUSAL);
}

/** What wakes a Trigger with no next fire: a missing pipeline or repository put it to sleep. */
export const NOT_SCHEDULED_HINT = "Run now or disable and enable it to resume";

/**
 * The status icon's tooltip: the last tick's result in words, then the last fire.
 * `last_fired_at` is the last *fire*, not the last tick: a later skip keeps it, so
 * the two are said apart, never as « result X at T ».
 */
export function lastOutcomeTooltip(t: Pick<Trigger, "last_outcome" | "last_fired_at">): string {
  const fired = formatFireTs(t.last_fired_at);
  const lastFire = fired ? `Last fired ${fired}` : "Never fired";
  if (!t.last_outcome) return lastFire;
  return `Last tick: ${outcomeWord(t.last_outcome)}\n${lastFire}`;
}

/**
 * An instant of a Trigger, named in its time zone: by default the one its
 * cron is read in (« Nov 1, 03:00 UTC », a daily 03:00 reads 03:00 whatever
 * the browser's zone); `"local"`, the browser's (« Nov 1, 04:00 GMT+1 »).
 */
export function formatFireTs(iso: string | null | undefined, zone: "cron" | "local" = "cron"): string | null {
  if (!iso) return null;
  try {
    const at = new Date(iso).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
      ...(zone === "cron" ? { timeZone: CRON_TIME_ZONE } : { timeZoneName: "short" }),
    });
    return zone === "cron" ? `${at} ${CRON_TIME_ZONE}` : at;
  } catch {
    return iso;
  }
}

/** The overlap policy, as the detail panel reads it. */
export function overlapLabel(t: Pick<Trigger, "overlap_policy" | "max_concurrent">): string {
  return t.overlap_policy === "allow"
    ? t.max_concurrent != null
      ? `allow (max ${t.max_concurrent} concurrent)`
      : "allow (unlimited concurrent fires)"
    : "skip (default)";
}

/** A fire outcome, as a word: `guard-exit-nonzero` → « Guard skip ». */
export function outcomeWord(outcome: string): string {
  switch (outcome) {
    case "fired":
      return "Fired";
    case "skipped-overlap":
      return "Skipped, overlap";
    case "skipped-suspended":
      return "Skipped, suspended";
    case "guard-exit-nonzero":
      return "Guard skip";
    case "guard-error":
      return "Guard error";
    case "error":
      return "Error";
    default:
      return outcome;
  }
}
