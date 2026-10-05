/**
 * The read-only guard of a tour step (#911, after the human test of *Overview*).
 *
 * A gesture opens a panel — the node inspector, the edge panel, Start's input —
 * and the reader should be able to look around in it: switch tabs, scroll, unfold
 * the prompt. What they must not do, while a tour is showing them around, is
 * *change* anything: type into a field, flip a toggle, pick another value, press
 * Save or Delete. The Projecteur lights those panels; this module keeps them
 * read-only.
 *
 * It works from **capture-phase listeners on `window`**, so it runs before any of
 * the app's own handlers (React listens on its root, below `window`) and the app
 * never has to know a tour exists. Nothing is mutated on the elements themselves
 * — no `readOnly` flag to forget to restore when a step ends mid-keystroke.
 *
 * The rules, in order:
 *
 * 1. Only events aimed **inside a read-only area** are looked at, and never those
 *    inside the step's own target: the thing the card asks you to click keeps
 *    every click (the `code` row lives in the very inspector that is read-only).
 * 2. **Editing** is refused in any editable element: typing, deleting, pasting,
 *    cutting, dropping — and a pointer may not even **focus** one. Several
 *    fields commit on blur (`NameInput`), and a focus-then-blur with the value
 *    untouched still marks the pipeline dirty: seen in the browser (#911), the
 *    Save button lit up after the reader merely clicked a name. A field that
 *    never takes focus never blurs. Should one get focus anyway (Tab), its
 *    caret keys, Tab, Escape and copy stay free.
 * 3. **Controls** are refused by default — a button can save or delete, and
 *    there is no telling which from the outside — except the ones that only
 *    navigate: tabs, `<summary>`, a disclosure that folds content in place (an
 *    `aria-expanded` that opens no popup), and whatever the step lists as
 *    `explore`. Default-deny is the robust side: an unknown button that does
 *    nothing is a small surprise; an unknown button that deletes is not.
 */

/** What the guard is protecting right now. */
export interface ReadOnlyScope {
  /** The lit, read-only areas. */
  regions: Element[];
  /** The step's own targets: fully interactive, even inside a region. */
  open: Element[];
  /** Selectors of extra navigation controls the step lets through. */
  explore: string[];
}

const EDITABLE = 'input, textarea, select, [contenteditable]:not([contenteditable="false"])';

/** Input types that are really buttons or value pickers, not text fields. */
const NON_TEXT_INPUTS = new Set(["button", "submit", "reset", "checkbox", "radio", "file", "range", "color", "image"]);

/**
 * Anything a click can act through. A text field is not in here: clicking one
 * only focuses it (selecting and copying its value is reading it), and typing is
 * refused separately.
 */
const CONTROL = [
  "button",
  "select",
  "summary",
  '[role="button"]',
  '[role="tab"]',
  '[role="checkbox"]',
  '[role="switch"]',
  '[role="radio"]',
  '[role="combobox"]',
  '[role="menuitem"]',
  '[role="menuitemcheckbox"]',
  '[role="menuitemradio"]',
  '[role="option"]',
  '[role="slider"]',
  ...[...NON_TEXT_INPUTS].map((type) => `input[type="${type}"]`),
].join(", ");

/** Controls that only move the reader around: they change what is shown, never a value. */
const NAVIGATION = '[role="tab"], summary, [aria-expanded]:not([aria-haspopup]), [aria-expanded][aria-haspopup="false"]';

/** Keys that move a caret or focus without writing anything. */
const READING_KEYS = new Set([
  "Tab",
  "Escape",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "Shift",
  "Control",
  "Alt",
  "Meta",
  "CapsLock",
]);

function elementOf(target: EventTarget | null): Element | null {
  if (target instanceof Element) return target;
  // A text node (a drop, a selection) answers for its parent.
  if (target instanceof Node) return target.parentElement;
  return null;
}

/** The region the element is in, unless it sits in the step's own target. */
function guardedRegion(el: Element, scope: ReadOnlyScope): Element | null {
  if (scope.open.some((open) => open.contains(el))) return null;
  return scope.regions.find((region) => region.contains(el)) ?? null;
}

function isEditable(el: Element): Element | null {
  const editable = el.closest(EDITABLE);
  if (!editable) return null;
  if (editable instanceof HTMLInputElement && NON_TEXT_INPUTS.has(editable.type)) return null;
  return editable;
}

function explores(control: Element, scope: ReadOnlyScope): boolean {
  if (control.matches(NAVIGATION)) return true;
  return scope.explore.some((selector) => {
    try {
      return control.closest(selector) !== null;
    } catch {
      return false;
    }
  });
}

/** The control a pointer event would act through, when the guard refuses it. */
function refusedControl(el: Element, region: Element, scope: ReadOnlyScope): Element | null {
  const control = el.closest(CONTROL);
  if (!control || !region.contains(control)) return null;
  return explores(control, scope) ? null : control;
}

/** Does the guard refuse this pointer event (press, release, click)? */
export function refusesPointer(event: Event, scope: ReadOnlyScope): boolean {
  const el = elementOf(event.target);
  if (!el) return false;
  const region = guardedRegion(el, scope);
  if (!region) return false;
  // No focus for a field: the press is what would give it one (rule 2).
  if (isEditable(el)) return true;
  return refusedControl(el, region, scope) !== null;
}

/** Does the guard refuse this keystroke? */
export function refusesKey(event: KeyboardEvent, scope: ReadOnlyScope): boolean {
  const el = elementOf(event.target);
  if (!el) return false;
  const region = guardedRegion(el, scope);
  if (!region) return false;
  const editable = isEditable(el);
  if (editable) {
    // A `<select>` changes its value on the arrow keys: only focus may leave it.
    if (editable instanceof HTMLSelectElement) return event.key !== "Tab" && event.key !== "Escape";
    if (READING_KEYS.has(event.key)) return false;
    // Copy and select-all read; every other shortcut (paste, cut, undo) writes.
    if ((event.ctrlKey || event.metaKey) && ["c", "a"].includes((event.key ?? "").toLowerCase())) return false;
    return true;
  }
  // Enter / Space on a focused control press it — the keyboard's click.
  if (event.key === "Enter" || event.key === " ") return refusedControl(el, region, scope) !== null;
  return false;
}

/** Does the guard refuse this edit (`beforeinput`, paste, cut, drop)? */
export function refusesEdit(event: Event, scope: ReadOnlyScope): boolean {
  const el = elementOf(event.target);
  if (!el) return false;
  if (!guardedRegion(el, scope)) return false;
  return isEditable(el) !== null;
}

const POINTER_EVENTS = ["pointerdown", "mousedown", "pointerup", "mouseup", "click", "dblclick", "auxclick"] as const;
const EDIT_EVENTS = ["beforeinput", "paste", "cut", "drop"] as const;

/**
 * Install the guard on `win` until the returned function is called. `scope` is
 * read on every event, so a step that re-aims — or ends — takes effect at once;
 * `null` lets everything through.
 */
export function installReadOnlyGuard(scope: () => ReadOnlyScope | null, win: Window = window): () => void {
  const refuse = (event: Event) => {
    event.preventDefault();
    // Before React, which listens below `window`: the app's own handler never runs.
    event.stopImmediatePropagation();
  };
  const onPointer = (event: Event) => {
    const current = scope();
    if (current && refusesPointer(event, current)) refuse(event);
  };
  const onKey = (event: Event) => {
    const current = scope();
    if (current && event instanceof KeyboardEvent && refusesKey(event, current)) refuse(event);
  };
  const onEdit = (event: Event) => {
    const current = scope();
    if (current && refusesEdit(event, current)) refuse(event);
  };
  for (const type of POINTER_EVENTS) win.addEventListener(type, onPointer, true);
  win.addEventListener("keydown", onKey, true);
  for (const type of EDIT_EVENTS) win.addEventListener(type, onEdit, true);
  return () => {
    for (const type of POINTER_EVENTS) win.removeEventListener(type, onPointer, true);
    win.removeEventListener("keydown", onKey, true);
    for (const type of EDIT_EVENTS) win.removeEventListener(type, onEdit, true);
  };
}
