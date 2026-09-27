/**
 * The tabs of the editor: loading the list, opening, closing and switching pipelines, the selection.
 */

import type { StoreApi } from "zustand";
import type { PipelineScope } from "../model/pipeline";
import { fetchRunPipeline } from "../../run/api/runPipeline";
import { fetchPipeline, fetchPipelines } from "../api/pipelines";
import { loadTabsDisabled, saveTabsDisabled } from "../../lib/uiPrefs";
import type { EditState } from "./editState";
import { hasUnsavedWork } from "./editState";
import type { OpenPipeline, Selection } from "./editState";
import { droppedHistory, replaceWithTab, placeOpenedTab } from "./tabHistory";

type Set = StoreApi<EditState>["setState"];
type Get = StoreApi<EditState>["getState"];

export function tabsSlice(set: Set, get: Get): Partial<EditState> {
  void get;
  return {
  pipelines: [],
  openTabs: [],
  activeTabId: null,
  selection: { kind: "none", id: null },
  scrollToPort: null,
  lastSavedAt: {},
  history: {},
  singleTabMode: loadTabsDisabled(),
  pendingSingleTab: null,

  loadPipelines: async () => {
    try {
      const pipelines = await fetchPipelines();
      set({ pipelines });
    } catch {
      // ignore
    }
  },

  openPipeline: async (id: string, scope?: PipelineScope) => {
    const existing = get().openTabs.find((t) => t.id === id);
    if (existing) {
      set({ activeTabId: id, selection: { kind: "none", id: null } });
      return;
    }
    try {
      // Pass the list entry's scope so a `library` (or `user`) pipeline opens
      // from its own store rather than a same-named repo file (#216).
      const detail = await fetchPipeline(id, scope);
      const tab: OpenPipeline = {
        id,
        scope: detail.scope,
        pipeline: detail.pipeline,
        prompts: detail.prompts,
        diagnostics: detail.diagnostics ?? [],
        dirty: false,
        externalDirty: false,
        libraryId: null,
        libraryScope: null,
      };
      // Single-tab mode replaces the current tab (or parks for confirmation);
      // multi-tab appends (#342).
      set((s) => placeOpenedTab(s, tab));
    } catch {
      // ignore
    }
  },

  openRunPipeline: async (runId: string) => {
    const tabId = `__run__${runId}`;
    const existing = get().openTabs.find((t) => t.id === tabId);
    if (existing) {
      set({ activeTabId: tabId, selection: { kind: "none", id: null } });
      return;
    }
    try {
      const detail = await fetchRunPipeline(runId);
      const tab: OpenPipeline = {
        id: tabId,
        scope: "run",
        pipeline: detail.pipeline,
        prompts: detail.prompts,
        diagnostics: detail.diagnostics ?? [],
        dirty: false,
        externalDirty: false,
        runId,
        libraryId: null,
        libraryScope: null,
      };
      // Single-tab mode replaces the current tab (or parks for confirmation);
      // multi-tab appends (#342).
      set((s) => placeOpenedTab(s, tab));
    } catch {
      // ignore
    }
  },

  closeRunPipeline: (runId: string) => {
    get().closeTab(`__run__${runId}`);
  },

  closeTab: (id: string) => {
    set((s) => {
      const tabs = s.openTabs.filter((t) => t.id !== id);
      let activeTabId = s.activeTabId;
      if (s.activeTabId === id) {
        activeTabId = tabs.length > 0 ? tabs[tabs.length - 1].id : null;
      }
      // DROP this tab's undo history (ADR-0014): the slot would otherwise leak,
      // and a reused tab id (e.g. reopening `__run__<runId>`) would inherit a
      // stale stack. `closeRunPipeline` routes through here, so it inherits this.
      return {
        openTabs: tabs,
        activeTabId,
        selection: { kind: "none", id: null },
        history: droppedHistory(s.history, id),
      };
    });
  },

  closeTabs: (ids: string[]) => {
    set((s) => {
      const drop = new Set(ids);
      const tabs = s.openTabs.filter((t) => !drop.has(t.id));
      const activeClosed = s.activeTabId != null && drop.has(s.activeTabId);
      // Recompute the active tab ONCE, and only when the active one closed —
      // never point it at a tab in `drop`. Rightmost survivor (mirror
      // `closeTab`/`removePipeline`), or null when everything closed.
      const activeTabId = activeClosed
        ? (tabs.length > 0 ? tabs[tabs.length - 1].id : null)
        : s.activeTabId;
      // Only reset `selection` when the active tab was closed — the
      // `removePipeline` semantics, NOT the unconditional reset of `closeTab`.
      // Keeping the reference stable when a background tab closes avoids a
      // needless render-phase reconciliation pass (#247/#320).
      const selection = activeClosed ? { kind: "none" as const, id: null } : s.selection;
      // DROP each closed tab's undo history by id (ADR-0014). Missing an id
      // would leak its stack and let a reused id inherit a stale one.
      const history = ids.reduce((h, id) => droppedHistory(h, id), s.history);
      return { openTabs: tabs, activeTabId, selection, history };
    });
  },

  setSingleTabMode: (v: boolean) => {
    // Persist AT THE CHANGE (Trap B): the pref is per-client, independent of the
    // daemon and of the numeric Save button. Update the store so the seams read
    // the mode hot without touching localStorage.
    saveTabsDisabled(v);
    set({ singleTabMode: v });
    if (!v) return; // disabling never closes anything — accumulation resumes.
    const s = get();
    if (s.openTabs.length <= 1) return; // already at most one tab
    const victims = s.openTabs.filter((t) => t.id !== s.activeTabId);
    if (victims.length === 0) return;
    if (victims.some(hasUnsavedWork)) {
      // Park (tab === null ⇒ collapse: keep the active tab, close the rest).
      set({ pendingSingleTab: { tab: null, victims } });
    } else {
      get().closeTabs(victims.map((t) => t.id));
    }
  },

  confirmPendingSingleTab: () => {
    const p = get().pendingSingleTab;
    if (!p) return;
    if (p.tab) {
      // Open-replace: apply the parked tab as the sole tab.
      set((s) => replaceWithTab(s, p.tab!, p.victims));
    } else {
      // Enable-collapse: close the others, keep the active tab. `closeTabs`
      // already drops their history and leaves the active selection intact.
      get().closeTabs(p.victims.map((t) => t.id));
      set({ pendingSingleTab: null });
    }
  },

  cancelPendingSingleTab: () => set({ pendingSingleTab: null }),

  setActiveTab: (id: string) => {
    set({ activeTabId: id, selection: { kind: "none", id: null } });
  },

  setSelection: (sel: Selection) => {
    set({ selection: sel });
  },

  setScrollToPort: (port: string | null) => {
    set({ scrollToPort: port });
  },

  };
}
