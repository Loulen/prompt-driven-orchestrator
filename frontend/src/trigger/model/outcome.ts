import type { Trigger } from "../../shared/generated/Trigger";
import type { StatusTone } from "../../shared/ui/kit";

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

export function lastOutcomeTooltip(t: Trigger): string {
  if (!t.last_outcome) return "never fired";
  const when = t.last_fired_at ?? "—";
  return `last run: ${when}, result: ${t.last_outcome}`;
}

/** A fire's instant, short and local (`Sep 27, 09:00`); `null` when absent. */
export function formatFireTs(iso: string | null | undefined): string | null {
  if (!iso) return null;
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
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
