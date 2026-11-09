/**
 * The pure helpers of the editor store: mutating the active tab, its undo
 * history (ADR-0014), rekeying a tab, keeping edges in step with ports.
 */

import type { EditorPipelineDef } from "../model/pipeline";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import { carriedPorts, withCarriedPorts } from "../../lib/edgePorts";
import { requalifyWhen, renameWhenPorts } from "../../lib/whenClause";
import { isEmergentInputNode } from "../../lib/anchorSide";
import { hasUnsavedWork, isTabLocked } from "./editState";
import { noteDraftGesture } from "./draftGesture";
import type { EditState, OpenPipeline, Selection, TabHistory } from "./editState";

export function mutateActiveTab(
  state: EditState,
  fn: (tab: OpenPipeline) => void,
): Partial<EditState> {
  const idx = state.openTabs.findIndex((t) => t.id === state.activeTabId);
  if (idx < 0) return {};
  // ADR-0085: a run tab in « pilotage » takes no definition change at all;
  // ADR-0088: nor does a pipeline tab reading its shared draft.
  if (isTabLocked(state.openTabs[idx])) return {};
  const tabs = [...state.openTabs];
  const tab = { ...tabs[idx], pipeline: { ...tabs[idx].pipeline }, dirty: true };
  fn(tab);
  tabs[idx] = tab;
  return { openTabs: tabs };
}

// Undo/redo history tuning (ADR-0014 / #226).
export const HISTORY_CAP = 50; // FIFO-capped per tab — bounds memory; oldest dropped first.
export const COALESCE_WINDOW_MS = 500; // same-key edits within this window = one undo step.

export function emptyHistory(): TabHistory {
  return { past: [], future: [], lastKey: null, lastAt: 0 };
}

// Returns the new `history` map after recording `before` for `tabId`. Coalescing:
// a non-null key that matches the previous push within COALESCE_WINDOW_MS keeps
// the existing top `before` (the correct restore point for the whole run) and
// only clears redo — so a typed run / waypoint drag collapses to one undo step.
export function recordHistory(
  history: Record<string, TabHistory>,
  tabId: string,
  before: EditorPipelineDef,
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
export function mutateActiveTabWithHistory(
  state: EditState,
  fn: (tab: OpenPipeline) => void,
  opts: { coalesceKey?: string | null; track?: boolean } = {},
): Partial<EditState> {
  const idx = state.openTabs.findIndex((t) => t.id === state.activeTabId);
  if (idx < 0 || isTabLocked(state.openTabs[idx])) return {};
  // ADR-0088: a shared draft keeps its own history, of my changes only; it
  // learns how this gesture folds into it.
  if (state.openTabs[idx].draft) {
    noteDraftGesture(state.openTabs[idx].id, opts.coalesceKey ?? null, opts.track !== false);
    return mutateActiveTab(state, fn);
  }
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
export function clearedHistory(
  history: Record<string, TabHistory>,
  tabId: string,
): Record<string, TabHistory> {
  return { ...history, [tabId]: emptyHistory() };
}

// DROP: the tab is gone (close/remove/self-close) — remove its slot entirely so
// a stale entry can't leak memory or be silently reattached if the id is reused.
export function droppedHistory(
  history: Record<string, TabHistory>,
  tabId: string,
): Record<string, TabHistory> {
  if (!(tabId in history)) return history;
  const next = { ...history };
  delete next[tabId];
  return next;
}

// REKEY (#774): the pipeline a tab was opened under changed identity (its file
// stem moved with a visible-name rename). The tab survives — content, dirty
// state and undo stack are the user's work, none of it belongs to the old
// stem — but every id-keyed slot (openTabs, activeTabId, lastSavedAt, history,
// tabSelections) must follow to the new id, or the next save/watcher event
// would operate on a tab the store can no longer find.
export function rekeyTab(
  state: EditState,
  oldId: string,
  newId: string,
  extra?: Partial<OpenPipeline>,
): Partial<EditState> {
  const tab = state.openTabs.find((t) => t.id === oldId);
  if (!tab) return {};
  const openTabs = state.openTabs.map((t) => (t.id === oldId ? { ...t, id: newId, ...extra } : t));
  const activeTabId = state.activeTabId === oldId ? newId : state.activeTabId;
  const lastSavedAt = { ...state.lastSavedAt };
  if (lastSavedAt[oldId] !== undefined) {
    lastSavedAt[newId] = lastSavedAt[oldId];
    delete lastSavedAt[oldId];
  }
  const history = { ...state.history };
  if (history[oldId]) {
    history[newId] = history[oldId];
    delete history[oldId];
  }
  const tabSelections = { ...state.tabSelections };
  if (tabSelections[oldId]) {
    tabSelections[newId] = tabSelections[oldId];
    delete tabSelections[oldId];
  }
  return { openTabs, activeTabId, lastSavedAt, history, tabSelections, lastRekey: { from: oldId, to: newId } };
}

/**
 * Bring the open tab `id` to the front: the tab left keeps its selection, and
 * `id` gets back the one it had, unless it no longer holds (a node gone, an
 * edge index past the end).
 */
export function switchedTab(state: EditState, id: string): Partial<EditState> {
  const tabSelections = leftSelection(state);
  if (id === state.activeTabId) return { selection: state.selection };
  const tab = state.openTabs.find((t) => t.id === id);
  const kept = tabSelections[id];
  return {
    activeTabId: id,
    tabSelections,
    selection: tab && kept && stillHolds(tab, kept) ? kept : { kind: "none", id: null },
  };
}

/**
 * The closed tabs forget their selection; when the active one closed, the tab
 * `activeTabId` that takes its place gets back its own.
 */
export function afterClose(
  state: EditState,
  closed: string[],
  openTabs: OpenPipeline[],
  activeTabId: string | null,
): Partial<EditState> {
  const tabSelections = { ...state.tabSelections };
  for (const id of closed) delete tabSelections[id];
  if (activeTabId == null || activeTabId === state.activeTabId) return { tabSelections };
  const tab = openTabs.find((t) => t.id === activeTabId);
  const kept = tabSelections[activeTabId];
  return { tabSelections, ...(tab && kept && stillHolds(tab, kept) ? { selection: kept } : {}) };
}

/** The selections by tab, the active tab's as it is now. */
function leftSelection(state: EditState): Record<string, Selection> {
  const kept = { ...state.tabSelections };
  if (state.activeTabId != null) kept[state.activeTabId] = state.selection;
  return kept;
}

function stillHolds(tab: OpenPipeline, selection: Selection): boolean {
  switch (selection.kind) {
    case "node":
      return tab.pipeline.nodes.some((n) => n.id === selection.id);
    case "edge":
      return selection.edgeIndex != null && selection.edgeIndex < tab.pipeline.edges.length;
    case "run":
      return tab.runId != null;
    case "region":
      return (tab.pipeline.loops ?? []).some((r) => r.id === selection.regionId);
    case "note":
      return (tab.pipeline.notes ?? []).some((n) => n.id === selection.noteId);
    case "none":
      return true;
  }
}

// Single-tab replace (#342): `tab` becomes the sole open tab, every `victims`
// tab is discarded (its undo stack dropped per id — a reused `__run__<runId>`
// must not inherit a stale stack, ADR-0014). One `set()` patch, no loop.
export function replaceWithTab(
  state: EditState,
  tab: OpenPipeline,
  victims: OpenPipeline[],
): Partial<EditState> {
  const history = victims.reduce((h, v) => droppedHistory(h, v.id), state.history);
  return {
    openTabs: [tab],
    activeTabId: tab.id,
    selection: { kind: "none", id: null },
    tabSelections: {},
    history,
    pendingSingleTab: null,
  };
}

// Compute the store patch for placing a freshly-opened tab (#342). Multi-tab:
// append + activate. Single-tab: replace the current tab — but if the replace
// would discard unsaved work, PARK it for confirmation instead of destroying it
// (the caller has already fetched, so the tab object is ready to apply on
// confirm). A tab whose id is already open just gets activated: the open's
// early-return misses a second open that started before the first one's fetch
// answered (a double-click on a library row), and the tab already there keeps
// its draft and history.
export function placeOpenedTab(state: EditState, tab: OpenPipeline): Partial<EditState> {
  if (state.openTabs.some((t) => t.id === tab.id)) return switchedTab(state, tab.id);
  if (!state.singleTabMode) {
    // A tab id opened afresh (a Run reopened) starts with nothing selected.
    const tabSelections = { ...leftSelection(state) };
    delete tabSelections[tab.id];
    return {
      openTabs: [...state.openTabs, tab],
      activeTabId: tab.id,
      selection: { kind: "none", id: null },
      tabSelections,
    };
  }
  const victims = state.openTabs.filter((t) => t.id !== tab.id);
  if (victims.some(hasUnsavedWork)) {
    return { pendingSingleTab: { tab, victims } };
  }
  return replaceWithTab(state, tab, victims);
}

export function edgeReferencesNode(edge: EdgeDef, nodeId: string): boolean {
  if (edge.source.node === nodeId) return true;
  return "node" in edge.target && (edge.target as { node: string }).node === nodeId;
}

export function propagatePortChangesToEdges(
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
    if (edgeSide === "target") {
      const renamed = renameMap.get(edge.target.port);
      if (renamed) {
        kept.push({ ...edge, target: { ...edge.target, port: renamed } });
      } else if (newPortNames.has(edge.target.port)) {
        kept.push(edge);
      }
      // else: the port the edge referenced is gone — drop the edge (no node-side
      // effect since ForEach `over` clearing was retired with the node type, #151).
      continue;
    }
    // Source side: the edge may carry SEVERAL outputs (ADR-0073 / #843). Rename
    // each one, drop the ones that disappeared, and drop the edge only when it
    // is left carrying nothing — losing one of two ports must not take the
    // arrow with it. A dropped port's condition rows are re-pointed at a port
    // still carried, exactly as unticking one in the panel does.
    const ports = carriedPorts(edge.source);
    // Rename first (same cardinality, so the clause's qualifiers follow their
    // port), then drop what disappeared (which re-points the orphaned rows).
    const renamed = ports.map((p) => renameMap.get(p) ?? p);
    const next = renamed.filter((p) => newPortNames.has(p));
    if (next.length === 0) continue;
    // An emergent input is named after the output it carries (#149): renaming
    // the output renames that input downstream (CORE-88). A declared input
    // (End's `result`, a switch's `in`) keeps its own name.
    const targetType = tab.pipeline.nodes.find((n) => n.id === edge.target.node)?.type;
    const inputRename = ports.includes(edge.target.port) ? renameMap.get(edge.target.port) : undefined;
    const target =
      inputRename && targetType && isEmergentInputNode(targetType)
        ? { ...edge.target, port: inputRename }
        : edge.target;
    kept.push({
      ...edge,
      target,
      source: withCarriedPorts(edge.source, next),
      when: requalifyWhen(renameWhenPorts(edge.when, ports, renameMap), renamed, next),
    });
  }
  tab.pipeline.edges = kept;
}

