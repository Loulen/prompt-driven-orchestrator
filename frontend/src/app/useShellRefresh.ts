import { useEffect, useRef } from "react";
import type { ConnectionStatus } from "../run/store/useDaemonSocket";
import type { RunState } from "../shared/generated/RunState";
import { pickLatestLiveNode } from "../run/model/pickLatestLiveNode";
import { shouldAutoSnapToLiveNode } from "../lib/autoSnap";
import { useRecentReposStore } from "../stores/recentReposStore";
import { useEditStore } from "../pipeline/store/editStore";

/** What the shell refreshes at mount and on a reconnect. */
export interface ShellRefreshTargets {
  status: ConnectionStatus;
  selectedRun: RunState | null;
  refreshRuns: () => void;
  refreshSessions: () => void;
  refreshTriggers: () => void;
  refreshProjects: () => void;
  hydrateTriggersPause: () => void;
  refreshUpdateStatus: () => void;
}

/** The shell's lists hydrated once at mount, re-fetched on every reconnect, and
 *  the selection snapped to a live Run's latest running node. */
export function useShellRefresh({
  status,
  selectedRun,
  refreshRuns,
  refreshSessions,
  refreshTriggers,
  refreshProjects,
  hydrateTriggersPause,
  refreshUpdateStatus,
}: ShellRefreshTargets) {
  const mountedRef = useRef(false);
  const selection = useEditStore((s) => s.selection);
  const setSelection = useEditStore((s) => s.setSelection);
  useEffect(() => {
    if (!mountedRef.current) {
      mountedRef.current = true;
      refreshRuns();
      refreshSessions();
      refreshTriggers();
      refreshProjects();
      // #348: hydrate the global pause flag once — no trigger polling carries it,
      // so without this a page reload would show the banner off while paused.
      hydrateTriggersPause();
      useRecentReposStore.getState().refresh();
    }
  }, [refreshRuns, refreshSessions, refreshTriggers, refreshProjects, hydrateTriggersPause]);

  // A WS reconnect usually means the daemon restarted — possibly as a different
  // binary, so the /sessions payload (version included, #139) may be stale. An
  // idle daemon emits no event afterwards, so the subscribe-side refresh never
  // fires; re-fetch on every transition to "connected".
  useEffect(() => {
    if (status === "connected") {
      refreshSessions();
      // #697: a reconnect is how a completed update becomes visible — and how a
      // periodic check that landed while the socket was down reaches the badge.
      refreshUpdateStatus();
    }
  }, [status, refreshSessions, refreshUpdateStatus]);

  // On a live run with nothing selected, snap selection to the latest
  // running (or awaiting_user) node so the user immediately sees its terminal.
  // Re-fires whenever the user deselects on a still-live run — but never over a
  // deliberate selection (a node, an inspector, or the Run-info sidebar, #465
  // slice 2, F1). The gate is `shouldAutoSnapToLiveNode`; this effect only
  // resolves the concrete node and applies it.
  useEffect(() => {
    if (!selectedRun) return;
    if (!shouldAutoSnapToLiveNode(selection.kind, !!selection.id, selectedRun.status)) return;
    const nodeId = pickLatestLiveNode(selectedRun);
    if (!nodeId) return;
    setSelection({ kind: "node", id: nodeId });
  }, [selectedRun, selection.kind, selection.id, setSelection]);
}
