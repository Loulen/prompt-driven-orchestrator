/**
 * The panels a step lights beside its target (#911): its read-only areas, the
 * scope the read-only guard protects, and its side bubbles — resolved from the
 * DOM on each tick of `useTour`'s pump.
 */

import { stepAsides, stepReadOnly } from "../../lib/tour";
import type { TourObservation, TourStep } from "../../lib/tourDef";
import type { ReadOnlyScope } from "../../lib/tourReadOnly";
import { query, rectOf, visible } from "./tourGeometry";
import type { TourAsideView, TourRect } from "./tourView";

export interface TourPanels {
  lit: TourRect[];
  asides: TourAsideView[];
  /** What the guard protects, or `null` when the step lights nothing read-only. */
  guard: ReadOnlyScope | null;
}

export function resolvePanels(step: TourStep, obs: TourObservation, targets: Element[]): TourPanels {
  // The read-only areas: lit at their own size — no padding, a panel runs to the
  // window's edge — and handed to the guard with the step's own targets, which
  // stay fully clickable inside them.
  const regions = stepReadOnly(step, obs).map(query).filter(visible);
  const guard = regions.length > 0 ? { regions, open: targets, explore: step.explore ?? [] } : null;

  // Side bubbles: only once the step holds, and only those whose target is on
  // screen — an aside is a remark, never something to wait for.
  const asides = stepAsides(step, obs).flatMap((aside) => {
    const el = query(aside.target);
    return visible(el) ? [{ target: aside.target, text: aside.text, rect: rectOf(el) }] : [];
  });
  return { lit: regions.map(rectOf), asides, guard };
}
