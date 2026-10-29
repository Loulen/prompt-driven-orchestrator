/** The status tones of the palette, and the icon type of the kit. */
import type { ComponentType } from "react";

/** An icon component: a `lucide-react` icon, or anything taking the same props. */
export type Icon = ComponentType<{
  size?: number | string;
  strokeWidth?: number | string;
  className?: string;
  "aria-hidden"?: boolean | "true";
}>;

/** The six status tones of the palette. */
export type StatusTone = "running" | "awaiting" | "done" | "blocked" | "failed" | "skipped";

/** A status tone, or none. */
export type Tone = "neutral" | StatusTone;

export const TONE_VAR: Record<StatusTone, string> = {
  running: "var(--color-st-running)",
  awaiting: "var(--color-st-await)",
  done: "var(--color-st-done)",
  blocked: "var(--color-st-blocked)",
  failed: "var(--color-st-failed)",
  skipped: "var(--color-st-skipped)",
};

/** The colour of `tone`, as a CSS value. */
export const toneColor = (tone: Tone): string => (tone === "neutral" ? "var(--text-3)" : TONE_VAR[tone]);
