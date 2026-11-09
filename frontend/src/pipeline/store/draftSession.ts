/**
 * One pipeline tab's side of its shared draft (ADR-0088): a Yjs copy of the
 * draft, kept in step with the daemon over the draft's socket, and with the
 * tab of the editor's store.
 *
 * - A change the store makes to the tab lands in the copy as a delta, field
 *   by field (`model/draftDelta.ts`), and goes to the daemon.
 * - A change from elsewhere (another participant, the daemon, an Undo) is
 *   read back into the tab.
 * - Undo and Redo replay my own changes only (a `Y.UndoManager` that tracks
 *   this tab's origin), capped like the history of a run tab.
 */

import * as Y from "yjs";
import type { DraftNotice } from "../../shared/generated/DraftNotice";
import type { DraftServerMessage } from "../../shared/generated/DraftServerMessage";
import type { PresenceState } from "../../shared/generated/PresenceState";
import { openDraftSocket, SYNC_REPLY, SYNC_REQUEST, UPDATE, type DraftSocket } from "../api/draftSocket";
import { describeChange } from "../model/draftChange";
import { applyObject, applyPrompts, promptsOf } from "../model/draftDelta";
import { fromDraftPipeline, toDraftPipeline } from "../model/draftForm";
import type { EditorPipelineDef } from "../model/pipeline";
import { takeDraftGesture } from "./draftGesture";
import type { DraftTabState, OpenPipeline } from "./editState";
import { HISTORY_CAP } from "./tabHistory";

/** What the session reads and writes of the editor's store. */
export interface DraftHost {
  tab: (tabId: string) => OpenPipeline | undefined;
  /** Write the tab's definition read from the draft (not a local edit). */
  showDraft: (tabId: string, pipeline: EditorPipelineDef, prompts: Record<string, string>) => void;
  setDraftState: (tabId: string, patch: Partial<DraftTabState>) => void;
  /** A rename moved the pipeline: the tab follows. */
  moved: (tabId: string, to: string) => void;
  /** The draft cannot open: the tab edits without it. */
  detach: (tabId: string) => void;
  /** Someone outside the canvas changed the draft. */
  notify: (tabId: string, notice: DraftNotice) => void;
}

/** The origin of my own changes: the only ones Undo replays. */
const MINE = "mine";
/** My changes Undo never replays (a prompt typed, no gesture noted). */
const MINE_UNTRACKED = "mine-untracked";
/** What came from the daemon. */
const REMOTE = "remote";

/** Changes closer than this fold into one Undo step when they share a gesture. */
const CAPTURE_MS = 500;

/** « Still here », so the daemon can tell a gone connection (it waits 20 s). */
const HEARTBEAT_MS = 5_000;

export const INITIAL_DRAFT_STATE: DraftTabState = {
  status: "connecting",
  synced: false,
  canWrite: false,
  connection: null,
  participants: [],
  changes: 0,
  canUndo: false,
  canRedo: false,
  undoLabel: null,
  notices: [],
};

export class DraftSession {
  private doc!: Y.Doc;
  private pipelineMap!: Y.Map<unknown>;
  private promptMap!: Y.Map<unknown>;
  private undo!: Y.UndoManager;
  /** My last transaction added an undo step, or folded into the top one. */
  private stepTouched = false;
  private generation: string | null = null;
  private canWrite = false;
  private synced = false;
  private lastKey: string | null = null;
  private presence: PresenceState = { selection: null, typing: null };
  private readonly socket: DraftSocket;
  private readonly heartbeat: ReturnType<typeof setInterval>;
  /** The definition the tab holds, as the copy last gave it. */
  shown: { pipeline: EditorPipelineDef; prompts: Record<string, string> };

  readonly tabId: string;
  private readonly host: DraftHost;

  constructor(tabId: string, host: DraftHost) {
    this.tabId = tabId;
    this.host = host;
    const tab = host.tab(tabId);
    this.shown = { pipeline: tab!.pipeline, prompts: tab!.prompts };
    this.newDoc();
    this.socket = openDraftSocket(tabId, {
      onOpen: () => {
        this.socket.sendBinary(SYNC_REQUEST, Y.encodeStateVector(this.doc));
        this.socket.send({ type: "presence", presence: this.presence });
      },
      onBinary: (tag, payload) => this.received(tag, payload),
      onMessage: (message) => this.heard(message),
      onState: (state) =>
        host.setDraftState(tabId, { status: state === "ouvert" ? "live" : state === "connexion" ? "connecting" : "offline" }),
    });
    this.heartbeat = setInterval(() => this.socket.send({ type: "heartbeat" }), HEARTBEAT_MS);
  }

  private newDoc(): void {
    this.undo?.destroy();
    this.doc?.destroy();
    this.doc = new Y.Doc();
    this.pipelineMap = this.doc.getMap("pipeline");
    this.promptMap = this.doc.getMap("prompts");
    this.undo = new Y.UndoManager(this.pipelineMap, {
      trackedOrigins: new Set([MINE]),
      captureTimeout: CAPTURE_MS,
    });
    this.undo.on("stack-item-added", (event: { type: "undo" | "redo" }) => {
      if (event.type === "undo") this.stepTouched = true;
      if (event.type === "undo" && this.undo.undoStack.length > HISTORY_CAP) this.undo.undoStack.shift();
      this.publishHistory();
    });
    this.undo.on("stack-item-updated", (event: { type: "undo" | "redo" }) => {
      if (event.type === "undo") this.stepTouched = true;
    });
    this.undo.on("stack-item-popped", () => this.publishHistory());
    this.doc.on("update", (update: Uint8Array, origin: unknown) => {
      if (origin !== REMOTE && this.canWrite) this.socket.sendBinary(UPDATE, update);
    });
    this.doc.on("afterTransaction", (transaction: Y.Transaction) => {
      if (transaction.origin === MINE || transaction.origin === MINE_UNTRACKED) return;
      if (transaction.changed.size === 0) return;
      this.readBack();
    });
  }

  private received(tag: number, payload: Uint8Array): void {
    if (tag === SYNC_REQUEST) {
      if (this.canWrite) this.socket.sendBinary(SYNC_REPLY, Y.encodeStateAsUpdate(this.doc, payload));
      return;
    }
    if (tag !== SYNC_REPLY && tag !== UPDATE) return;
    Y.applyUpdate(this.doc, payload, REMOTE);
    if (tag === SYNC_REPLY && !this.synced) {
      this.synced = true;
      this.readBack();
      this.host.setDraftState(this.tabId, { synced: true });
    }
  }

  private heard(message: DraftServerMessage): void {
    switch (message.type) {
      case "welcome":
        // Another history than my copy's (the draft was started again):
        // drop the copy rather than merge two unrelated ones.
        if (this.generation != null && this.generation !== message.generation) {
          this.newDoc();
          this.synced = false;
          this.socket.sendBinary(SYNC_REQUEST, Y.encodeStateVector(this.doc));
        }
        this.generation = message.generation;
        this.canWrite = message.can_write;
        this.host.setDraftState(this.tabId, { canWrite: message.can_write, connection: message.connection });
        return;
      case "participants":
        this.host.setDraftState(this.tabId, { participants: message.participants });
        return;
      case "status":
        this.host.setDraftState(this.tabId, { changes: message.changes });
        return;
      case "moved":
        this.host.moved(this.tabId, message.to);
        return;
      case "removed":
        // The pipeline is gone: nothing to reconnect to.
        this.stop();
        this.host.setDraftState(this.tabId, { status: "offline", canWrite: false });
        return;
      case "unavailable":
        this.stop();
        this.host.detach(this.tabId);
        return;
      case "notice":
        this.host.notify(this.tabId, message.notice);
        return;
      default:
        return;
    }
  }

  /** The tab now holds what the copy holds. */
  private readBack(): void {
    if (!this.synced || this.pipelineMap.size === 0) return;
    const pipeline = fromDraftPipeline(this.pipelineMap.toJSON(), this.shown.pipeline);
    // An empty prompt the tab did not have is one opened for typing
    // (`openPrompt`), not one somebody cleared: no prompt yet.
    const prompts = Object.fromEntries(
      Object.entries(promptsOf(this.promptMap)).filter(([node, text]) => text !== "" || node in this.shown.prompts),
    );
    this.shown = { pipeline, prompts };
    this.host.showDraft(this.tabId, pipeline, prompts);
  }

  /** The store changed the tab: the change lands in the copy. */
  pushLocal(tab: OpenPipeline): void {
    const before = this.shown;
    this.shown = { pipeline: tab.pipeline, prompts: tab.prompts };
    if (!this.synced || !this.canWrite) return;
    const gesture = takeDraftGesture(this.tabId);
    const tracked = gesture != null && before.pipeline !== tab.pipeline;
    if (tracked && gesture.track && (gesture.coalesceKey == null || gesture.coalesceKey !== this.lastKey)) {
      this.undo.stopCapturing();
    }
    if (gesture?.track !== false) this.lastKey = gesture?.coalesceKey ?? null;
    this.stepTouched = false;
    this.doc.transact(() => {
      if (before.pipeline !== tab.pipeline) {
        applyObject(this.pipelineMap, toDraftPipeline(before.pipeline), toDraftPipeline(tab.pipeline));
      }
      if (before.prompts !== tab.prompts) applyPrompts(this.promptMap, before.prompts, tab.prompts);
    }, tracked ? MINE : MINE_UNTRACKED);
    const top = this.undo.undoStack[this.undo.undoStack.length - 1];
    // Only a step this change wrote to: a change that made no edit to the
    // copy must not rename an older step, which peers' edits have moved on from.
    if (tracked && top && this.stepTouched) {
      // A change made within the capture window folds into the step before it
      // (a drawn edge, then its anchors): the label names the whole step, from
      // what it started on, the way Undo reverts it.
      if (!top.meta.has("from")) top.meta.set("from", before.pipeline);
      const from = top.meta.get("from") as EditorPipelineDef;
      const label = describeChange(from, tab.pipeline) ?? describeChange(before.pipeline, tab.pipeline);
      if (label) top.meta.set("label", label);
    }
    this.publishHistory();
  }

  /** The draft as this tab holds it, for a save. */
  content(): { pipeline: unknown; prompts: Record<string, string> } {
    return { pipeline: this.pipelineMap.toJSON(), prompts: promptsOf(this.promptMap) };
  }

  /**
   * Make sure the prompt of `nodeId` has its text before anyone types in it:
   * two people starting a new prompt at once then type into the same one,
   * rather than each into a text of their own of which one is dropped.
   */
  openPrompt(nodeId: string): void {
    if (!this.synced || !this.canWrite || this.promptMap.has(nodeId)) return;
    this.doc.transact(() => this.promptMap.set(nodeId, new Y.Text()), MINE_UNTRACKED);
  }

  /** Undo my last change; nobody else's. */
  undoMine(): void {
    this.replay(this.undo.undoStack, () => this.undo.undo(), this.undo.redoStack);
  }

  redoMine(): void {
    this.replay(this.undo.redoStack, () => this.undo.redo(), this.undo.undoStack);
  }

  private replay(from: { meta: Map<unknown, unknown> }[], step: () => void, to: { meta: Map<unknown, unknown> }[]): void {
    if (!this.canWrite) return;
    const label = from[from.length - 1]?.meta.get("label");
    this.lastKey = null;
    step();
    const moved = to[to.length - 1];
    if (moved && label) moved.meta.set("label", label);
    this.publishHistory();
  }

  private publishHistory(): void {
    const top = this.undo.undoStack[this.undo.undoStack.length - 1];
    this.host.setDraftState(this.tabId, {
      canUndo: this.undo.undoStack.length > 0,
      canRedo: this.undo.redoStack.length > 0,
      undoLabel: (top?.meta.get("label") as string | undefined) ?? null,
    });
  }

  /** Say what this tab selected, or where it types. */
  present(patch: Partial<PresenceState>): void {
    const next = { ...this.presence, ...patch };
    if (JSON.stringify(next) === JSON.stringify(this.presence)) return;
    this.presence = next;
    this.socket.send({ type: "presence", presence: next });
  }

  private stop(): void {
    clearInterval(this.heartbeat);
    this.socket.close();
  }

  close(): void {
    this.stop();
    this.undo.destroy();
    this.doc.destroy();
  }
}
