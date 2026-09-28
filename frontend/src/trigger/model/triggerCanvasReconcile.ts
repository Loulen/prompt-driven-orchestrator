export interface TriggerCanvasFocusInputs {
  /** The currently-selected Trigger id, or null when none is selected. */
  selectedTriggerId: string | null;
  /** The active edit-tab id (=== pipeline id for a Trigger-opened tab). */
  editActiveTabId: string | null;
  /** The kind of the current canvas selection (`selection.kind`). */
  selectionKind: string;
  /** The tab id that selecting a Trigger opened, or null (the ref value). */
  triggerOpenedTabId: string | null;
}

/**
 * Decide whether a canvas-focus change should clear the selected Trigger (#320).
 *
 * Selecting a Trigger now *also* opens the pipeline it would launch in the
 * canvas (`App.handleSelectTrigger`), which changes the canvas focus. PDO's
 * #247 reconciliation clears the selected Trigger whenever the canvas focus
 * changes — so, naively, the Trigger's own `openPipeline` would immediately
 * close the Trigger detail it just opened.
 *
 * This function is the guard that tells those two apart:
 *  - **Keep** (return `false`) when the focus change is the Trigger's OWN
 *    open — the tab it opened is now active (`editActiveTabId ===
 *    triggerOpenedTabId`) with nothing selected (`selectionKind === "none"`).
 *    This also covers re-selecting a Trigger whose pipeline is already the
 *    active tab: `openPipeline` resets `selection` to `none` without changing
 *    the active tab, so both clauses still hold.
 *  - **Clear** (return `true`) on a genuine #247 reclaim: a node/edge/region
 *    selection makes `selectionKind !== "none"`, and switching to / opening a
 *    different tab makes `editActiveTabId !== triggerOpenedTabId`. Deselecting
 *    to empty on the Trigger's own tab is *not* a reclaim — it keeps the
 *    Trigger, matching #320 intent.
 *  - **No-op** (return `false`) when no Trigger is selected — there is nothing
 *    to clear, so a stale `triggerOpenedTabId` ref is inert.
 */
export function shouldClearTriggerOnCanvasFocus(
  input: TriggerCanvasFocusInputs,
): boolean {
  if (input.selectedTriggerId === null) return false;
  const isTriggerOwnOpen =
    input.triggerOpenedTabId !== null &&
    input.editActiveTabId === input.triggerOpenedTabId &&
    input.selectionKind === "none";
  return !isTriggerOwnOpen;
}
