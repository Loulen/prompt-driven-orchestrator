import { useEffect, useRef } from "react";
import type { useDaemonSocket } from "../run/store/useDaemonSocket";
import { useEditStore } from "../pipeline/store/editStore";

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
  bumpFiresRefreshKey,
}: DaemonEventTargets) {
  const reloadPipeline = useEditStore((s) => s.reloadPipeline);
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

  useEffect(() => {
    return subscribe((msg) => {
      if (msg.type === "pipeline_changed" && msg.pipeline_id) {
        reloadPipeline(msg.pipeline_id);
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
      // #552: a Projet mutation (name/harness/members) — refresh the Projet list
      // so the Runs and Triggers regrouping is live across clients. The Runs /
      // Triggers rows themselves are unchanged, so only the Projet list refetches.
      if (msg.type === "project_changed") {
        refreshProjects();
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
        if (msg.type === "trigger_fired") {
          refreshRuns();
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
      refreshRuns();
      refreshRun();
      // Node start/complete/fail/waiting transitions change the live session
      // count (#159) — keep the status-bar counter current.
      refreshSessions();
    });
  }, [subscribe, refreshRuns, refreshRun, refreshSessions, refreshTriggers, setTriggersPaused, refreshProjects, reloadPipeline, loadPipelines, bumpFiresRefreshKey]);
}
