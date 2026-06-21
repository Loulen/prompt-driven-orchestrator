export type RightPaneOwner = "info" | "trigger" | "editTab" | "selectedNode";

export interface RightPaneInputs {
  /** A Trigger is selected in the left panel — its detail should own the pane. */
  triggerSelected: boolean;
  /** The pipeline/run info overlay is toggled open from the canvas. */
  infoPanelOpen: boolean;
  /** An edit tab (pipeline or run) owns the centre canvas. */
  hasEditTab: boolean;
}

/**
 * Decide which view owns the right-hand detail pane.
 *
 * Precedence (highest first):
 *  1. `info`    — the explicit pipeline/run info overlay, toggled from the canvas.
 *  2. `trigger` — a Trigger is selected in the left panel. It wins over an open
 *                 edit tab: opening a run leaves a *persistent* edit tab behind
 *                 (`hasEditTab` stays true forever after, since the run tab never
 *                 closes on its own), so the previous `selectedTrigger && !hasEditTab`
 *                 guard made the Trigger detail — and its Fire history — permanently
 *                 unreachable once any run had been opened (#247). The canvas
 *                 reclaims the pane from a Trigger by clearing the Trigger
 *                 selection on the next canvas selection / tab switch, NOT through
 *                 this precedence (see the reconciliation effect in App).
 *  3. `editTab` — a node/edge/region selection or the run/pipeline inspector for
 *                 the active edit tab.
 *  4. `selectedNode` — the legacy no-tab node detail path.
 *
 * The four owners are mutually exclusive: callers render exactly one branch.
 */
export function rightPaneOwner(input: RightPaneInputs): RightPaneOwner {
  if (input.infoPanelOpen) return "info";
  if (input.triggerSelected) return "trigger";
  if (input.hasEditTab) return "editTab";
  return "selectedNode";
}
