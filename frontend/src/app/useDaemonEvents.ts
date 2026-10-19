import { useEffect, useRef } from "react";
import type { useDaemonSocket } from "../run/store/useDaemonSocket";
import { useEditStore } from "../pipeline/store/editStore";
import { trailingWindow } from "../shared/lib/trailingWindow";
import { refreshGestureRights } from "../run/store/gestureRightsStore";
import { announceHarnessCatalogueChanged } from "../harness/store/useHarnessCatalog";

/** What the live stream refreshes. */
export interface DaemonEventTargets {
  subscribe: ReturnType<typeof useDaemonSocket>["subscribe"];
  selectedTriggerId: string | null;
  refreshRuns: () => void;
  refreshRun: () => void;
  refreshSessions: () => void;
  refreshTriggers: () => void;
  refreshProjects: () => void;
  setTriggersPaused: (paused: boolean) => void;
  /** Read the global Trigger pause again from the daemon. */
  refreshTriggersPaused: () => void;
  bumpFiresRefreshKey: () => void;
}

/** The shell's dispatch of the daemon's `/ws` messages. */
export function useDaemonEvents({
  subscribe,
  selectedTriggerId,
  refreshRuns,
  refreshRun,
  refreshSessions,
  refreshTriggers,
  refreshProjects,
  setTriggersPaused,
  refreshTriggersPaused,
  bumpFiresRefreshKey,
}: DaemonEventTargets) {
  const reloadPipeline = useEditStore((s) => s.reloadPipeline);
  const showListedDraft = useEditStore((s) => s.showListedDraft);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  // #368: mirror readable inside the stable WS callback (App.tsx subscribe
  // effect) without widening its deps — same latest-value idiom as
  // currentIdRef in useSelectedRun. A ref read during render is banned by
  // react-hooks/refs (see note ~L148), but here read/write are outside render.
  const selectedTriggerIdRef = useRef<string | null>(null);

  // #368: keep the ref in step with the selected Trigger so the stable WS
  // callback below reads the committed value, not the stale-null closure.
  useEffect(() => {
    selectedTriggerIdRef.current = selectedTriggerId;
  }, [selectedTriggerId]);

  // The latest refreshers, so the batched calls below survive a change of
  // them (another Run selected) instead of being dropped with the effect.
  const refreshers = useRef({ refreshRuns, refreshRun, refreshSessions });
  useEffect(() => {
    refreshers.current = { refreshRuns, refreshRun, refreshSessions };
  }, [refreshRuns, refreshRun, refreshSessions]);

  useEffect(() => {
    // ADR-0084: a burst of events costs one refresh of each list per window
    // (~300 ms), not one per event.
    const runs = trailingWindow(() => refreshers.current.refreshRuns());
    const run = trailingWindow(() => refreshers.current.refreshRun());
    const sessions = trailingWindow(() => refreshers.current.refreshSessions());
    // What only a resync re-reads, in one window too: the daemon may send
    // several in a burst, and the stream's own may follow one.
    const lists = trailingWindow(() => {
      refreshTriggers();
      refreshTriggersPaused();
      refreshProjects();
      void loadPipelines();
    });
    // CORE-42: the gestures shown follow the rights and the Projets.
    const gestures = trailingWindow(refreshGestureRights);
    const unsubscribe = subscribe((msg) => {
      // ADR-0083 §6: back from an absence (the stream reopened, the tab shows
      // again, the network is back) or told by the daemon that this client
      // lost messages. Nothing is replayed: read the whole picture again.
      if (msg.type === "resync") {
        runs();
        run();
        sessions();
        lists();
        gestures();
        return;
      }
      if (msg.type === "pipeline_changed" && msg.pipeline_id) {
        reloadPipeline(msg.pipeline_id);
        loadPipelines();
        return;
      }
      // ADR-0088: the list shows who has a shared draft open, live.
      if (msg.type === "pipeline_draft_changed" && msg.pipeline_id && msg.draft) {
        showListedDraft(msg.pipeline_id, msg.draft);
        return;
      }
      // A Pipeline's validation changed (another tab or client, or a Projet
      // deleted): the indicators and the launch forms read it from the list.
      if (msg.type === "pipeline_validation_changed") {
        loadPipelines();
        return;
      }
      // #348: global pause flip — reflect it live across every client. The
      // per-Trigger `enabled` state (and hence the Triggers list) is untouched,
      // so no refresh is needed; just mirror the flag.
      if (msg.type === "triggers_paused") {
        setTriggersPaused(!!msg.paused);
        return;
      }
      // A harness's catalogue probed again, here or in another browser: every
      // model picker of the tab reads it anew.
      if (msg.type === "harness_catalogue_changed") {
        announceHarnessCatalogueChanged();
        return;
      }
      // #552: a Projet mutation (name/harness/members) — refresh the Projet list
      // so the Runs and Triggers regrouping is live across clients. The Runs /
      // Triggers rows themselves are unchanged, so only the Projet list refetches.
      if (msg.type === "project_changed") {
        refreshProjects();
        gestures();
        return;
      }
      // Trigger lifecycle (#160/#162): create/update/delete refreshes the
      // Triggers list; a fire also creates a Run, so refresh both.
      if (
        msg.type === "trigger_created" ||
        msg.type === "trigger_updated" ||
        msg.type === "trigger_deleted" ||
        msg.type === "trigger_fired"
      ) {
        refreshTriggers();
        // A `trigger_updated` about every Trigger: an extension signalled that
        // the suspensions changed, as when an Actor's rights change.
        if (msg.type === "trigger_updated" && !msg.trigger_id) gestures();
        if (msg.type === "trigger_fired") {
          runs();
          // #341 + #368: only the OPEN trigger's panel refetches, so an
          // unrelated fire doesn't flash/reload the currently-viewed history.
          // Read the ref (not selectedTriggerId) to keep this stable callback
          // out of the effect deps and dodge the stale-null closure.
          if (msg.trigger_id === selectedTriggerIdRef.current) {
            bumpFiresRefreshKey();
          }
        }
        return;
      }
      // #315: an archived run's outputs are now preserved (ADR-0020) and its
      // `/pipeline` endpoint keeps serving, so we no longer prune the open tab
      // on `run_archived`. `refreshRun` below flips the run's status to
      // `archived`, which turns the open canvas read-only in place — the run the
      // user is watching stays put instead of vanishing.
      runs();
      run();
      // Node start/complete/fail/waiting transitions change the live session
      // count (#159) — keep the status-bar counter current.
      sessions();
    });
    return () => {
      unsubscribe();
      runs.cancel();
      run.cancel();
      sessions.cancel();
      lists.cancel();
      gestures.cancel();
    };
  }, [
    subscribe,
    refreshTriggers,
    setTriggersPaused,
    refreshTriggersPaused,
    refreshProjects,
    reloadPipeline,
    showListedDraft,
    loadPipelines,
    bumpFiresRefreshKey,
  ]);
}
