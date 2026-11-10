import { useRef, useState } from "react";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { RunStatus } from "../../../shared/generated/RunStatus";
import { forgetRun } from "../../api/runs";
import { cleanupRun, pauseRun, renameRun, resumeRun, retryAll } from "../../api/commands";
import { useRunShell } from "../../../session/store/useRunShell";
import { toastRefusal } from "../../../shared/authorization/refusal";

/**
 * The per-row actions of the Runs list: the confirm-gated cleanup / forget /
 * retry-all, pause / resume, the inline rename and the ad-hoc run shell.
 */
export function useRunRowActions(onSelectRun: (runId: string) => void) {
  const [confirmCleanup, setConfirmCleanup] = useState<
    { runId: string; status: RunStatus } | null
  >(null);
  const [confirmForget, setConfirmForget] = useState<string | null>(null);
  // #110 — run id awaiting Retry-all confirmation (the one destructive control).
  const [confirmRetryAll, setConfirmRetryAll] = useState<string | null>(null);
  // Ad-hoc bash shell opened on a terminal run (#316). Holds the tmux session
  // name to attach the inline terminal to.
  const runShell = useRunShell();
  const [renamingRunId, setRenamingRunId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);

  async function handleCleanup(runId: string) {
    try {
      await cleanupRun(runId);
    } catch (e) {
      // event-driven refresh will pick up state change; a refusal is said
      toastRefusal(e);
    }
    setConfirmCleanup(null);
  }

  async function handleForget(runId: string) {
    try {
      await forgetRun(runId);
    } catch (e) {
      // event-driven refresh will pick up state change; a refusal is said
      toastRefusal(e);
    }
    setConfirmForget(null);
  }

  // #110 — Pause/Resume are cheap, reversible, fire-and-forget (silent catch like
  // handleCleanup); the daemon's WS events re-drive the list state.
  async function handlePause(runId: string) {
    try {
      await pauseRun(runId);
    } catch (e) {
      // event-driven refresh will pick up state change; a refusal is said
      toastRefusal(e);
    }
  }

  async function handleResume(runId: string) {
    try {
      await resumeRun(runId);
    } catch (e) {
      // event-driven refresh will pick up state change; a refusal is said
      toastRefusal(e);
    }
  }

  // #110 — Retry-all is destructive (archives the original), so it's confirm-gated.
  // The daemon replies 201 with the offspring run_id; selecting it fires an
  // independent fetch-by-id (App's handleSelectRun) — safe before the WS refresh
  // lands the new row in `runs`.
  async function handleRetryAll(runId: string) {
    try {
      const { run_id } = await retryAll(runId);
      onSelectRun(run_id);
    } catch (e) {
      // event-driven refresh will pick up state change; a refusal is said
      toastRefusal(e);
    }
    setConfirmRetryAll(null);
  }

  function startRename(run: RunListEntry) {
    setRenamingRunId(run.run_id);
    setRenameValue(run.name ?? "");
    setTimeout(() => renameInputRef.current?.focus(), 0);
  }

  async function commitRename() {
    if (!renamingRunId) return;
    try {
      await renameRun(renamingRunId, renameValue.trim());
    } catch (e) {
      // event-driven refresh will pick up state change; a refusal is said
      toastRefusal(e);
    }
    setRenamingRunId(null);
    setRenameValue("");
  }

  function cancelRename() {
    setRenamingRunId(null);
    setRenameValue("");
  }

  return {
    confirmCleanup,
    setConfirmCleanup,
    confirmForget,
    setConfirmForget,
    confirmRetryAll,
    setConfirmRetryAll,
    runShell,
    renamingRunId,
    renameValue,
    setRenameValue,
    renameInputRef,
    handleCleanup,
    handleForget,
    handlePause,
    handleResume,
    handleRetryAll,
    startRename,
    commitRename,
    cancelRename,
  };
}
