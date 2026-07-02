import { create } from "zustand";
import type {
  PipelineListEntry,
  PipelineScope,
  PipelineDef,
  NodeDef,
  EdgeDef,
} from "../types";
import type { LibraryPipelineScope } from "../api";
import type { LoopRegion } from "../types";
import {
  fetchPipeline,
  fetchPipelines,
  savePipeline,
  fetchRunPipeline,
  saveRunPipeline,
  deletePipeline as apiDeletePipeline,
  saveLibraryPipeline,
} from "../api";
import { generateNodeId } from "../lib/nanoid";
import {
  materializeMissingRegions,
  reconcileLoopRegions,
  regionsDestroyedByEdgeRemoval,
} from "../lib/loopRegions";

export type SelectionKind = "node" | "edge" | "region" | "none";

export interface Selection {
  kind: SelectionKind;
  id: string | null;
  /**
   * Index into `pipeline.edges` when `kind === "edge"`. Edges have no stable id,
   * so the index is the selection key — the same key the canvas uses (`e-{i}`)
   * and `updateEdge`/`deleteEdge` take. Undefined for node/none selections.
   */
  edgeIndex?: number;
  /**
   * The selected loop region's `id` when `kind === "region"` (ADR-0011 / #150).
   * Regions carry a stable `id`, so the id is the selection key the region
   * inspector and `updateRegion` use. Undefined for other selections.
   */
  regionId?: string;
}

export interface ConflictData {
  pipeline: PipelineDef;
  prompts: Record<string, string>;
  diagnostics: string[];
}

export interface SaveErrorData {
  message: string;
  line?: number;
}

export interface OpenPipeline {
  id: string;
  scope: string;
  pipeline: PipelineDef;
  prompts: Record<string, string>;
  diagnostics: string[];
  dirty: boolean;
  externalDirty: boolean;
  runId?: string;
  conflict?: ConflictData;
  saveError?: SaveErrorData;
  // Stable id of the library entry this tab is mirroring. Locked once known
  // (either by name-matching at open time or by the response of `Save to
  // library`). Renaming the pipeline does NOT clear this — it's the whole
  // point of tracking it: a renamed-then-saved pipeline stays the same library
  // entry on disk.
  libraryId?: string | null;
  libraryScope?: LibraryPipelineScope | null;
}

/**
 * Per-tab undo/redo history (ADR-0014 / #226). Entries are whole `PipelineDef`
 * object references captured *before* a structural mutation — NOT deep clones.
 * This is safe only because every store mutation is copy-on-write (it rebuilds
 * whole arrays, never mutating a node/edge/port in place), so a captured
 * reference stays frozen as long as nobody writes through it. The history is
 * in-memory only (no cross-reload persistence) and excludes run state (it lives
 * in a separate overlay) and prompt text (intentionally not tracked).
 */
export interface TabHistory {
  /** Pre-mutation snapshots, oldest→newest. The top is the most recent restore point. */
  past: PipelineDef[];
  /** States undone, available for redo (newest at the end). */
  future: PipelineDef[];
  /** Coalescing key of the most recent push (null = never coalesce). */
  lastKey: string | null;
  /** `Date.now()` of the most recent push, for the time-window coalescer. */
  lastAt: number;
}

interface EditState {
  pipelines: PipelineListEntry[];
  openTabs: OpenPipeline[];
  activeTabId: string | null;
  selection: Selection;
  scrollToPort: string | null;
  lastSavedAt: Record<string, number>;
  // Undo/redo history keyed by tabId (ADR-0014 / #226). Lazily initialized on
  // the first tracked mutation; `canUndo`/`canRedo` are derived by components
  // with a selector, never stored.
  history: Record<string, TabHistory>;

  loadPipelines: () => Promise<void>;
  openPipeline: (id: string, scope?: PipelineScope) => Promise<void>;
  openRunPipeline: (runId: string) => Promise<void>;
  closeRunPipeline: (runId: string) => void;
  closeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
  setSelection: (sel: Selection) => void;
  setScrollToPort: (port: string | null) => void;

  // Node mutations
  addNode: (node: NodeDef) => void;
  updateNode: (nodeId: string, updates: Partial<NodeDef>) => void;
  // Batched position write for a group drag (#232): xyflow's onNodeDragStop
  // hands us every dragged node at once; persist all their `view` coords in one
  // store mutation (one re-derivation, one dirty/save unit).
  updateNodeViews: (updates: { id: string; x: number; y: number }[]) => void;
  deleteNode: (nodeId: string) => void;
  duplicateNode: (nodeId: string) => void;

  // Edge mutations
  addEdge: (edge: EdgeDef) => void;
  // `opts.track === false` mutates without pushing a history entry — used by the
  // draw-edge arrival-side stamp (#168) so a single edge-draw gesture folds into
  // ONE undo step instead of two (the `addEdge` push + a separate stamp push).
  updateEdge: (index: number, updates: Partial<EdgeDef>, opts?: { track?: boolean }) => void;
  deleteEdge: (index: number) => void;

  // Region mutations (ADR-0011 / #150) — edit a bounded region's bound live.
  updateRegion: (regionId: string, updates: Partial<LoopRegion>) => void;

  // Pipeline-level mutations
  updatePipelineMeta: (updates: Partial<Pick<PipelineDef, "name" | "version" | "variables" | "prompt_required">>) => void;

  // Prompt mutations
  updatePrompt: (nodeId: string, content: string) => void;

  // Undo/redo (ADR-0014 / #226) — operate on the active tab's history.
  undo: () => void;
  redo: () => void;

  // Pipeline deletion
  removePipeline: (id: string, scope?: PipelineScope) => Promise<void>;

  // Persistence
  save: (id: string) => Promise<void>;
  flushPendingSaves: () => Promise<void>;
  clearSaveError: (id: string) => void;

  // Hot-reload
  reloadPipeline: (id: string) => Promise<void>;
  resolveConflict: (id: string, resolution: "keep" | "take") => void;

  // Library pipeline sync — overwrite this tab's pipeline with the library
  // YAML and re-fetch the parsed form. Used by the "Reload changes" action
  // when a run's snapshot has diverged from its library template.
  reloadFromLibrary: (tabId: string, libraryYaml: string) => Promise<void>;

  // Library binding — record that a tab corresponds to a library entry. Locking
  // happens once (the first time a name match is found, OR right after a star
  // click that creates the entry). After that, renames on the canvas no longer
  // detach the star.
  setLibraryBinding: (
    tabId: string,
    libraryId: string | null,
    libraryScope: LibraryPipelineScope | null,
  ) => void;
}

// Canonical plain-object form of a pipeline — the exact structure that gets
// YAML-serialized on save. Also used for semantic comparison against library
// entries (see useLibraryPipelines): building both sides through this single
// code path erases formatting noise (key order, quoting, parser defaults)
// that a textual YAML comparison would misread as divergence.
export function pipelineToYamlObject(p: PipelineDef): Record<string, unknown> {
  const obj: Record<string, unknown> = {
    name: p.name,
  };
  if (p.version) obj.version = p.version;
  // Prompt-optional pipelines (#158) carry an explicit `prompt_required: false`.
  // The default (prompt required) is omitted so the common case stays clean and
  // round-trips by absence — same convention as `loops` and `version`.
  if (p.prompt_required === false) obj.prompt_required = false;
  if (Object.keys(p.variables).length > 0) {
    const vars: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(p.variables)) {
      vars[k] = v.default;
    }
    obj.variables = vars;
  }
  obj.nodes = p.nodes.map((n) => {
    const node: Record<string, unknown> = {
      id: n.id,
      name: n.name ?? n.id,
      type: n.type,
    };
    if (n.interactive) node.interactive = true;
    // Per-node model override (#296): semantic (compared in the pipeline diff),
    // emitted only when set so an unset node and a library twin with no model
    // both produce objects without the key and stay `synced`, not `diverged`.
    if (n.model) node.model = n.model;
    // A collection's `over` driver now lives on the `loops:` region, not on any
    // node (#151) — no node-level `over` serialization.
    if (n.inputs.length > 0)
      node.inputs = n.inputs.map((port) => {
        const p: Record<string, unknown> = { name: port.name };
        if (port.repeated) p.repeated = true;
        if (port.side) p.side = port.side;
        if (port.port_type && port.port_type !== "markdown")
          p.port_type = port.port_type;
        if (port.frontmatter) p.frontmatter = port.frontmatter;
        return p;
      });
    if (n.outputs.length > 0)
      node.outputs = n.outputs.map((port) => {
        const p: Record<string, unknown> = { name: port.name };
        if (port.repeated) p.repeated = true;
        if (port.side) p.side = port.side;
        if (port.port_type && port.port_type !== "markdown")
          p.port_type = port.port_type;
        if (port.frontmatter) p.frontmatter = port.frontmatter;
        if (port.when) p.when = port.when;
        return p;
      });
    if (n.view) node.view = n.view;
    return node;
  });
  obj.edges = p.edges.map((e) => {
    const edge: Record<string, unknown> = {
      source: e.source,
      target: e.target,
    };
    // Conditional routing (ADR-0011): a guarded edge carries `when:`, a
    // fallback edge carries `else: true`. Both live on the edge now, not on a
    // Switch node's output ports.
    if (e.when && Object.keys(e.when).length > 0) edge.when = e.when;
    if (e.else === true) edge.else = true;
    // Routing (#154): only manually-pinned edges persist their route. Auto
    // edges recompute deterministically, so they store no `mode`/`waypoints` —
    // emitting them would be noise. A `manual` mode without waypoints is also
    // meaningless (nothing pinned), so guard on a non-empty waypoint list.
    if (e.mode === "manual" && e.waypoints && e.waypoints.length > 0) {
      edge.mode = "manual";
      edge.waypoints = e.waypoints.map((w) => ({ x: w.x, y: w.y }));
    }
    // Drop-position anchor side (#168). Layout, like mode/waypoints: persists so
    // a shared workflow keeps its arrow arrival sides. `left` is the legacy
    // default and round-trips by absence, so emit only the other three sides.
    if (e.target_side && e.target_side !== "left") {
      edge.target_side = e.target_side;
    }
    return edge;
  });

  // Named bounded loop regions (ADR-0011 / #148). Emitted only when present so
  // loop-less pipelines stay clean and round-trip identically.
  if (p.loops && p.loops.length > 0) {
    obj.loops = p.loops.map((r) => {
      const region: Record<string, unknown> = {
        id: r.id,
        kind: r.kind,
        members: r.members,
      };
      if (r.max_iter !== undefined && r.max_iter !== null)
        region.max_iter = r.max_iter;
      return region;
    });
  }

  return obj;
}

export function serializePipeline(p: PipelineDef): string {
  return yamlStringify(pipelineToYamlObject(p));
}

function yamlStringify(obj: unknown): string {
  return dumpYaml(obj, 0);
}

function dumpYaml(val: unknown, indent: number): string {
  const prefix = "  ".repeat(indent);
  if (val === null || val === undefined) return "null";
  if (typeof val === "boolean") return val ? "true" : "false";
  if (typeof val === "number") return String(val);
  if (typeof val === "string") {
    if (val.includes("\n") || val.includes(":") || val.includes("#") || val.includes('"') || val === "") {
      return JSON.stringify(val);
    }
    if (/^\d/.test(val) || val === "true" || val === "false" || val === "null") {
      return JSON.stringify(val);
    }
    return val;
  }
  if (Array.isArray(val)) {
    if (val.length === 0) return "[]";
    const isSimple = val.every(
      (v) => typeof v === "string" || typeof v === "number" || typeof v === "boolean",
    );
    if (isSimple) {
      return `[${val.map((v) => dumpYaml(v, 0)).join(", ")}]`;
    }
    return val
      .map((v) => {
        const child = dumpYaml(v, indent + 1);
        if (typeof v === "object" && v !== null && !Array.isArray(v)) {
          const lines = child.split("\n");
          // The recursive call already indented continuation lines at indent+1,
          // which lines up with the column where the first key lands after `- `
          // — so pass them through verbatim.
          const rest = lines.slice(1).join("\n");
          return rest
            ? `${prefix}- ${lines[0]}\n${rest}`
            : `${prefix}- ${lines[0]}`;
        }
        return `${prefix}- ${child}`;
      })
      .join("\n");
  }
  if (typeof val === "object") {
    const entries = Object.entries(val as Record<string, unknown>);
    if (entries.length === 0) return "{}";
    const isFlowable = entries.every(
      ([, v]) => typeof v !== "object" || v === null,
    );
    if (isFlowable && entries.length <= 3) {
      const inner = entries.map(([k, v]) => `${k}: ${dumpYaml(v, 0)}`).join(", ");
      return `{ ${inner} }`;
    }
    return entries
      .map(([k, v]) => {
        const child = dumpYaml(v, indent + 1);
        if (typeof v === "object" && v !== null && !Array.isArray(v)) {
          const lines = child.split("\n");
          if (lines.length === 1 && lines[0].startsWith("{")) {
            return `${k}: ${lines[0]}`;
          }
          const [first, ...rest] = lines;
          const head = `${k}:\n  ${prefix}${first}`;
          return rest.length > 0 ? `${head}\n${rest.join("\n")}` : head;
        }
        if (Array.isArray(v) && v.length > 0 && !v.every((x) => typeof x !== "object" || x === null)) {
          return `${k}:\n${child}`;
        }
        return `${k}: ${child}`;
      })
      .join("\n" + prefix);
  }
  return String(val);
}

function mutateActiveTab(
  state: EditState,
  fn: (tab: OpenPipeline) => void,
): Partial<EditState> {
  const idx = state.openTabs.findIndex((t) => t.id === state.activeTabId);
  if (idx < 0) return {};
  const tabs = [...state.openTabs];
  const tab = { ...tabs[idx], pipeline: { ...tabs[idx].pipeline }, dirty: true };
  fn(tab);
  tabs[idx] = tab;
  return { openTabs: tabs };
}

// Undo/redo history tuning (ADR-0014 / #226).
const HISTORY_CAP = 50; // FIFO-capped per tab — bounds memory; oldest dropped first.
const COALESCE_WINDOW_MS = 500; // same-key edits within this window = one undo step.

function emptyHistory(): TabHistory {
  return { past: [], future: [], lastKey: null, lastAt: 0 };
}

// Returns the new `history` map after recording `before` for `tabId`. Coalescing:
// a non-null key that matches the previous push within COALESCE_WINDOW_MS keeps
// the existing top `before` (the correct restore point for the whole run) and
// only clears redo — so a typed run / waypoint drag collapses to one undo step.
function recordHistory(
  history: Record<string, TabHistory>,
  tabId: string,
  before: PipelineDef,
  coalesceKey: string | null,
): Record<string, TabHistory> {
  const h = history[tabId] ?? emptyHistory();
  const now = Date.now();
  if (
    coalesceKey != null &&
    coalesceKey === h.lastKey &&
    now - h.lastAt < COALESCE_WINDOW_MS &&
    h.past.length > 0
  ) {
    return { ...history, [tabId]: { ...h, future: [], lastAt: now } };
  }
  const past = [...h.past, before];
  if (past.length > HISTORY_CAP) past.shift(); // drop oldest (FIFO)
  return { ...history, [tabId]: { past, future: [], lastKey: coalesceKey, lastAt: now } };
}

// History-aware sibling to `mutateActiveTab`. `opts.track === false` mutates
// without recording (the draw-edge stamp folds into the preceding `addEdge`).
function mutateActiveTabWithHistory(
  state: EditState,
  fn: (tab: OpenPipeline) => void,
  opts: { coalesceKey?: string | null; track?: boolean } = {},
): Partial<EditState> {
  const idx = state.openTabs.findIndex((t) => t.id === state.activeTabId);
  if (idx < 0) return {};
  const before = state.openTabs[idx].pipeline; // immutable per the COW invariant
  const tabId = state.openTabs[idx].id;
  const mutated = mutateActiveTab(state, fn);
  if (opts.track === false) return mutated;
  const history = recordHistory(state.history, tabId, before, opts.coalesceKey ?? null);
  return { ...mutated, history };
}

// CLEAR: the tab survives but its `pipeline` was replaced by foreign content
// (hot-reload, "Take theirs", "Reload changes") — past/future are stale, drop
// them but keep the (now-empty) slot.
function clearedHistory(
  history: Record<string, TabHistory>,
  tabId: string,
): Record<string, TabHistory> {
  return { ...history, [tabId]: emptyHistory() };
}

// DROP: the tab is gone (close/remove/self-close) — remove its slot entirely so
// a stale entry can't leak memory or be silently reattached if the id is reused.
function droppedHistory(
  history: Record<string, TabHistory>,
  tabId: string,
): Record<string, TabHistory> {
  if (!(tabId in history)) return history;
  const next = { ...history };
  delete next[tabId];
  return next;
}

function edgeReferencesNode(edge: EdgeDef, nodeId: string): boolean {
  if (edge.source.node === nodeId) return true;
  return "node" in edge.target && (edge.target as { node: string }).node === nodeId;
}

function propagatePortChangesToEdges(
  tab: OpenPipeline,
  nodeId: string,
  oldPorts: { name: string }[],
  newPorts: { name: string }[],
  side: "inputs" | "outputs",
): void {
  const edgeSide = side === "inputs" ? "target" : "source";
  const newPortNames = new Set(newPorts.map((p) => p.name));

  const renameMap = new Map<string, string>();
  if (oldPorts.length === newPorts.length) {
    for (let i = 0; i < oldPorts.length; i++) {
      if (oldPorts[i].name !== newPorts[i].name && !newPortNames.has(oldPorts[i].name)) {
        renameMap.set(oldPorts[i].name, newPorts[i].name);
      }
    }
  }

  const kept: EdgeDef[] = [];
  for (const edge of tab.pipeline.edges) {
    if (edge[edgeSide].node !== nodeId) {
      kept.push(edge);
      continue;
    }
    const renamed = renameMap.get(edge[edgeSide].port);
    if (renamed) {
      kept.push({ ...edge, [edgeSide]: { ...edge[edgeSide], port: renamed } });
    } else if (newPortNames.has(edge[edgeSide].port)) {
      kept.push(edge);
    }
    // else: the port the edge referenced is gone — drop the edge (no node-side
    // effect since ForEach `over` clearing was retired with the node type, #151).
  }
  tab.pipeline.edges = kept;
}

export const useEditStore = create<EditState>((set, get) => ({
  pipelines: [],
  openTabs: [],
  activeTabId: null,
  selection: { kind: "none", id: null },
  scrollToPort: null,
  lastSavedAt: {},
  history: {},

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
      set((s) => ({
        openTabs: [...s.openTabs, tab],
        activeTabId: id,
        selection: { kind: "none", id: null },
      }));
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
      set((s) => ({
        openTabs: [...s.openTabs, tab],
        activeTabId: tabId,
        selection: { kind: "none", id: null },
      }));
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

  setActiveTab: (id: string) => {
    set({ activeTabId: id, selection: { kind: "none", id: null } });
  },

  setSelection: (sel: Selection) => {
    set({ selection: sel });
  },

  setScrollToPort: (port: string | null) => {
    set({ scrollToPort: port });
  },

  addNode: (node: NodeDef) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      tab.pipeline.nodes = [...tab.pipeline.nodes, node];
    }));
  },

  updateNode: (nodeId: string, updates: Partial<NodeDef>) => {
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
    set((s) => ({
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
    }));
  },

  duplicateNode: (nodeId: string) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      const src = tab.pipeline.nodes.find((n) => n.id === nodeId);
      if (!src) return;
      const newId = generateNodeId();
      const srcName = src.name ?? src.id;
      const copy: NodeDef = {
        ...src,
        id: newId,
        name: `${srcName} copy`,
        inputs: src.inputs.map((p) => ({ ...p })),
        outputs: src.outputs.map((p) => ({ ...p })),
        view: src.view ? { x: src.view.x + 40, y: src.view.y + 40 } : { x: 200, y: 200 },
      };
      tab.pipeline.nodes = [...tab.pipeline.nodes, copy];
    }));
  },

  addEdge: (edge: EdgeDef) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      tab.pipeline.edges = [...tab.pipeline.edges, edge];
      // Auto-materialize a bounded loop region when this edge closes a cycle
      // (ADR-0011 / #166): a drawn cycle is never accidentally unbounded. Only
      // cycles not already covered by an existing `loops:` entry add a region;
      // acyclic edges add nothing.
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

  deleteEdge: (index: number) => {
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
      selection: { kind: "none" as const, id: null },
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

  updatePipelineMeta: (updates) => {
    set((s) => mutateActiveTabWithHistory(s, (tab) => {
      if (updates.name !== undefined) tab.pipeline.name = updates.name;
      if (updates.version !== undefined) tab.pipeline.version = updates.version;
      if (updates.variables !== undefined) tab.pipeline.variables = updates.variables;
      if (updates.prompt_required !== undefined) tab.pipeline.prompt_required = updates.prompt_required;
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

  undo: () => set((s) => {
    const tabId = s.activeTabId;
    if (!tabId) return {};
    const h = s.history[tabId];
    if (!h || h.past.length === 0) return {};
    const idx = s.openTabs.findIndex((t) => t.id === tabId);
    if (idx < 0) return {};
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
    const tabId = s.activeTabId;
    if (!tabId) return {};
    const h = s.history[tabId];
    if (!h || h.future.length === 0) return {};
    const idx = s.openTabs.findIndex((t) => t.id === tabId);
    if (idx < 0) return {};
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

  save: async (id: string) => {
    const tab = get().openTabs.find((t) => t.id === id);
    if (!tab) return;
    try {
      const yaml = serializePipeline(tab.pipeline);
      if (tab.runId) {
        await saveRunPipeline(tab.runId, yaml, tab.prompts);
      } else {
        // Save back into the same store the tab was opened from, so a
        // `library`-scoped edit never overwrites a same-named repo file (#216).
        await savePipeline(id, yaml, tab.prompts, tab.scope);
      }
      // Mirror the save into the library entry when this tab is starred, so
      // renames-then-Save propagate to the library file (without orphaning the
      // previous entry under the old slug — `libraryId` keeps the file stable).
      if (tab.libraryId) {
        try {
          await saveLibraryPipeline(
            tab.pipeline.name,
            yaml,
            tab.prompts,
            {
              id: tab.libraryId,
              ...(tab.libraryScope ? { scope: tab.libraryScope } : {}),
            },
          );
        } catch {
          // Non-fatal: the primary pipeline write succeeded. The library entry
          // will diverge until the next manual sync, but the user's primary
          // edit is safe on disk.
        }
      }
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id ? { ...t, dirty: false, saveError: undefined } : t,
        ),
        lastSavedAt: { ...s.lastSavedAt, [id]: Date.now() },
      }));
    } catch (err: unknown) {
      const raw = err as Record<string, unknown> | null;
      const status = typeof raw?.status === "number" ? raw.status : undefined;
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
        return;
      }
      const message =
        typeof raw?.message === "string" ? raw.message : "Save failed";
      const line =
        typeof raw?.line === "number" ? raw.line : undefined;
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id ? { ...t, saveError: { message, line } } : t,
        ),
      }));
    }
  },

  flushPendingSaves: async () => {
    const dirtyTabs = get().openTabs.filter((t) => t.dirty);
    await Promise.all(dirtyTabs.map((t) => get().save(t.id)));
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
      const detail = await fetchPipeline(id, tab?.scope);
      if (tab?.dirty) {
        set((s) => ({
          openTabs: s.openTabs.map((t) =>
            t.id === id
              ? {
                  ...t,
                  conflict: {
                    pipeline: detail.pipeline,
                    prompts: detail.prompts,
                    diagnostics: detail.diagnostics ?? [],
                  },
                }
              : t,
          ),
        }));
        return;
      }
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

  resolveConflict: (id: string, resolution: "keep" | "take") => {
    set((s) => {
      const tab = s.openTabs.find((t) => t.id === id);
      const hadConflict = tab?.conflict != null;
      return {
        openTabs: s.openTabs.map((t) => {
          if (t.id !== id || !t.conflict) return t;
          if (resolution === "keep") {
            return { ...t, conflict: undefined };
          }
          return {
            ...t,
            pipeline: t.conflict.pipeline,
            prompts: t.conflict.prompts,
            diagnostics: t.conflict.diagnostics,
            dirty: false,
            conflict: undefined,
          };
        }),
        // "Take theirs" replaces the pipeline → CLEAR. "Keep mine" leaves the
        // local edits (and thus the undo stack) intact → KEEP (no change).
        history:
          resolution === "take" && hadConflict
            ? clearedHistory(s.history, id)
            : s.history,
      };
    });
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
        await savePipeline(tabId, libraryYaml, tab.prompts, tab.scope);
        const detail = await fetchPipeline(tabId, tab.scope);
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
          // CLEAR — "Reload changes" overwrote this tab with the library YAML.
          history: clearedHistory(s.history, tabId),
        }));
      }
    } catch (err: unknown) {
      const raw = err as Record<string, unknown> | null;
      const message =
        typeof raw?.message === "string" ? raw.message : "Reload failed";
      const line = typeof raw?.line === "number" ? raw.line : undefined;
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === tabId ? { ...t, saveError: { message, line } } : t,
        ),
      }));
    }
  },
}));
