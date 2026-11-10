/**
 * What a tour step lights, marked in the DOM (`data-tour-lit`) so the
 * stylesheet can show a control that otherwise appears only on hover: a lit
 * section's « Add output » `+` sat at opacity 0, and the hole framed an empty
 * box (FP of CORE-227). One tour runs at a time, so the marks are this module's.
 */

export const LIT_ATTR = "data-tour-lit";

let marked: Element[] = [];

/** Mark `elements` as lit, and unmark what the previous aim lit and this one does not. */
export function markLit(elements: Element[]): void {
  for (const e of marked) if (!elements.includes(e)) e.removeAttribute(LIT_ATTR);
  for (const e of elements) if (!e.hasAttribute(LIT_ATTR)) e.setAttribute(LIT_ATTR, "");
  marked = [...elements];
}

/** The tour no longer aims at anything. */
export function clearLit(): void {
  markLit([]);
}
