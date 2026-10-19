/**
 * How the gesture the store is applying folds into the Undo history of a
 * shared draft (ADR-0088): the store notes it, the draft's session reads it
 * when it records the change. A change with no gesture noted (a prompt typed,
 * a run-only field) is not undone by Undo.
 */

export interface DraftGesture {
  tabId: string;
  /** Same key as the previous gesture: one Undo step for both. */
  coalesceKey: string | null;
  /** `false`: folded into the previous step (the stamp of a drawn edge). */
  track: boolean;
}

let pending: DraftGesture | null = null;

export function noteDraftGesture(tabId: string, coalesceKey: string | null, track: boolean): void {
  pending = { tabId, coalesceKey, track };
}

/** The gesture noted for `tabId`, once. */
export function takeDraftGesture(tabId: string): DraftGesture | null {
  const gesture = pending?.tabId === tabId ? pending : null;
  pending = null;
  return gesture;
}
