/**
 * The editor's state: the open tabs, the selection, each tab's history, and
 * every gesture the store answers.
 */

import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import type { NoteDef } from "../../shared/generated/NoteDef";
import type { EditorNodeDef, EditorPipelineDef, PipelineScope } from "../model/pipeline";


// `"run"` is not a canvas element: it means the right pane shows the Run-info /
// Repositories sidebar for the active run tab (#465 slice 2, F1). It is set by
// an explicit toggle (canvas toolbar) rather than a canvas click, and — unlike
// `"none"` — it survives the App auto-snap, so the panel stays reachable while a
// live run's node is running. Cleared to `"none"` on any tab switch.
export type SelectionKind = "node" | "edge" | "region" | "note" | "run" | "none";

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
  /**
   * The selected canvas note's `id` when `kind === "note"` (#307). Notes carry
   * a stable `id`, so the id is the selection key the note inspector and
   * `updateNote`/`moveNote`/`deleteNote` use. Undefined for other selections.
   */
  noteId?: string;
}

export interface ConflictData {
  pipeline: EditorPipelineDef;
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
  pipeline: EditorPipelineDef;
  prompts: Record<string, string>;
  diagnostics: string[];
  dirty: boolean;
  externalDirty: boolean;
  runId?: string;
  conflict?: ConflictData;
  saveError?: SaveErrorData;
  /** @deprecated Pipeline library bindings were removed with scoped pipelines. */
  libraryId?: string | null;
  /** @deprecated Pipeline library bindings were removed with scoped pipelines. */
  libraryScope?: "repo" | "user" | null;
}

/**
 * A tab holds unsaved work that a close/replace would silently discard (#342).
 * `conflict`/`saveError` both imply `dirty` (a conflict is only recorded inside
 * `if (tab.dirty)`, a save-error follows a failed save that leaves `dirty`), but
 * we spell them out so the guard reads as "would we lose anything the user can't
 * get back". `externalDirty` is intentionally excluded — it's the transient
 * 2 s hot-reload flash, not user work.
 */
export function hasUnsavedWork(t: OpenPipeline): boolean {
  return t.dirty || t.conflict != null || t.saveError != null;
}

/**
 * A single-tab-mode gesture (#342) parked because confirming it would discard
 * unsaved work. Resolved by a global confirm modal: `confirmPendingSingleTab`
 * performs it, `cancelPendingSingleTab` drops it (keeping the tabs).
 *
 * - `tab` set → an open-replace: `tab` becomes the sole tab, every current tab
 *   is discarded.
 * - `tab` null → the enable-toggle collapse: the active tab is kept, the others
 *   (`victims`) are closed.
 */
export interface PendingSingleTab {
  tab: OpenPipeline | null;
  /** Tabs that will be discarded; the modal names the unsaved ones. */
  victims: OpenPipeline[];
}

/**
 * Per-tab undo/redo history (ADR-0014 / #226). Entries are whole `EditorPipelineDef`
 * object references captured *before* a structural mutation — NOT deep clones.
 * This is safe only because every store mutation is copy-on-write (it rebuilds
 * whole arrays, never mutating a node/edge/port in place), so a captured
 * reference stays frozen as long as nobody writes through it. The history is
 * in-memory only (no cross-reload persistence) and excludes run state (it lives
 * in a separate overlay) and prompt text (intentionally not tracked).
 */
export interface TabHistory {
  /** Pre-mutation snapshots, oldest→newest. The top is the most recent restore point. */
  past: EditorPipelineDef[];
  /** States undone, available for redo (newest at the end). */
  future: EditorPipelineDef[];
  /** Coalescing key of the most recent push (null = never coalesce). */
  lastKey: string | null;
  /** `Date.now()` of the most recent push, for the time-window coalescer. */
  lastAt: number;
}

export interface EditState {
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

  // Single-tab mode (#342): at most one pipeline/run tab. A per-client UI pref
  // (localStorage, NOT instance_config) seeded once at store creation. When on,
  // opening a pipeline/run REPLACES the current tab instead of appending.
  singleTabMode: boolean;
  // A parked single-tab gesture awaiting confirmation (would discard unsaved
  // work). Null when nothing is pending.
  pendingSingleTab: PendingSingleTab | null;

  loadPipelines: () => Promise<void>;
  openPipeline: (id: string, scope?: PipelineScope) => Promise<void>;
  openRunPipeline: (runId: string) => Promise<void>;
  closeRunPipeline: (runId: string) => void;
  closeTab: (id: string) => void;
  // Atomic mass-close (#342): removes every id in ONE `set()`. Never loop
  // `closeTab` — the App reconciliation (#247/#320) runs at render on a
  // reference change of `selection`/`activeTabId`, so intermediate states where
  // `activeTabId` points at a closing tab flash the center panel and can
  // self-close the Trigger detail.
  closeTabs: (ids: string[]) => void;
  // Single-tab pref (#342). Persists to localStorage immediately (at the toggle
  // change, not on a Save button) and, when enabling with several tabs open,
  // collapses to the active tab (parking for confirmation if any other is
  // unsaved).
  setSingleTabMode: (v: boolean) => void;
  confirmPendingSingleTab: () => void;
  cancelPendingSingleTab: () => void;
  setActiveTab: (id: string) => void;
  setSelection: (sel: Selection) => void;
  setScrollToPort: (port: string | null) => void;

  addNode: (node: EditorNodeDef) => void;
  updateNode: (nodeId: string, updates: Partial<EditorNodeDef>) => void;
  // Batched position write for a group drag (#232): xyflow's onNodeDragStop
  // hands us every dragged node at once; persist all their `view` coords in one
  // store mutation (one re-derivation, one dirty/save unit).
  updateNodeViews: (updates: { id: string; x: number; y: number }[]) => void;
  deleteNode: (nodeId: string) => void;
  duplicateNode: (nodeId: string) => void;

  addEdge: (edge: EdgeDef) => void;
  // `opts.track === false` mutates without pushing a history entry — used by the
  // draw-edge arrival-side stamp (#168) so a single edge-draw gesture folds into
  // ONE undo step instead of two (the `addEdge` push + a separate stamp push).
  updateEdge: (index: number, updates: Partial<EdgeDef>, opts?: { track?: boolean }) => void;
  // `opts.keepSelection` keeps the current selection instead of clearing it —
  // used by the inspector's per-source × (#339) so the panel stays open on the
  // node whose input was just deleted. Canvas deletions keep the default clear.
  deleteEdge: (index: number, opts?: { keepSelection?: boolean }) => void;

  // Region mutations (ADR-0011 / #150) — edit a bounded region's bound live.
  updateRegion: (regionId: string, updates: Partial<LoopRegion>) => void;
  // Explicit "fan out over a collection" gesture (#151 / #269): wraps `members`
  // in a `collection` region iterating the list field `over`. No-ops when any
  // member already belongs to a region (a node lives in at most one).
  createCollectionRegion: (members: string[], over: string) => void;

  // Note mutations (#307 / ADR-0018) — inert canvas notes. All are COW and
  // history-tracked (ADR-0014): they live on `EditorPipelineDef.notes`, so a
  // reference snapshot covers them for free as long as each reducer reassigns
  // the array rather than mutating in place.
  addNote: (note: NoteDef) => void;
  updateNote: (noteId: string, updates: Partial<NoteDef>) => void;
  // Position-only write for a note drag; coalesced at the drag-stop call site
  // (mirror of `updateNodeViews`), so a whole drag folds into one undo step.
  moveNote: (noteId: string, x: number, y: number) => void;
  deleteNote: (noteId: string) => void;

  /** `grid_size: null` clears the pipeline's own size (it follows the global
   *  default again) — the key is then dropped from the document, not nulled. */
  updatePipelineMeta: (updates: Partial<Pick<EditorPipelineDef, "name" | "version" | "variables" | "prompt_required" | "grid_size">>) => void;

  updatePrompt: (nodeId: string, content: string) => void;

  // Undo/redo (ADR-0014 / #226) — operate on the active tab's history.
  undo: () => void;
  redo: () => void;

  removePipeline: (id: string, scope?: PipelineScope) => Promise<void>;

  /**
   * #774 — rename a pipeline: the daemon moves `<id>.yaml` + its sidecar to
   * the slug of the new name and answers with the new id, so this action also
   * rekeys the open tab (and its undo history / last-saved stamp) when the
   * stem changed.
   */
  renamePipeline: (id: string, name: string) => Promise<void>;

  save: (id: string) => Promise<void>;
  flushPendingSaves: () => Promise<void>;
  clearSaveError: (id: string) => void;

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
    libraryScope: "repo" | "user" | null,
  ) => void;
}
