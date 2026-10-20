import { isLiveRun } from "../../model/runStatus";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { cascadeImpact } from "../../model/runTree";
import CleanupConfirmModal from "../CleanupConfirmModal";
import ForgetRunModal from "../ForgetRunModal";
import RunShellModal from "../../../session/components/RunShellModal";
import RetryAllConfirmModal from "./RetryAllConfirmModal";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";

interface Props {
  pane: RunsPaneState;
  runs: RunListEntry[];
}

/**
 * The Runs tab's overlays: the per-row confirms and the run shell. Rendered at
 * the panel's root, whatever the active tab.
 */
export default function RunsModals({ pane, runs }: Props) {
  const {
    confirmCleanup, setConfirmCleanup, handleCleanup, confirmForget, setConfirmForget, handleForget,
    runShell, confirmRetryAll, setConfirmRetryAll,
    handleRetryAll,
  } = pane;
  return (
    <>
      {confirmCleanup && (
        <CleanupConfirmModal
          runId={confirmCleanup.runId}
          isLive={
            isLiveRun(confirmCleanup.status)
          }
          cascade={cascadeImpact(runs, [confirmCleanup.runId])}
          onConfirm={() => handleCleanup(confirmCleanup.runId)}
          onCancel={() => setConfirmCleanup(null)}
        />
      )}

      {confirmForget && (
        <ForgetRunModal
          onConfirm={() => handleForget(confirmForget)}
          onCancel={() => setConfirmForget(null)}
        />
      )}

      {runShell.shell && (
        <RunShellModal session={runShell.shell.session} onClose={runShell.close} />
      )}

      {confirmRetryAll && (
        <RetryAllConfirmModal
          onConfirm={() => handleRetryAll(confirmRetryAll)}
          onCancel={() => setConfirmRetryAll(null)}
        />
      )}
    </>
  );
}
