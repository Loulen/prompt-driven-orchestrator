import { useEditStore } from "../pipeline/store/editStore";
import { useLibassistLifecycle } from "../session/store/useLibassistLifecycle";

/** The pipeline the Assistant authors, and the Assistant's lifecycle. Returns
 *  the target id, `null` when the active tab is a Run (or there is none). */
export function useAssistantTarget(): string | null {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const editTab = openTabs.find((t) => t.id === activeTabId);

  // #302 / ADR-0048: the Assistant authors a library *template*, so it targets the
  // active edit tab's pipeline id + scope — never a run. `null` on a run tab hides
  // the Assistant tab (the Manager tab covers a run instead).
  const assistantId = editTab && editTab.scope !== "run" ? editTab.id : null;
  // Paired with `assistantId`, deliberately: a scope of `"run"` alongside a null
  // id would be a half-fact, and it would churn the lifecycle effect on every
  // hop to a Run for nothing.
  const assistantScope = assistantId ? editTab?.scope : undefined;

  // #594 / ADR-0051 §4: the reap fires when the user leaves **every** edit view —
  // so it reads the open tabs, not the active one. Keying it on the active tab
  // (the first cut) meant a glance at a Run threw the conversation away while two
  // templates were still open, which is the regret this issue is made of.
  const hasTemplateTab = openTabs.some((t) => t.scope !== "run");

  // The assistant's whole lifecycle hangs here, and not down in the Assistant
  // tab. This is the right altitude for it: the tab unmounts on every panel close
  // (#385), the user does not stop editing when it does, and a lifecycle keyed on
  // the tab is what made the assistant lose its conversation on each round trip
  // between two templates.
  useLibassistLifecycle(assistantId, assistantScope, hasTemplateTab);
  return assistantId;
}
