/**
 * Editing a Run (ADR-0085): a run tab opens in « pilotage », locked; « Edit for
 * this run » unlocks it, « Finish editing » locks it again, and « Overwrite
 * default pipeline » copies the Run's snapshot over the shared pipeline.
 */

import type { StoreApi } from "zustand";
import { fetchRunPipeline, overwriteDefaultPipelineFromRun } from "../../run/api/runPipeline";
import { hasUnsavedWork } from "./editState";
import type { EditState } from "./editState";
import type { EditorPipelineDef } from "../model/pipeline";
import { clearedHistory } from "./tabHistory";
import {
  costCapBaseline,
  foldCostCapOverrides,
  followCostCaps,
  followCostCapsIn,
  runAppliedCaps,
  type CostCapOverrides,
  type RunCapSource,
} from "../model/runCostCaps";

type Set = StoreApi<EditState>["setState"];
type Get = StoreApi<EditState>["getState"];

export function runEditSlice(set: Set, get: Get): Partial<EditState> {
  return {
    startRunEditing: (id: string, run?: RunCapSource | null) => {
      set((s) => ({
        openTabs: s.openTabs.map((t) => {
          if (t.id !== id || t.runId == null || t.runEditing) return t;
          // The overrides only: a `$var` cap keeps its expression in the YAML
          // (the inspector shows what it resolves to).
          const pipeline = foldCostCapOverrides(t.pipeline, run?.region_max_cost_overrides);
          return {
            ...t,
            pipeline,
            runCostCaps: costCapBaseline(pipeline, runAppliedCaps(t.pipeline, run)),
            runEditing: true,
          };
        }),
      }));
    },

    followRunCostCaps: (id: string, overrides: CostCapOverrides | null | undefined) => {
      const tab = get().openTabs.find((t) => t.id === id);
      if (!tab?.runEditing) return;
      const baseline = tab.runCostCaps ?? {};
      const moved = followCostCaps(tab.pipeline, baseline, overrides);
      if (!moved) return;
      // The undo history follows too, or an undo would bring the stale cap back.
      const follow = (p: EditorPipelineDef) => followCostCapsIn(p, baseline, overrides);
      set((s) => {
        const h = s.history[id];
        return {
          openTabs: s.openTabs.map((t) =>
            t.id === id ? { ...t, pipeline: moved.pipeline, runCostCaps: moved.baseline } : t,
          ),
          history: h ? { ...s.history, [id]: { ...h, past: h.past.map(follow), future: h.future.map(follow) } } : s.history,
        };
      });
    },

    finishRunEditing: async (id: string) => {
      const tab = get().openTabs.find((t) => t.id === id);
      if (!tab?.runId) return;
      if (!hasUnsavedWork(tab)) {
        set((s) => ({
          openTabs: s.openTabs.map((t) => (t.id === id ? { ...t, runEditing: false, runCostCaps: undefined } : t)),
          history: clearedHistory(s.history, id),
        }));
        return;
      }
      // Discard: the tab goes back to what the Run actually runs, its snapshot.
      let detail: Awaited<ReturnType<typeof fetchRunPipeline>> | null = null;
      try {
        detail = await fetchRunPipeline(tab.runId);
      } catch {
        // The snapshot is gone (archived meanwhile): lock anyway, no save can
        // reach the tab's content any more.
      }
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id
            ? {
                ...t,
                ...(detail
                  ? { pipeline: detail.pipeline, prompts: detail.prompts, diagnostics: detail.diagnostics ?? [] }
                  : {}),
                dirty: false,
                saveError: undefined,
                runEditing: false,
                runCostCaps: undefined,
              }
            : t,
        ),
        history: clearedHistory(s.history, id),
        selection: s.activeTabId === id ? { kind: "none" as const, id: null } : s.selection,
      }));
    },

    overwriteDefaultPipeline: async (id: string) => {
      const tab = get().openTabs.find((t) => t.id === id);
      if (!tab?.runId) return;
      if (tab.dirty) {
        // « Save for this run » first: the overwrite copies the Run's snapshot
        // on disk, which must be what the user sees.
        await get().save(id);
        const after = get().openTabs.find((t) => t.id === id);
        if (after?.dirty) {
          throw new Error(after.saveError?.message ?? "Save for this run failed");
        }
      }
      const { pipeline_id } = await overwriteDefaultPipelineFromRun(tab.runId);
      // The daemon also broadcasts `pipeline_changed`; reloading here keeps an
      // open tab of the shared pipeline current even with the socket down.
      if (get().openTabs.some((t) => t.id === pipeline_id && t.runId == null)) {
        await get().reloadPipeline(pipeline_id);
      }
      await get().loadPipelines();
    },
  };
}
