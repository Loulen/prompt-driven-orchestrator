/**
 * The Éléments masquables of a Trigger's row in the Triggers list (surface
 * `triggers.list`): what a switch of Settings › Appearance hides. Its name, its
 * enable switch and its last result (the select control too) are fixed. The
 * extensions' contributions at {@link TRIGGER_ROW_LOCATIONS} are elements of its
 * first line, one each.
 */
import type { UiLocation } from "../../shared/generated/UiLocation";
import type { RowElement } from "../../run/model/runRowElements";

export const TRIGGER_ROW = {
  pipeline: "triggers.list.pipeline",
  last: "triggers.list.last",
  next: "triggers.list.next",
  guard: "triggers.list.guard",
  overlap: "triggers.list.overlap",
} as const;

/** Its switches, in the order of Settings › Appearance. */
export const TRIGGER_ROW_ELEMENTS: readonly RowElement[] = [
  { id: TRIGGER_ROW.pipeline, label: "Pipeline" },
  { id: TRIGGER_ROW.last, label: "Last fired" },
  { id: TRIGGER_ROW.next, label: "Next fire" },
  { id: TRIGGER_ROW.guard, label: "Guard" },
  { id: TRIGGER_ROW.overlap, label: "Overlap" },
];

/** The elements of its second line; its last result sits there but is fixed. */
const SECOND_LINE: readonly string[] = [TRIGGER_ROW.pipeline, TRIGGER_ROW.last, TRIGGER_ROW.next];

/** The extensions' location of a Trigger's row, on its first line. */
export const TRIGGER_ROW_LOCATIONS: readonly UiLocation[] = ["trigger/item"];

/**
 * Whether a Trigger's row keeps its second line: one of its elements is shown
 * (`shown` says so of an element id), whatever a given Trigger holds, so every
 * row of the list has one height. Without it, the last result moves beside the
 * enable switch.
 */
export function hasSecondLine(shown: (id: string) => boolean): boolean {
  return SECOND_LINE.some(shown);
}

export function isSecondLineElement(id: string): boolean {
  return SECOND_LINE.includes(id);
}
