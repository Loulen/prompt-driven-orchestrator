/**
 * Undo and redo, and the pipeline on disk: save, rename, remove, reload, the library binding.
 */

import type { StoreApi } from "zustand";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { PipelineScope } from "../model/pipeline";
import { ApiError } from "../../shared/http/client";
import { fetchRunPipeline, saveRunPipeline } from "../../run/api/runPipeline";
import { appliedCostCaps, applyCostCaps, keepFollowed, type CostCapsApplied } from "./runCostCapSave";
import type { RunCostCapBaseline } from "../model/runCostCaps";
import { fetchPipeline, savePipeline, deletePipeline as apiDeletePipeline, renamePipeline as apiRenamePipeline } from "../api/pipelines";
import { serializePipeline } from "../../lib/serializePipeline";
import type { EditState, OpenPipeline } from "./editState";
import { isRunTabLocked } from "./editState";
import { clearedHistory, droppedHistory, rekeyTab } from "./tabHistory";
import { draftSessionOf } from "./draftSessions";
import type { SaveAs } from "../api/versions";

type Set = StoreApi<EditState>["setState"];
type Get = StoreApi<EditState>["getState"];

/** `tab` without the notices of its draft whose ids are in `ids`. */
function withoutNotices(tab: OpenPipeline, ids: ReadonlySet<number>): OpenPipeline {
  if (!tab.draft || ids.size === 0) return tab;
  return { ...tab, draft: { ...tab.draft, notices: tab.draft.notices.filter((n) => !ids.has(n.id)) } };
}

export function saveSlice(set: Set, get: Get): Partial<EditState> {
  // The lint above the canvas is the daemon's parse of the file as it was
  // opened: once a save has written the fix (a broken edge deleted, CORE-88),
  // re-read it, or the banner keeps naming what is gone until a reload. Only
  // when there is lint to clear; a failed read keeps it, the save stands.
  const refreshStaleLint = async (id: string) => {
    const tab = get().openTabs.find((t) => t.id === id);
    if (!tab || tab.diagnostics.length === 0) return;
    try {
      const detail = tab.runId ? await fetchRunPipeline(tab.runId) : await fetchPipeline(id, tab.scope);
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id ? { ...t, diagnostics: detail.diagnostics ?? [] } : t,
        ),
      }));
    } catch {
      // keep the lint
    }
  };
  // `took`: the tab's baseline when the save started. A loop the tab followed
  // meanwhile (the Run moved during the save) keeps what it followed.
  const sayCostCaps = (id: string, took: RunCostCapBaseline | undefined, { message, baseline }: CostCapsApplied) => {
    if (message == null && baseline == null) return;
    set((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === id
          ? {
              ...t,
              ...(message != null ? { saveError: { message } } : {}),
              ...(baseline && t.runEditing ? { runCostCaps: keepFollowed(baseline, took, t.runCostCaps) } : {}),
            }
          : t,
      ),
    }));
  };
  return {
  undo: () => set((s) => {
    // ADR-0088: a shared draft undoes my own changes only.
    const draft = draftSessionOf(s.activeTabId);
    if (draft) {
      draft.undoMine();
      return {};
    }
    const tabId = s.activeTabId;
    if (!tabId) return {};
    const h = s.history[tabId];
    if (!h || h.past.length === 0) return {};
    const idx = s.openTabs.findIndex((t) => t.id === tabId);
    if (idx < 0 || isRunTabLocked(s.openTabs[idx])) return {};
    const current = s.openTabs[idx].pipeline;
    const prev = h.past[h.past.length - 1];
    const tabs = [...s.openTabs];
    // dirty:true always — undo is an edit; no content-hash dirty-clearing (#226).
    tabs[idx] = { ...tabs[idx], pipeline: prev, dirty: true };
    return {
      openTabs: tabs,
      history: {
        ...s.history,
        [tabId]: {
          past: h.past.slice(0, -1),
          future: [...h.future, current],
          // Reset so the next edit never coalesces across an undo boundary.
          lastKey: null,
          lastAt: 0,
        },
      },
      // The edge selection key is positional and goes stale after an undo that
      // changes the edge list (subagent #5) — same reason delete clears it.
      selection: { kind: "none", id: null },
    };
  }),

  redo: () => set((s) => {
    const draft = draftSessionOf(s.activeTabId);
    if (draft) {
      draft.redoMine();
      return {};
    }
    const tabId = s.activeTabId;
    if (!tabId) return {};
    const h = s.history[tabId];
    if (!h || h.future.length === 0) return {};
    const idx = s.openTabs.findIndex((t) => t.id === tabId);
    if (idx < 0 || isRunTabLocked(s.openTabs[idx])) return {};
    const current = s.openTabs[idx].pipeline;
    const next = h.future[h.future.length - 1];
    const tabs = [...s.openTabs];
    tabs[idx] = { ...tabs[idx], pipeline: next, dirty: true };
    return {
      openTabs: tabs,
      history: {
        ...s.history,
        [tabId]: {
          past: [...h.past, current],
          future: h.future.slice(0, -1),
          lastKey: null,
          lastAt: 0,
        },
      },
      selection: { kind: "none", id: null },
    };
  }),

  removePipeline: async (id: string, scope?: PipelineScope) => {
    // Pass the entry's scope so a `library` delete hits the library store, not
    // the same-named repo `.yaml` + `.prompts/` that would otherwise be
    // destroyed (#216).
    await apiDeletePipeline(id, scope);
    set((s) => {
      const openTabs = s.openTabs.filter((t) => t.id !== id);
      let activeTabId = s.activeTabId;
      if (s.activeTabId === id) {
        activeTabId = openTabs.length > 0 ? openTabs[openTabs.length - 1].id : null;
      }
      // The merged /pipelines list can hold the same id under two scopes (a repo
      // pipeline and its promoted `library` copy). Drop only the entry whose
      // scope was deleted, so deleting the library row doesn't also blank the
      // surviving repo row (#216). With no scope, fall back to id-only removal.
      const removed = (p: PipelineListEntry) =>
        p.id === id && (scope === undefined || p.scope === scope);
      return {
        pipelines: s.pipelines.filter((p) => !removed(p)),
        openTabs,
        activeTabId,
        selection: s.activeTabId === id ? { kind: "none" as const, id: null } : s.selection,
        // DROP undo history — this path closes the tab inline, bypassing closeTab.
        history: droppedHistory(s.history, id),
      };
    });
  },

  // #774 — rename a pipeline: the daemon moves `<id>.yaml` + its `.prompts/`
  // sidecar to the slug of the new name (refusing collisions and active-run
  // renames), and answers with the final id. The list refreshes under the new
  // id and the open tab — if the renamed pipeline is open — is rekeyed to it
  // with its undo history intact: a rename is an identity change, not a
  // content change, so the user's work follows.
  renamePipeline: async (id: string, name: string) => {
    const result = await apiRenamePipeline(id, name);
    await get().loadPipelines();
    const newId = result?.id && result.id !== id ? result.id : null;
    if (!newId) return;
    const tab = get().openTabs.find((t) => t.id === id);
    set((s) =>
      rekeyTab(s, id, newId, tab ? { pipeline: { ...tab.pipeline, name: result.name ?? name } } : undefined),
    );
  },

  save: async (id: string, version?: SaveAs) => {
    const tab = get().openTabs.find((t) => t.id === id);
    // ADR-0088: a version shown read only has nothing to save.
    if (!tab || tab.version) return undefined;
    try {
      const yaml = serializePipeline(tab.pipeline);
      if (tab.runId) {
        // A cost cap changed for this run goes through `set_region_max_cost`
        // (an override the Run keeps), which is also what resumes a Run
        // parked « budget reached ». What the Run applied before the save
        // tells which caps this save changes.
        const before = await appliedCostCaps(tab.runId, tab.pipeline);
        await saveRunPipeline(tab.runId, yaml, tab.prompts);
        set((s) => ({
          openTabs: s.openTabs.map((t) =>
            t.id === id ? { ...t, dirty: false, saveError: undefined } : t,
          ),
          lastSavedAt: { ...s.lastSavedAt, [id]: Date.now() },
        }));
        sayCostCaps(id, tab.runCostCaps, await applyCostCaps(tab.runId, before, tab.pipeline, tab.runCostCaps));
        await refreshStaleLint(id);
        return { ok: true, id };
      } else {
        // Save back into the same store the tab was opened from, so a
        // `library`-scoped edit never overwrites a same-named repo file (#216).
        // ADR-0088: a tab with a shared draft saves the draft as it holds it,
        // as the version `version` says; the daemon then counts no change left.
        const draft = draftSessionOf(id)?.content();
        // What the notices said changed is in the draft this save freezes:
        // they give way once it is a version. One heard meanwhile stays.
        const absorbed = new Set(draft ? (tab.draft?.notices ?? []).map((n) => n.id) : []);
        const result = await savePipeline(id, yaml, tab.prompts, tab.scope, draft, draft ? version : undefined);
        // #774 — the save may have RENAMED the pipeline (a `name:` edit moved
        // the file): rekey the tab to the daemon's final id in the same
        // gesture, so the store never keeps a tab pointing at a dead stem.
        const newId = result?.id && result.id !== id ? result.id : null;
        if (newId) {
          set((s) => {
            const patch = rekeyTab(s, id, newId);
            return {
              ...patch,
              openTabs: (patch.openTabs ?? s.openTabs).map((t) =>
                t.id === newId ? withoutNotices({ ...t, dirty: false, saveError: undefined }, absorbed) : t,
              ),
              lastSavedAt: { ...patch.lastSavedAt, [newId]: Date.now() },
            };
          });
        } else {
          set((s) => ({
            openTabs: s.openTabs.map((t) =>
              t.id === id ? withoutNotices({ ...t, dirty: false, saveError: undefined }, absorbed) : t,
            ),
            lastSavedAt: { ...s.lastSavedAt, [id]: Date.now() },
          }));
        }
        await refreshStaleLint(newId ?? id);
        // The list shows the new version, and the pipeline it kept.
        if (result?.version || result?.kept) void get().loadPipelines();
        return result;
      }
    } catch (err: unknown) {
      // One typed error contract (`ApiError`) — no more sniffing an `unknown`
      // shape. `status`/`line` are typed fields; a non-`ApiError` (unexpected)
      // still degrades gracefully to its `Error.message`.
      const apiErr = err instanceof ApiError ? err : null;
      const status = apiErr?.status;
      // A 404 on a run-scoped PUT means the run was archived (its pipeline.yaml
      // was deleted) while this tab was still open and dirty. There's nothing
      // left to save into — silently close the tab rather than surfacing a
      // confusing save-error modal that the user would associate with whatever
      // action triggered the flush (e.g. Launch new run).
      if (status === 404 && tab.runId) {
        set((s) => {
          const tabs = s.openTabs.filter((t) => t.id !== id);
          const lastSavedAt = { ...s.lastSavedAt };
          delete lastSavedAt[id];
          let activeTabId = s.activeTabId;
          let selection = s.selection;
          if (s.activeTabId === id) {
            activeTabId = tabs.length > 0 ? tabs[tabs.length - 1].id : null;
            selection = { kind: "none" as const, id: null };
          }
          // DROP undo history — this run tab is self-closing (its on-disk
          // pipeline was archived away), so its stack must go with it.
          return { openTabs: tabs, activeTabId, selection, lastSavedAt, history: droppedHistory(s.history, id) };
        });
        return undefined;
      }
      const message =
        apiErr?.message ?? (err instanceof Error ? err.message : "Save failed");
      const line = apiErr?.line;
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id ? { ...t, saveError: { message, line } } : t,
        ),
      }));
      return undefined;
    }
  },

  clearSaveError: (id: string) => {
    set((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === id ? { ...t, saveError: undefined } : t,
      ),
    }));
  },

  reloadPipeline: async (id: string) => {
    try {
      const tab = get().openTabs.find((t) => t.id === id);
      // ADR-0088: the daemon lands a written file in the shared draft, field
      // by field; the tab gets it from there, with no conflict to resolve.
      // Without a shared draft, unsaved work stays as it is.
      if (tab?.draft || tab?.dirty) return;
      const detail = await fetchPipeline(id, tab?.scope);
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id
            ? {
                ...t,
                pipeline: detail.pipeline,
                prompts: detail.prompts,
                diagnostics: detail.diagnostics ?? [],
                dirty: false,
                externalDirty: true,
              }
            : t,
        ),
        // CLEAR undo history — a clean external hot-reload replaced this tab's
        // pipeline with foreign content; the old stack can't cross that boundary.
        history: clearedHistory(s.history, id),
      }));
      setTimeout(() => {
        set((s) => ({
          openTabs: s.openTabs.map((t) =>
            t.id === id ? { ...t, externalDirty: false } : t,
          ),
        }));
      }, 2000);
    } catch {
      // ignore
    }
  },

  setLibraryBinding: (tabId, libraryId, libraryScope) => {
    set((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === tabId ? { ...t, libraryId, libraryScope } : t,
      ),
    }));
  },

  reloadFromLibrary: async (tabId: string, libraryYaml: string) => {
    const tab = get().openTabs.find((t) => t.id === tabId);
    if (!tab) return;
    try {
      if (tab.runId) {
        await saveRunPipeline(tab.runId, libraryYaml, tab.prompts);
        const detail = await fetchRunPipeline(tab.runId);
        set((s) => ({
          openTabs: s.openTabs.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  pipeline: detail.pipeline,
                  prompts: detail.prompts,
                  diagnostics: detail.diagnostics ?? [],
                  dirty: false,
                  saveError: undefined,
                }
              : t,
          ),
          lastSavedAt: { ...s.lastSavedAt, [tabId]: Date.now() },
          // CLEAR — "Reload changes" overwrote this run tab with the library YAML.
          history: clearedHistory(s.history, tabId),
        }));
      } else {
        const result = await savePipeline(tabId, libraryYaml, tab.prompts, tab.scope);
        // #774 — the save may have moved the file to a new stem; everything
        // below must address the tab (and rekey its slots) under the final id.
        const newId = result?.id && result.id !== tabId ? result.id : tabId;
        const detail = await fetchPipeline(newId, tab.scope);
        set((s) => {
          const patch = newId !== tabId ? rekeyTab(s, tabId, newId) : {};
          return {
            ...patch,
            openTabs: (patch.openTabs ?? s.openTabs).map((t) =>
              t.id === newId
                ? {
                    ...t,
                    pipeline: detail.pipeline,
                    prompts: detail.prompts,
                    diagnostics: detail.diagnostics ?? [],
                    dirty: false,
                    saveError: undefined,
                  }
                : t,
            ),
            lastSavedAt: { ...(patch.lastSavedAt ?? s.lastSavedAt), [newId]: Date.now() },
            // CLEAR — "Reload changes" overwrote this tab with the library YAML.
            history: clearedHistory(patch.history ?? s.history, newId),
          };
        });
      }
    } catch (err: unknown) {
      const apiErr = err instanceof ApiError ? err : null;
      const message =
        apiErr?.message ?? (err instanceof Error ? err.message : "Reload failed");
      const line = apiErr?.line;
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === tabId ? { ...t, saveError: { message, line } } : t,
        ),
      }));
    }
  },
  };
}
