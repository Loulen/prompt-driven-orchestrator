/**
 * The versions of a registry pipeline in the editor (ADR-0088): the « Save… »
 * window of a shared draft, and a version restored into the draft.
 */

import type { StoreApi } from "zustand";
import { fetchPipelineVersion } from "../api/versions";
import type { EditState } from "./editState";
import { noteDraftGesture } from "./draftGesture";

type Set = StoreApi<EditState>["setState"];
type Get = StoreApi<EditState>["getState"];

/** How long a restore waits for the draft of its pipeline to arrive. */
const DRAFT_WAIT_MS = 5_000;

/** Whether the draft of the tab `tabId` arrives, writable, in time. */
function draftSynced(get: Get, tabId: string): Promise<boolean> {
  const ready = () => {
    const tab = get().openTabs.find((t) => t.id === tabId);
    return tab?.draft?.synced === true && tab.draft.canWrite;
  };
  if (ready()) return Promise.resolve(true);
  return new Promise((resolve) => {
    const started = Date.now();
    const timer = setInterval(() => {
      if (ready()) {
        clearInterval(timer);
        resolve(true);
      } else if (Date.now() - started > DRAFT_WAIT_MS) {
        clearInterval(timer);
        resolve(false);
      }
    }, 50);
  });
}

export function versionsSlice(set: Set, get: Get): Partial<EditState> {
  return {
    saveVersionFor: null,

    requestSave: (id: string) => {
      const tab = get().openTabs.find((t) => t.id === id);
      if (!tab || tab.version) return;
      // A shared draft is frozen as the version the window names.
      if (tab.draft) {
        set({ saveVersionFor: id });
        return;
      }
      void get().save(id);
    },

    closeSaveVersion: () => set({ saveVersionFor: null }),

    restoreVersion: async (pipelineId: string, number: number) => {
      const version = await fetchPipelineVersion(pipelineId, number);
      await get().openPipeline(pipelineId);
      const synced = await draftSynced(get, pipelineId);
      if (!synced) throw new Error("The draft is not open for writing: nothing was restored.");
      set((s) => {
        const idx = s.openTabs.findIndex((t) => t.id === pipelineId);
        if (idx < 0) return {};
        // One change of mine: Undo takes the whole restore back. The name
        // stays the pipeline's: a version from before a rename does not
        // rename it back.
        noteDraftGesture(pipelineId, null, true);
        const tabs = [...s.openTabs];
        const pipeline = { ...version.pipeline, name: tabs[idx].pipeline.name };
        tabs[idx] = { ...tabs[idx], pipeline, prompts: version.prompts };
        return { openTabs: tabs, activeTabId: pipelineId, selection: { kind: "none" as const, id: null } };
      });
    },
  };
}
