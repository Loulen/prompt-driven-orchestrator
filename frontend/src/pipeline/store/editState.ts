/**
 * The editor's state: the open tabs, the selection, each tab's history, and
 * every gesture the store answers.
 */

import type { Actor } from "../../shared/generated/Actor";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import type { DraftNotice } from "../../shared/generated/DraftNotice";
import type { DraftSummary } from "../../shared/generated/DraftSummary";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import type { NoteDef } from "../../shared/generated/NoteDef";
import type { EditorNodeDef, EditorPipelineDef, PipelineScope } from "../model/pipeline";
import type { SaveResult } from "../api/pipelines";
import type { SaveAs } from "../api/versions";


// `"run"` is not a canvas element: it means the right pane shows the Run-info /
// Repositories sidebar for the active run tab (#465 slice 2, F1). It is set by
// an explicit toggle (canvas toolbar) rather than a canvas click, and — unlike
// `"none"` — it survives the App auto-snap, so the panel stays reachable while a
// live run's node is running. A tab switch brings back the selection the tab
// had (`tabSelections`), `"none"` the first time.
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

export interface SaveErrorData {
  message: string;
  line?: number;
}

/**
 * The shared draft of a pipeline tab (ADR-0088): the tab edits it with every
 * other participant. Absent on a run tab, which edits the Run's snapshot.
 */
export interface DraftTabState {
  /** The socket: opening, open, or down (edits stay local until it is back). */
  status: "connecting" | "live" | "offline";
  /** The draft arrived at least once; before, the tab takes no edit. */
  synced: boolean;
  /** When it may not update the pipeline, the tab reads. */
  canWrite: boolean;
  /** This tab's connection among the participants. */
  connection: string | null;
  participants: DraftParticipant[];
  /** The draft's changes since the file; 0 is nothing to save. */
  changes: number;
  canUndo: boolean;
  canRedo: boolean;
  /** The words of my last change, for the Undo tooltip. */
  undoLabel: string | null;
  /**
   * What changed the draft from outside the canvas (a file written outside
   * PDO, the library assistant), oldest first, until dismissed.
   */
  notices: DraftNoticeEntry[];
}

/** A notice of the draft, as the tab keeps it until it is dismissed. */
export interface DraftNoticeEntry {
  id: number;
  notice: DraftNotice;
}

/**
 * ADR-0088: the version of a pipeline a tab shows, read only (« View » in the
 * history). Such a tab has no draft and takes no edit.
 */
export interface VersionTabState {
  pipelineId: string;
  number: number;
  title: string;
  author: Actor | null;
  savedAt: string;
}

/** The id of the tab that shows the version `number` of `pipelineId`. */
export function versionTabId(pipelineId: string, number: number): string {
  return `__version__${pipelineId}@${number}`;
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
  /**
   * ADR-0085: a run tab opens in « pilotage », locked, with no authoring
   * gesture. `true` once the user chose « Edit for this run »; « Finish
   * editing » drops it. Meaningless on a pipeline tab (no `runId`), which is
   * always editable.
   */
  runEditing?: boolean;
  saveError?: SaveErrorData;
  /** ADR-0088: the shared draft a pipeline tab edits. */
  draft?: DraftTabState;
  /** ADR-0088: the version a read-only tab shows; absent on every other tab. */
  version?: VersionTabState;
  /** @deprecated Pipeline library bindings were removed with scoped pipelines. */
  libraryId?: string | null;
  /** @deprecated Pipeline library bindings were removed with scoped pipelines. */
  libraryScope?: "repo" | "user" | null;
}

/**
 * A tab holds unsaved work that a close/replace would silently discard (#342).
 * `saveError` implies `dirty` (it follows a failed save that leaves `dirty`),
 * but we spell it out so the guard reads as "would we lose anything the user
 * can't get back". `externalDirty` is intentionally excluded — it's the
 * transient 2 s hot-reload flash, not user work. A live shared draft
 * (ADR-0088) holds its changes for everyone: closing its tab loses none.
 */
export function hasUnsavedWork(t: OpenPipeline): boolean {
  if (t.saveError != null) return true;
  if (t.draft?.status === "live" && t.draft.synced) return false;
  return t.dirty;
}

/**
 * ADR-0085: a run tab in « pilotage », followed and never modified. Every
 * definition mutation of the store is a no-op on such a tab, so no gesture
 * (canvas, inspector, keyboard) changes it without « Edit for this run ».
 */
export function isRunTabLocked(t: OpenPipeline | undefined | null): boolean {
  return t?.runId != null && !t.runEditing;
}

/**
 * A tab that takes no definition change: a locked run tab, or a pipeline tab
 * whose shared draft has not arrived yet or that only reads it (ADR-0088).
 */
export function isTabLocked(t: OpenPipeline | undefined | null): boolean {
  if (isRunTabLocked(t)) return true;
  if (t?.version != null) return true;
  return t?.draft != null && (!t.draft.synced || !t.draft.canWrite);
}

/** The active tab takes no edit (point-of-use selector). */
export function selectActiveTabLocked(s: { openTabs: OpenPipeline[]; activeTabId: string | null }): boolean {
  return isTabLocked(s.openTabs.find((t) => t.id === s.activeTabId));
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
  /**
   * The selection each tab had when another one came to the front: back on
   * the tab, the canvas and the inspector show it again, if it still holds.
   */
  tabSelections: Record<string, Selection>;
  scrollToPort: string | null;
  lastSavedAt: Record<string, number>;
  // Undo/redo history keyed by tabId (ADR-0014 / #226). Lazily initialized on
  // the first tracked mutation; `canUndo`/`canRedo` are derived by components
  // with a selector, never stored.
  history: Record<string, TabHistory>;
  // The last tab REKEY (#774): a rename moved the tab `from` → `to`. The same
  // tab under a new id, not a tab switch — the right-pane router reads it so a
  // Save that renames the pipeline keeps Pipeline info open (#948).
  lastRekey: { from: string; to: string } | null;

  // Single-tab mode (#342): at most one pipeline/run tab. A per-client UI pref
  // (localStorage, NOT instance_config) seeded once at store creation. When on,
  // opening a pipeline/run REPLACES the current tab instead of appending.
  singleTabMode: boolean;
  // A parked single-tab gesture awaiting confirmation (would discard unsaved
  // work). Null when nothing is pending.
  pendingSingleTab: PendingSingleTab | null;

  loadPipelines: () => Promise<void>;
  /** ADR-0088: the shared draft of the listed pipeline `id`, as announced. */
  showListedDraft: (id: string, draft: DraftSummary) => void;
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
  /** Move the tab `id` to `index` in the tab bar (a drag of the tab). */
  moveTab: (id: string, index: number) => void;
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
  updatePipelineMeta: (updates: Partial<Pick<EditorPipelineDef, "name" | "version" | "variables" | "prompt_required" | "wait_for_started_nodes" | "grid_size">>) => void;

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

  /**
   * Save the tab `id`. A tab with a shared draft freezes it as the version
   * `version` says (ADR-0088), a new one by default; the daemon's answer, or
   * `undefined` when the save failed (the tab carries the error).
   */
  save: (id: string, version?: SaveAs) => Promise<SaveResult | undefined>;
  /** ADR-0088: the tab whose « Save… » window is open, `null` when none. */
  saveVersionFor: string | null;
  /** Save the tab `id`: a shared draft opens the « Save… » window first. */
  requestSave: (id: string) => void;
  closeSaveVersion: () => void;
  /** ADR-0088: open the version `number` of `pipelineId` in a read-only tab. */
  openPipelineVersion: (pipelineId: string, number: number) => Promise<void>;
  /**
   * ADR-0088: the version `number` of `pipelineId` becomes the content of its
   * draft, for every participant, as one change of mine Undo takes back. No
   * version is created.
   */
  restoreVersion: (pipelineId: string, number: number) => Promise<void>;
  /** ADR-0085: « Edit for this run », a run tab unlocked for hot editing. */
  startRunEditing: (id: string) => void;
  /**
   * ADR-0085: « Finish editing », back to « pilotage ». Unsaved edits are
   * discarded (the caller confirmed): the tab re-reads the Run's snapshot.
   */
  finishRunEditing: (id: string) => Promise<void>;
  /**
   * ADR-0085: « Overwrite default pipeline », the run tab saved for this Run
   * first if needed, then the shared pipeline replaced by the Run's whole
   * snapshot. Rejects with the daemon's message when either fails.
   */
  overwriteDefaultPipeline: (id: string) => Promise<void>;
  clearSaveError: (id: string) => void;

  reloadPipeline: (id: string) => Promise<void>;

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
