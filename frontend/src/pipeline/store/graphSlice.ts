/**
 * Editing the graph of the active tab: nodes, edges, regions, notes, metadata, prompts.
 */

import type { StoreApi } from "zustand";
import type { EditorNodeDef } from "../model/pipeline";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import type { NoteDef } from "../../shared/generated/NoteDef";
import { generateNodeId } from "../../lib/nanoid";
import { isStructuralMarker } from "../../lib/structuralMarkers";
import {
  generatedRegionId,
  materializeMissingRegions,
  reconcileLoopRegions,
  regionsDestroyedByEdgeRemoval,
} from "../../lib/loopRegions";
import type { EditState } from "./editState";
import { mutateActiveTab, mutateActiveTabWithHistory, edgeReferencesNode, propagatePortChangesToEdges } from "./tabHistory";

type Set = StoreApi<EditState>["setState"];
type Get = StoreApi<EditState>["getState"];

export function graphSlice(set: Set, get: Get): Partial<EditState> {
  void get;
  return {
  addNode: (node: EditorNodeDef) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      tab.pipeline.nodes = [...tab.pipeline.nodes, node];
    }));
  },

  updateNode: (nodeId: string, updates: Partial<EditorNodeDef>) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      const oldNode = tab.pipeline.nodes.find((n) => n.id === nodeId);
      tab.pipeline.nodes = tab.pipeline.nodes.map((n) =>
        n.id === nodeId ? { ...n, ...updates } : n,
      );
      if (oldNode) {
        if (updates.inputs) {
          propagatePortChangesToEdges(tab, nodeId, oldNode.inputs, updates.inputs, "inputs");
        }
        if (updates.outputs) {
          propagatePortChangesToEdges(tab, nodeId, oldNode.outputs, updates.outputs, "outputs");
        }
      }
    }, { coalesceKey: `updateNode:${nodeId}:${Object.keys(updates).sort().join(",")}` }));
  },

  updateNodeViews: (updates: { id: string; x: number; y: number }[]) => {
    if (updates.length === 0) return; // no-op: don't dirty/re-render on an empty drag
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      // Round x/y exactly as the single-node drag did, so a group drag writes
      // the same integer coords. One `set` = one re-derivation = one dirty/save
      // unit. Positions never touch edges, so this skips
      // propagatePortChangesToEdges. Unknown ids match nothing and are ignored
      // (same as updateNode).
      const moved = new Map(
        updates.map((u) => [u.id, { x: Math.round(u.x), y: Math.round(u.y) }]),
      );
      tab.pipeline.nodes = tab.pipeline.nodes.map((n) => {
        const view = moved.get(n.id);
        return view ? { ...n, view } : n;
      });
    }));
  },

  deleteNode: (nodeId: string) => {
    set((s) => {
      // #684: start/end are structural markers, not nodes the user owns. The
      // rule lived only in the context-menu gesture until now; enforcing it here
      // covers every present and future entry point (shortcuts, multi-select,
      // assistant actions).
      if (isStructuralMarker(s.openTabs.find((t) => t.id === s.activeTabId)?.pipeline.nodes.find((n) => n.id === nodeId))) return s;
      return {
      ...mutateActiveTabWithHistory(s, (tab) => {
        tab.pipeline.nodes = tab.pipeline.nodes.filter((n) => n.id !== nodeId);
        tab.pipeline.edges = tab.pipeline.edges.filter((e) => !edgeReferencesNode(e, nodeId));
        // Reconcile loop regions against the removed node (ADR-0011 / #173).
        // Deleting a node also drops the edges that referenced it, which can take
        // a bounded region's last cycle, and always leaves the deleted id
        // dangling in any region's `members`. Mirror the edge path's
        // destroy-on-last-cycle rule (`deleteEdge`): prune the id from every
        // region and drop a bounded region that no longer closes a cycle, so
        // neither an orphan region nor a ghost member id is ever written to the
        // saved pipeline file.
        if (tab.pipeline.loops && tab.pipeline.loops.length > 0) {
          tab.pipeline.loops = reconcileLoopRegions(tab.pipeline);
        }
      }),
      selection: { kind: "none" as const, id: null },
      };
    });
  },

  duplicateNode: (nodeId: string) => {
    set((s) => {
      // #684: a pipeline has exactly one start and one end marker. Bail before
      // the mutator so a refused duplicate neither dirties the tab nor records
      // an empty undo step.
      if (isStructuralMarker(s.openTabs.find((t) => t.id === s.activeTabId)?.pipeline.nodes.find((n) => n.id === nodeId))) return s;
      return mutateActiveTabWithHistory(s, (tab) => {
      const src = tab.pipeline.nodes.find((n) => n.id === nodeId);
      if (!src) return;
      const newId = generateNodeId();
      const srcName = src.name ?? src.id;
      const copy: EditorNodeDef = {
        ...src,
        id: newId,
        name: `${srcName} copy`,
        inputs: src.inputs.map((p) => ({ ...p })),
        outputs: src.outputs.map((p) => ({ ...p })),
        view: src.view ? { x: src.view.x + 40, y: src.view.y + 40 } : { x: 200, y: 200 },
      };
      tab.pipeline.nodes = [...tab.pipeline.nodes, copy];
      });
    });
  },

  addEdge: (edge: EdgeDef) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      tab.pipeline.edges = [...tab.pipeline.edges, edge];
      // Auto-materialize a bounded loop region when this edge closes a cycle
      // (ADR-0011 / #166): a drawn cycle is never accidentally unbounded. Only
      // cycles not already covered by an existing `loops:` entry add a region;
      // acyclic edges add nothing.
      //
      // This is the ONLY store-side materialization, and it is deliberate (#396):
      // it covers the cycle the user just drew on a canvas the daemon has not
      // parsed yet. A pipeline that *arrives* carrying a cycle is already
      // reconciled by the daemon's mirror in `parse_pipeline`, so none of the six
      // load paths below (`openPipeline`, `openRunPipeline`, conflict record,
      // "take theirs", `reloadPipeline`, `reloadFromLibrary`) needs its own call —
      // and the library twin the star compares against is reconciled the same
      // way, which a frontend-only fix would have desynced into a false
      // "diverged".
      const newRegions = materializeMissingRegions(
        tab.pipeline.nodes,
        tab.pipeline.edges,
        tab.pipeline.loops ?? [],
      );
      if (newRegions.length > 0) {
        tab.pipeline.loops = [...(tab.pipeline.loops ?? []), ...newRegions];
      }
    }));
  },

  updateEdge: (index: number, updates: Partial<EdgeDef>, opts?: { track?: boolean }) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      tab.pipeline.edges = tab.pipeline.edges.map((e, i) =>
        i === index ? { ...e, ...updates } : e,
      );
    }, {
      track: opts?.track ?? true,
      coalesceKey: `updateEdge:${index}:${Object.keys(updates).sort().join(",")}`,
    }));
  },

  deleteEdge: (index: number, opts?: { keepSelection?: boolean }) => {
    set((s) => ({
      ...mutateActiveTabWithHistory(s, (tab) => {
        // Destroy-loop on last-cycle removal (ADR-0011 / #150): if this edge was
        // the last cycle of one or more bounded regions, those regions are
        // destroyed — their `loops:` entry (bound + iteration state) goes with
        // the edge. Computed BEFORE the edge is removed, against the live graph.
        // Deleting a non-last cycle edge leaves the loop intact (the list is
        // empty). The confirmation popup is owned by the canvas, which calls
        // this only after the user confirms.
        const destroyed = new Set(regionsDestroyedByEdgeRemoval(tab.pipeline, index));
        tab.pipeline.edges = tab.pipeline.edges.filter((_, i) => i !== index);
        if (destroyed.size > 0 && tab.pipeline.loops) {
          tab.pipeline.loops = tab.pipeline.loops.filter((r) => !destroyed.has(r.id));
        }
      }),
      selection: opts?.keepSelection ? s.selection : { kind: "none" as const, id: null },
    }));
  },

  updateRegion: (regionId: string, updates: Partial<LoopRegion>) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      // Editing a bounded region's `max_iter` round-trips into the `loops:`
      // entry and, on a live run, applies to the running region — the
      // `extend_cycle` of the Pipeline Manager (ADR-0007 / ADR-0011 / #150). The
      // daemon enforces the only guard (no drop below the current lap) on save.
      tab.pipeline.loops = (tab.pipeline.loops ?? []).map((r) =>
        r.id === regionId ? { ...r, ...updates } : r,
      );
    }, { coalesceKey: `updateRegion:${regionId}:${Object.keys(updates).sort().join(",")}` }));
  },

  createCollectionRegion: (members: string[], over: string) => {
    set((s) => {
      // Guard BEFORE mutating so a rejected gesture stays a true no-op: no
      // dirty flag, no history entry. A node lives in at most one region —
      // the context menu applies the same guard; this is the authoritative
      // backstop.
      const tab = s.openTabs.find((t) => t.id === s.activeTabId);
      if (!tab || members.length === 0) return {};
      const taken = new Set(
        (tab.pipeline.loops ?? []).flatMap((r) => r.members),
      );
      if (members.some((m) => taken.has(m))) return {};
      return mutateActiveTabWithHistory(s, (t) => {
        t.pipeline.loops = [
          ...(t.pipeline.loops ?? []),
          { id: generatedRegionId(members), kind: "collection", members, over },
        ];
      });
    });
  },

  addNote: (note: NoteDef) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      tab.pipeline.notes = [...(tab.pipeline.notes ?? []), note];
    }));
  },

  updateNote: (noteId: string, updates: Partial<NoteDef>) => {
    // Tracked (unlike `updatePrompt`): a note's content is edit data that must
    // ride the undo stack. Coalesced per-note so a burst of keystrokes folds
    // into one undo step (mirror of `updateRegion`).
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      tab.pipeline.notes = (tab.pipeline.notes ?? []).map((n) =>
        n.id === noteId ? { ...n, ...updates } : n,
      );
    }, { coalesceKey: `updateNote:${noteId}:${Object.keys(updates).sort().join(",")}` }));
  },

  moveNote: (noteId: string, x: number, y: number) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      tab.pipeline.notes = (tab.pipeline.notes ?? []).map((n) =>
        n.id === noteId ? { ...n, view: { x: Math.round(x), y: Math.round(y) } } : n,
      );
    }));
  },

  deleteNote: (noteId: string) => {
    set((s) => ({
      ...mutateActiveTabWithHistory(s, (tab) => {
        tab.pipeline.notes = (tab.pipeline.notes ?? []).filter((n) => n.id !== noteId);
      }),
      // Clear selection unconditionally (mirror `deleteNode`/`deleteEdge`): the
      // deleted note's inspector must close.
      selection: { kind: "none" as const, id: null },
    }));
  },

  updatePipelineMeta: (updates) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      if (updates.name !== undefined) tab.pipeline.name = updates.name;
      if (updates.version !== undefined) tab.pipeline.version = updates.version;
      if (updates.variables !== undefined) tab.pipeline.variables = updates.variables;
      if (updates.prompt_required !== undefined) tab.pipeline.prompt_required = updates.prompt_required;
      // ADR-0086: off is the absence of the key, so an untouched pipeline
      // round-trips byte for byte.
      if (updates.wait_for_started_nodes !== undefined) {
        if (updates.wait_for_started_nodes) tab.pipeline.wait_for_started_nodes = true;
        else delete tab.pipeline.wait_for_started_nodes;
      }
      if (updates.grid_size !== undefined) {
        if (updates.grid_size) tab.pipeline.grid_size = updates.grid_size;
        else delete tab.pipeline.grid_size;
      }
    }, { coalesceKey: `updatePipelineMeta:${Object.keys(updates).sort().join(",")}` }));
  },

  updatePrompt: (nodeId: string, content: string) => {
    // Intentionally NOT tracked (ADR-0014): run snapshots exclude prompts, and
    // snapshotting them would *lose* later prompt edits on undo. Prompts are
    // serialized wholesale on save independent of structural history.
    set((s) => mutateActiveTab(s, (tab) => {
      tab.prompts = { ...tab.prompts, [nodeId]: content };
    }));
  },

  };
}
