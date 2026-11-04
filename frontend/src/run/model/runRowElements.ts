/**
 * The Éléments masquables of a Run's row in the Runs list (surface
 * `runs.list`): what a switch of Settings › Appearance hides. Its status and its
 * name are fixed. The extensions' contributions at {@link RUN_ROW_LOCATIONS} are
 * elements of its second line too, one each.
 */
import type { RowElement } from "../../lib/appearance";
import type { UiLocation } from "../../shared/generated/UiLocation";

export const RUN_ROW = {
  trigger: "runs.list.trigger",
  childPills: "runs.list.child-pills",
  nodePips: "runs.list.node-pips",
  pipeline: "runs.list.pipeline",
  parent: "runs.list.parent",
  duration: "runs.list.duration",
  cost: "runs.list.cost",
} as const;

export type { RowElement } from "../../lib/appearance";

export const RUN_ROW_FIRST_LINE: readonly RowElement[] = [
  { id: RUN_ROW.trigger, label: "Trigger" },
  { id: RUN_ROW.childPills, label: "Child runs" },
  { id: RUN_ROW.nodePips, label: "Node progress" },
];

export const RUN_ROW_SECOND_LINE: readonly RowElement[] = [
  { id: RUN_ROW.pipeline, label: "Pipeline" },
  { id: RUN_ROW.parent, label: "Parent link" },
  { id: RUN_ROW.duration, label: "Duration" },
  { id: RUN_ROW.cost, label: "Cost" },
];

/** The extensions' locations of a Run's row: its marks, then its people. */
export const RUN_ROW_LOCATIONS: readonly UiLocation[] = ["run/item", "run/item/people"];

/**
 * Whether a Run's row keeps its second line: one of its elements is shown
 * (`shown` says so of an element id), whatever a given Run holds. So every row
 * of a list has the same height, set by the Apparence alone. The contributions
 * are those of the extensions loaded: until one is, it does not count.
 */
export function hasSecondLine(shown: (id: string) => boolean, contributionIds: readonly string[]): boolean {
  return RUN_ROW_SECOND_LINE.some((element) => shown(element.id)) || contributionIds.some(shown);
}

/** Whether `id` is an element of a Run row's second line: a core one or a contribution. */
export function isSecondLineElement(id: string, contributionIds: readonly string[]): boolean {
  return RUN_ROW_SECOND_LINE.some((element) => element.id === id) || contributionIds.includes(id);
}
