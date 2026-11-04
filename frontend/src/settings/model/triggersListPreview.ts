/**
 * What the preview of the Triggers list block of Settings › Appearance shows:
 * the Trigger the user sees that fired last, and a guarded one when it is not
 * (else the next most recent), so that the guard mark appears once; a sample
 * Trigger when the user sees none.
 */
import type { TriggerListEntry } from "../../shared/generated/TriggerListEntry";

export interface TriggersListPreview {
  /** The rows, in their order. */
  triggers: TriggerListEntry[];
  /** A sample Trigger: the user sees no Trigger. */
  sample: boolean;
  caption: string;
}

const HOUR = 3_600_000;

/** The Trigger the preview shows when the user sees none: no extension mark goes on it. */
export function sampleTrigger(now: number): TriggerListEntry {
  return {
    id: "sample-trigger",
    name: "Nightly review",
    pipeline_id: "my-pipeline",
    pipeline_name: "my-pipeline",
    effective_repo: "",
    input_template: "",
    variables: {},
    cron: "0 3 * * *",
    guard_command: "test -f ready",
    overlap_policy: "skip",
    auto_name: false,
    enabled: true,
    next_fire_at: new Date(now + 7 * HOUR).toISOString(),
    last_fired_at: new Date(now - 17 * HOUR).toISOString(),
    last_outcome: "fired",
  };
}

/** The last fired first; one that never fired after, in the list's order. */
function newer(a: TriggerListEntry, b: TriggerListEntry): number {
  const at = a.last_fired_at ?? "";
  const bt = b.last_fired_at ?? "";
  return at === bt ? 0 : at < bt ? 1 : -1;
}

/** The rows of the preview, from the Triggers the user sees. */
export function triggersListPreview(triggers: TriggerListEntry[], now: number = Date.now()): TriggersListPreview {
  if (triggers.length === 0) {
    return { triggers: [sampleTrigger(now)], sample: true, caption: "Sample trigger · you see no trigger yet" };
  }
  const [recent, ...others] = [...triggers].sort(newer);
  const guarded = recent.guard_command ? undefined : others.find((t) => t.guard_command);
  const second = guarded ?? others[0];
  if (!second) return { triggers: [recent], sample: false, caption: "Your most recent trigger" };
  return {
    triggers: [recent, second],
    sample: false,
    caption: guarded ? "Your most recent trigger, and a guarded trigger" : "Your two most recent triggers",
  };
}
