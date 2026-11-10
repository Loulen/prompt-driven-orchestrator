/**
 * Stats' filter vocabulary and its defaults (#819).
 *
 * **Réglages de Stats éphémères** (CONTEXT.md): nothing here is persisted.
 * Every Stats setting — period, per-tab cohort, duration mode, node kinds,
 * zoom, scales, grouping, sort — returns to the value below each time the
 * surface is mounted; deviating from it is a deliberate gesture, kept only
 * while Stats stays open (switching tabs resets nothing). No `localStorage`
 * key is read or written for Stats: the theme remains the one per-browser
 * preference, and the stale `pdo.stats.*` entries an older build left behind
 * are simply ignored — no migration, no cleanup.
 *
 * This module holds only the words and the defaults, so the shell (which owns
 * the state, because the cohort feeds the fetches) and the charts (which render
 * the band) can never disagree about what « default » means.
 */

import type { CostAxis } from "./cost";

/** The node **genre** a Performance row can carry (CONTEXT.md § Genre de nœud).
 *  A node that is both interactive and orchestrator matches either chip;
 *  « standard » is the absence of both, never a third stored flag. */
export type NodeKind = "interactive" | "orchestrator" | "standard";

export const ALL_NODE_KINDS: readonly NodeKind[] = [
  "interactive",
  "orchestrator",
  "standard",
];

/**
 * **Mode de durée** (CONTEXT.md): how Performance reads the duration of a Node
 * execution. `total` is the wall-clock, `active` the wall-clock minus the
 * declared wait, `waiting` that declared wait alone — the human brain time the
 * execution cost. The daemon sends the three distributions together, so the
 * switch is client-side: no refetch, no dimmed pane.
 */
export type DurationMode = "total" | "active" | "waiting";

export const DURATION_MODES: readonly DurationMode[] = [
  "total",
  "active",
  "waiting",
];

/** What the whiskers of a box-plot reach (#811). */
export type StatsZoom = "full" | "fenced" | "box";

/** One axis per metric, or one axis per row (#811). */
export type StatsAxis = "shared" | "independent";

/**
 * The cohort each tab opens on — « Runs terminés seulement », per tab and with
 * its own default. Performance opens narrowed, so durations are not polluted by
 * runs still in flight or abandoned; Cost and Overview open on every run, so
 * the spend and the errors of a failed run stay visible. Overview, Sessions and
 * Triggers read the same response, so they share one state.
 */
export const TAB_COHORT_DEFAULTS = {
  overview: false,
  cost: false,
  performance: true,
} as const;

/** The Performance band's controls, cohort excluded (it travels per tab). */
export interface PerformanceBand {
  durationMode: DurationMode;
  nodeKinds: NodeKind[];
  zoom: StatsZoom;
  axis: StatsAxis;
}

export const DEFAULT_PERFORMANCE_BAND: PerformanceBand = {
  durationMode: "active",
  nodeKinds: [...ALL_NODE_KINDS],
  zoom: "full",
  axis: "independent",
};

/** Does the Performance band deviate from what the tab opens on? Drives the
 *  « reset filters » link — and nothing else: no rail badge marks a deviation,
 *  because the band itself is on screen, saying exactly what it does. */
export function performanceBandDeviates(
  completedOnly: boolean,
  band: PerformanceBand,
): boolean {
  return (
    completedOnly !== TAB_COHORT_DEFAULTS.performance ||
    band.durationMode !== DEFAULT_PERFORMANCE_BAND.durationMode ||
    band.nodeKinds.length !== ALL_NODE_KINDS.length ||
    band.zoom !== DEFAULT_PERFORMANCE_BAND.zoom ||
    band.axis !== DEFAULT_PERFORMANCE_BAND.axis
  );
}

/**
 * The Cost tab's reading: its grouping, and « Roll up child runs » — every
 * child run counted under the Pipeline, the Project and the period of its root
 * run. Held by the shell, because the roll-up travels to `/stats/cost` and
 * « By model » does not take it; off at each open, like every Stats setting.
 */
export interface CostView {
  axis: CostAxis;
  rollUp: boolean;
}

export const DEFAULT_COST_VIEW: CostView = { axis: "pipeline", rollUp: false };

/** Is the roll-up in force? Never on « By model »: it has nothing to roll up,
 *  and leaving it there gives it back, as it was, on the other groupings. */
export const rollsUp = (view: CostView): boolean =>
  view.rollUp && view.axis !== "model";
