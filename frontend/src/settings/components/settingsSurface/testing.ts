/**
 * Test helpers for the Settings surface: a setting row by its id, the tooltip of its
 * name (help, then the source of the value), its switch.
 */
import { screen, within } from "@testing-library/react";

export const rowOf = (id: string): HTMLElement => screen.getByTestId(`setting-row-${id}`);

/** The tooltip of the row's name: its help, then which tier the value comes from. */
export const tipOf = (id: string): string =>
  rowOf(id).querySelector(".set-row__name")?.getAttribute("title") ?? "";

export const switchOf = (id: string): HTMLElement => within(rowOf(id)).getByRole("switch");

/** The tooltip of a row's name, once the row is there. */
export async function tipFor(id: string): Promise<string> {
  await screen.findByTestId(`setting-row-${id}`);
  return tipOf(id);
}

/** A row's switch, once the row is there. */
export async function switchFor(id: string): Promise<HTMLElement> {
  await screen.findByTestId(`setting-row-${id}`);
  return switchOf(id);
}

/** Whether a switch is on. */
export const isOn = (el: HTMLElement): boolean => el.getAttribute("aria-checked") === "true";
