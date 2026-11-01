/**
 * Drives a kit `Select` the way a user does: open its trigger, read or pick an
 * option by its label. `cell` is the element that holds the trigger (a
 * test-id'd wrapper), so a row of several selects is addressed one by one.
 */
import { fireEvent, screen, within } from "@testing-library/react";

/** The trigger button of the kit Select inside `cell`. */
export function selectTrigger(cell: HTMLElement): HTMLElement {
  return within(cell).getByRole("button");
}

/** The label the trigger shows: the current option's. */
export function selectedLabel(cell: HTMLElement): string {
  return selectTrigger(cell).textContent ?? "";
}

function open(cell: HTMLElement): HTMLElement {
  const trigger = selectTrigger(cell);
  if (trigger.getAttribute("aria-expanded") !== "true") fireEvent.click(trigger);
  return screen.getByRole("listbox", { name: trigger.getAttribute("aria-label") ?? undefined });
}

function close(cell: HTMLElement) {
  const trigger = selectTrigger(cell);
  if (trigger.getAttribute("aria-expanded") === "true") fireEvent.keyDown(trigger, { key: "Escape" });
  if (trigger.getAttribute("aria-expanded") === "true") fireEvent.click(trigger);
}

/** The options of the list: the label, its hint, and the group it sits under. */
export function selectOptions(cell: HTMLElement): { label: string; hint: string | null; group: string | null }[] {
  const list = open(cell);
  const out: { label: string; hint: string | null; group: string | null }[] = [];
  let group: string | null = null;
  for (const el of Array.from(list.children)) {
    if (el.getAttribute("role") !== "option") {
      group = el.textContent || null;
      continue;
    }
    const texts = Array.from(el.querySelectorAll("span"))
      .map((span) => span.textContent ?? "")
      .filter((text) => text !== "");
    out.push({ label: texts[0] ?? "", hint: texts[1] ?? null, group });
  }
  close(cell);
  return out;
}

/** Picks the option whose label is exactly `label`. */
export function pickOption(cell: HTMLElement, label: string) {
  const list = open(cell);
  const option = within(list)
    .getAllByRole("option")
    .find((o) => within(o).queryByText(label, { exact: true }) != null);
  if (!option) throw new Error(`no option « ${label} »`);
  fireEvent.click(option);
}
