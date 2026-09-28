import { useCallback, useEffect, useReducer, useState } from "react";
import { applyUpdate, fetchUpdateStatus } from "../api/update";
import { IDLE as UPDATE_IDLE, isUpdateInProgress, reduceUpdateFlow, type UpdateFlowEvent } from "../model/updateFlow";
import type { UpdateStatus } from "../model/update";
import type { ConnectionStatus } from "../../run/store/useDaemonSocket";

/**
 * The in-app update (#699) and the « What's new » changelog it opens from
 * (#698): the confirm, then the pure flow machine — applying → restarting →
 * verifying → reload / failed — fed by the socket and a poll of `GET /update`.
 */
export function useUpdateFlow(updateStatus: UpdateStatus | null, status: ConnectionStatus) {
  // #698: the « What's new » changelog modal, opened from the status-bar version (and
  // from Settings › Version & update). Lives here next to the other modals.
  const [changelogOpen, setChangelogOpen] = useState(false);
  // #699: the in-app update — a confirm (active Runs count, tmux survival), then the
  // pure flow machine: applying → restarting → verifying → reload / failed.
  const [updateConfirmOpen, setUpdateConfirmOpen] = useState(false);
  const [updateStarting, setUpdateStarting] = useState(false);
  const [updateFlow, dispatchUpdateFlow] = useReducer(reduceUpdateFlow, UPDATE_IDLE);
  const startUpdate = useCallback(async () => {
    setUpdateStarting(true);
    try {
      const fresh = updateStatus ?? (await fetchUpdateStatus());
      const res = await applyUpdate();
      dispatchUpdateFlow({
        type: "applied",
        attemptId: res.attempt_id,
        fromVersion: res.from_version || fresh.installed_version,
        now: Date.now(),
      });
    } catch (e) {
      dispatchUpdateFlow({ type: "apply-failed", error: e instanceof Error ? e.message : String(e) });
    } finally {
      setUpdateStarting(false);
      setUpdateConfirmOpen(false);
      setChangelogOpen(false);
    }
  }, [updateStatus]);
  // Feed the machine: the socket's transitions, and a poll of `GET /update` for the
  // whole in-progress window — while the old daemon still answers it reports a failed
  // executor; once the new one answers, its `installed_version` is the verdict (reload
  // when it differs from the one we started on). Polling rather than trusting the
  // cached `/sessions` value: right after a reconnect the cache still holds the OLD
  // version and would read as « nothing changed ».
  useEffect(() => {
    const ev: UpdateFlowEvent = { type: "socket", connected: status === "connected" };
    dispatchUpdateFlow(ev);
  }, [status]);
  useEffect(() => {
    if (!isUpdateInProgress(updateFlow)) return;
    let cancelled = false;
    const poll = () => {
      fetchUpdateStatus()
        .then((s) => {
          if (cancelled) return;
          dispatchUpdateFlow({ type: "status", status: s });
          dispatchUpdateFlow({ type: "version", version: s.installed_version });
        })
        .catch(() => {
          /* the daemon is down or restarting: the next tick retries */
        });
    };
    poll();
    const id = window.setInterval(poll, 1500);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [updateFlow]);
  useEffect(() => {
    if (updateFlow.phase !== "reload") return;
    const t = window.setTimeout(() => window.location.reload(), 600);
    return () => window.clearTimeout(t);
  }, [updateFlow.phase]);
  return {
    changelogOpen,
    setChangelogOpen,
    updateConfirmOpen,
    setUpdateConfirmOpen,
    updateStarting,
    updateFlow,
    dispatchUpdateFlow,
    startUpdate,
  };
}
