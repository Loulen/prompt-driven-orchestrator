export interface InfoPanelReconcileInputs {
  /** The active edit-tab id captured at the previous render (the tracker snapshot). */
  prevTabId: string | null;
  /** The active edit-tab id now. */
  nextTabId: string | null;
  /** Whether the Pipeline Info peek overlay is currently open. */
  infoOpen: boolean;
}

/**
 * Decide whether the Pipeline Info peek overlay should auto-close because the
 * active edit tab changed (#385).
 *
 * The overlay is toggled from the canvas and given TOP precedence by
 * `rightPaneOwner`, so while open it shadows whatever inspector the right pane
 * would otherwise show. Its content is bound to the ACTIVE tab (that tab's
 * pipeline / run), so when the active tab changes the overlay is describing a
 * tab that is no longer in focus — closing it is the coherent choice.
 *
 * Selecting a different run/library-pipeline/trigger-pipeline, and switching
 * tabs, all move `activeTabId` (run tabs are keyed `__run__<runId>`, so two
 * runs of the SAME pipeline still differ). Reselecting the already-active tab
 * leaves `activeTabId` unchanged (`prevTabId === nextTabId`) → keep it open.
 *
 * Keyed on the tab id, NOT on `selection`: the live-run auto-snap effect mutates
 * `selection` (none → node) with no user intent, which would spuriously close.
 */
export function shouldCloseInfoOnTabChange(input: InfoPanelReconcileInputs): boolean {
  return input.infoOpen && input.prevTabId !== input.nextTabId;
}

/** A tab of the Pipeline info panel. */
export type TabId = "info" | "diff" | "repositories" | "manager" | "yaml" | "assistant";

/** What the panel's context offers: a live Run (Diff / Repositories / Manager)
 *  or a library template with an Assistant (#302) — never both. */
export interface InfoTabContext {
  hasRun: boolean;
  hasAssistant: boolean;
}

/**
 * The tab the panel actually SHOWS for a requested tab: a tab the context does
 * not offer (Diff / Repositories / Manager without a Run, Assistant without a
 * template) falls back to Info. The panel renders it and the toolbar lights its
 * buttons from it (#938) — one rule, so the two can never disagree.
 */
export function resolveInfoTab(tab: TabId, ctx: InfoTabContext): TabId {
  if ((tab === "manager" || tab === "diff" || tab === "repositories") && !ctx.hasRun) return "info";
  if (tab === "assistant" && !ctx.hasAssistant) return "info";
  return tab;
}

/** The panel's open state and requested tab, as the app owns them (#938). */
export interface InfoPanelState {
  open: boolean;
  tab: TabId;
}

/**
 * The toolbar's two panel buttons are mutually exclusive (#938, story PDO-3):
 * the "agent" glyph is lit iff the Assistant tab is shown, `(i)` iff the panel
 * is open on any other tab.
 */
export function infoPanelButtons(
  state: InfoPanelState,
  ctx: InfoTabContext,
): { assistantActive: boolean; infoActive: boolean } {
  const shown = resolveInfoTab(state.tab, ctx);
  return {
    assistantActive: state.open && shown === "assistant",
    infoActive: state.open && shown !== "assistant",
  };
}

/** The "agent" glyph: closes the panel when it shows the Assistant, otherwise
 *  opens it (or switches it) on the Assistant tab. */
export function toggleAssistantTab(state: InfoPanelState, ctx: InfoTabContext): InfoPanelState {
  if (infoPanelButtons(state, ctx).assistantActive) return { ...state, open: false };
  return { open: true, tab: "assistant" };
}

/** `(i)`: closes the panel when it is active, otherwise opens it (or switches
 *  it away from the Assistant) on the Info tab. */
export function toggleInfoTab(state: InfoPanelState, ctx: InfoTabContext): InfoPanelState {
  if (infoPanelButtons(state, ctx).infoActive) return { ...state, open: false };
  return { open: true, tab: "info" };
}
