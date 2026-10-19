/**
 * The shared drafts of the open pipeline tabs (ADR-0088): one
 * {@link DraftSession} per pipeline tab (never a run tab, which edits the
 * Run's snapshot), opened with the tab and closed with it, kept in step with
 * the editor's store.
 */

import type { StoreApi } from "zustand";
import type { PresenceState } from "../../shared/generated/PresenceState";
import type { SelectionKind } from "../../shared/generated/SelectionKind";
import { DraftSession, INITIAL_DRAFT_STATE, type DraftHost } from "./draftSession";
import type { DraftTabState, EditState, OpenPipeline, Selection } from "./editState";
import { rekeyTab } from "./tabHistory";

const sessions = new Map<string, DraftSession>();
/** The store is being written from a draft: not a local edit to send. */
let showing = false;

/** The session of the tab `tabId`, if it has one. */
export function draftSessionOf(tabId: string | null | undefined): DraftSession | undefined {
  return tabId ? sessions.get(tabId) : undefined;
}

function patchTab(store: StoreApi<EditState>, tabId: string, patch: (tab: OpenPipeline) => Partial<OpenPipeline>): void {
  showing = true;
  try {
    store.setState((s) => ({
      openTabs: s.openTabs.map((t) => (t.id === tabId ? { ...t, ...patch(t) } : t)),
    }));
  } finally {
    showing = false;
  }
}

function hostOf(store: StoreApi<EditState>): DraftHost {
  return {
    tab: (tabId) => store.getState().openTabs.find((t) => t.id === tabId),
    showDraft: (tabId, pipeline, prompts) => {
      showing = true;
      try {
        store.setState((s) => {
          const tab = s.openTabs.find((t) => t.id === tabId);
          // An edge is selected by its position: a change of the edge list
          // from elsewhere makes it point at another edge.
          const edgesMoved = tab != null && tab.pipeline.edges.length !== pipeline.edges.length;
          const selection =
            s.activeTabId === tabId && edgesMoved && s.selection.kind === "edge"
              ? { kind: "none" as const, id: null }
              : s.selection;
          return {
            openTabs: s.openTabs.map((t) => (t.id === tabId ? { ...t, pipeline, prompts } : t)),
            selection,
          };
        });
      } finally {
        showing = false;
      }
    },
    setDraftState: (tabId, patch: Partial<DraftTabState>) =>
      patchTab(store, tabId, (t) => {
        const draft = { ...(t.draft ?? INITIAL_DRAFT_STATE), ...patch };
        // The « • » of the tab: the draft differs from the file, whoever
        // changed it.
        return patch.changes === undefined ? { draft } : { draft, dirty: patch.changes > 0 };
      }),
    moved: (tabId, to) => {
      // The draft opens again under the new id.
      store.setState((s) => rekeyTab(s, tabId, to, { draft: INITIAL_DRAFT_STATE }));
      void store.getState().loadPipelines();
    },
  };
}

function presenceSelection(selection: Selection): PresenceState["selection"] {
  const kinds: Partial<Record<Selection["kind"], SelectionKind>> = {
    node: "node",
    edge: "edge",
    note: "note",
    region: "region",
  };
  const kind = kinds[selection.kind];
  if (!kind) return null;
  const id =
    selection.kind === "edge"
      ? selection.edgeIndex != null ? String(selection.edgeIndex) : null
      : selection.kind === "note"
        ? selection.noteId ?? selection.id
        : selection.kind === "region"
          ? selection.regionId ?? selection.id
          : selection.id;
  return id != null ? { kind, id } : null;
}

/** Open, feed and close the drafts of `store`'s pipeline tabs from now on. */
export function startDraftSessions(store: StoreApi<EditState>): () => void {
  const host = hostOf(store);
  const sync = (state: EditState, previous: EditState | null) => {
    const pipelineTabs = state.openTabs.filter((t) => t.runId == null);
    const open = new Set(pipelineTabs.map((t) => t.id));
    for (const [tabId, session] of sessions) {
      if (!open.has(tabId)) {
        session.close();
        sessions.delete(tabId);
      }
    }
    for (const tab of pipelineTabs) {
      const session = sessions.get(tab.id);
      if (!session) {
        sessions.set(tab.id, new DraftSession(tab.id, host));
        if (!tab.draft) patchTab(store, tab.id, () => ({ draft: INITIAL_DRAFT_STATE }));
        continue;
      }
      if (!showing && (tab.pipeline !== session.shown.pipeline || tab.prompts !== session.shown.prompts)) {
        session.pushLocal(tab);
      }
    }
    if (previous && (state.selection !== previous.selection || state.activeTabId !== previous.activeTabId)) {
      for (const [tabId, session] of sessions) {
        session.present({ selection: tabId === state.activeTabId ? presenceSelection(state.selection) : null });
      }
    }
  };
  sync(store.getState(), null);
  const unsubscribe = store.subscribe((state, previous) => sync(state, previous));
  return () => {
    unsubscribe();
    for (const session of sessions.values()) session.close();
    sessions.clear();
  };
}
