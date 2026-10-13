// PDO-22 (#996/#997) — the ONE status table: status -> glyph, colour, label, spin.
// Every surface that shows a status renders it through `StatusIcon`, which reads
// this table; the SHAPE carries the status (deutan/protan users cannot rely on
// the colour), the colour only reinforces it. The round glyphs are reserved for
// statuses; a row's selection is a square (`SelectControl`).
import {
  Archive,
  Circle,
  CircleArrowRight,
  CircleCheck,
  CircleHelp,
  CirclePause,
  CircleSlash,
  CircleStop,
  CircleX,
  Clock,
  Hourglass,
  LoaderCircle,
  type LucideIcon,
} from "lucide-react";
import type { NodeStatus, RunStatus } from "../types";

/** The outcome of a Trigger's last fire (`Trigger.last_outcome`), plus `never`
 *  for a Trigger that has not fired yet. */
export type TriggerOutcomeKey =
  | "fired"
  | "error"
  | "skipped-overlap"
  | "guard-exit-nonzero"
  | "guard-error"
  | "never";

/** Every status a `StatusIcon` can show: Run statuses (with the derived
 *  `stalled` flag), NodeRun statuses and Trigger outcomes. */
export type StatusKey = RunStatus | NodeStatus | "stalled" | TriggerOutcomeKey;

export interface StatusSpec {
  /** The lucide glyph — its shape is what carries the status. */
  glyph: LucideIcon;
  /** The glyph's name, for tests and docs (lucide aliases share components). */
  glyphName: string;
  /** `text-st-*` class: the glyph strokes with `currentColor`. */
  colorClass: string;
  /** Human label: tooltip prefix and accessible name. */
  label: string;
  /** Turns slowly (running), fixed under `prefers-reduced-motion`. */
  spin: boolean;
}

const spec = (glyph: LucideIcon, glyphName: string, colorClass: string, label: string, spin = false): StatusSpec => ({
  glyph,
  glyphName,
  colorClass,
  label,
  spin,
});

export const STATUS_TABLE: Record<StatusKey, StatusSpec> = {
  pending: spec(Circle, "Circle", "text-st-pending", "Pending"),
  running: spec(LoaderCircle, "LoaderCircle", "text-st-running", "Running", true),
  awaiting_user: spec(CircleHelp, "CircleHelp", "text-st-await", "Awaiting you"),
  completed: spec(CircleCheck, "CircleCheck", "text-st-done", "Completed"),
  failed: spec(CircleX, "CircleX", "text-st-failed", "Failed"),
  skipped: spec(CircleArrowRight, "CircleArrowRight", "text-st-skipped", "Skipped"),
  paused: spec(CirclePause, "CirclePause", "text-st-paused", "Paused"),
  // halted (Run) and stopped (node) mean the same thing: the one shared glyph.
  halted: spec(CircleStop, "CircleStop", "text-st-blocked", "Halted"),
  stopped: spec(CircleStop, "CircleStop", "text-st-stopped", "Stopped"),
  interrupted: spec(CircleSlash, "CircleSlash", "text-st-interrupted", "Interrupted"),
  stale: spec(Clock, "Clock", "text-st-stale", "Stale"),
  stalled: spec(Hourglass, "Hourglass", "text-st-stale", "Stalled"),
  archived: spec(Archive, "Archive", "text-st-archived", "Archived"),
  // Trigger outcomes reuse the Run vocabulary (fired = check, error = cross…).
  fired: spec(CircleCheck, "CircleCheck", "text-st-done", "Fired"),
  error: spec(CircleX, "CircleX", "text-st-failed", "Error"),
  "skipped-overlap": spec(CirclePause, "CirclePause", "text-st-paused", "Skipped (previous run still live)"),
  "guard-exit-nonzero": spec(CirclePause, "CirclePause", "text-st-paused", "Skipped by its guard"),
  "guard-error": spec(CircleStop, "CircleStop", "text-st-blocked", "Guard error"),
  never: spec(Circle, "Circle", "text-st-pending", "Never fired"),
};

/** The table entry for `status`; an unknown value (a newer daemon) reads as pending. */
export function statusSpec(status: StatusKey | string): StatusSpec {
  return STATUS_TABLE[status as StatusKey] ?? STATUS_TABLE.pending;
}

/** The tooltip text: the label, plus the reason when there is one
 *  (« Failed: exit 1 », #503 / #588). */
export function statusTitle(status: StatusKey | string, reason?: string | null): string {
  const { label } = statusSpec(status);
  return reason ? `${label}: ${reason}` : label;
}

/** A Run list entry's key: the derived `stalled` flag (#180) overrides the
 *  still-`running` canonical status — « never a silent stall ». */
export function runStatusKey(run: { status: RunStatus; stalled?: boolean | null }): StatusKey {
  return run.stalled ? "stalled" : run.status;
}

/** A Trigger's `last_outcome` as a key; none yet (or an unknown value) ⇒ `never`. */
export function triggerStatusKey(outcome: string | null | undefined): TriggerOutcomeKey {
  switch (outcome) {
    case "fired":
    case "error":
    case "skipped-overlap":
    case "guard-exit-nonzero":
    case "guard-error":
      return outcome;
    default:
      return "never";
  }
}
