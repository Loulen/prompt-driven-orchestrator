/**
 * The Éléments masquables of a Pipeline's row in the Pipelines list (surface
 * `pipelines.list`): what a switch of Settings › Appearance hides. Its name and
 * its select control are fixed, and so is the lint of an invalid one. The
 * extensions' contributions at {@link PIPELINE_ROW_LOCATIONS} are elements of
 * its second line, one each.
 */
import type { UiLocation } from "../../shared/generated/UiLocation";
import type { RowElement } from "../../lib/appearance";

export const PIPELINE_ROW = {
  version: "pipelines.list.version",
  draft: "pipelines.list.draft",
  modified: "pipelines.list.modified",
  validation: "pipelines.list.validation",
} as const;

/** Its switches, in the order of Settings › Appearance. */
export const PIPELINE_ROW_ELEMENTS: readonly RowElement[] = [
  { id: PIPELINE_ROW.version, label: "Version" },
  { id: PIPELINE_ROW.draft, label: "Draft marker" },
  { id: PIPELINE_ROW.modified, label: "Modified" },
  { id: PIPELINE_ROW.validation, label: "Validation" },
];

const SECOND_LINE: readonly string[] = [PIPELINE_ROW.version, PIPELINE_ROW.draft, PIPELINE_ROW.modified];

/** The extensions' location of a Pipeline's row, on its second line. */
export const PIPELINE_ROW_LOCATIONS: readonly UiLocation[] = ["pipeline/item"];

/**
 * Whether a Pipeline's row keeps its second line: one of its elements, or one
 * of the loaded extensions' contributions, is shown — the Apparence decides,
 * not the Pipeline, so every row of the list has one height.
 */
export function hasSecondLine(shown: (id: string) => boolean, contributionIds: readonly string[]): boolean {
  return SECOND_LINE.some(shown) || contributionIds.some(shown);
}

export function isSecondLineElement(id: string, contributionIds: readonly string[]): boolean {
  return SECOND_LINE.includes(id) || contributionIds.includes(id);
}
