import { useMemo } from "react";
import { Pause, RotateCcw, Trash2 } from "lucide-react";
import { useExtensionBulkActions } from "../../../shared/extensions/useActions";
import { cleanupRun, pauseRun, retryAll } from "../../api/commands";
import BulkActionBar from "../../../pipeline/components/BulkActionBar";
import BulkActionModal from "../../../pipeline/components/BulkActionModal";
import { cleanupTitle } from "./runsListTables";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";

/** The Runs tab's bulk-action bar and the confirm / progress modal it opens.
 *  After the core's actions, the extensions' `runs/bulk` ones (#30), called
 *  with the selected Runs. */
export default function RunsBulkActions({ pane }: { pane: RunsPaneState }) {
  const {
    runSelIds, runningWillStop, cleanupItems, retryItems, pauseItems, setRunBulkKind, clearSel,
    runBulkKind, cleanupCascade, handleRunsSettled, bulkRefused,
  } = pane;
  const extra = useMemo(() => ({ selection: { kind: "run" as const, ids: runSelIds } }), [runSelIds]);
  const extensionActions = useExtensionBulkActions("runs/bulk", extra);
  return (
    <>
      {/* #577 — the floating bulk bar of the Runs tab. */}
      {runSelIds.length > 0 && (
        <BulkActionBar
          count={runSelIds.length}
          live={runningWillStop > 0 ? { count: runningWillStop, note: `${runningWillStop} running will stop` } : null}
          actions={[
            // CORE-205: an action no selected Run allows the caller is greyed, with why.
            {
              key: "cleanup" as const,
              label: "Cleanup",
              icon: <Trash2 size={14} strokeWidth={1.5} aria-hidden />,
              destructive: true,
              disabled: cleanupItems.length === 0,
              disabledReason: bulkRefused.cleanup,
              onClick: () => setRunBulkKind("cleanup"),
            },
            {
              key: "retry" as const,
              label: "Retry",
              icon: <RotateCcw size={14} strokeWidth={1.5} aria-hidden />,
              disabled: retryItems.length === 0,
              disabledReason: bulkRefused.retry,
              onClick: () => setRunBulkKind("retry"),
            },
            {
              key: "pause" as const,
              label: "Pause",
              icon: <Pause size={14} strokeWidth={1.5} aria-hidden />,
              disabled: pauseItems.length === 0,
              disabledReason: bulkRefused.pause,
              onClick: () => setRunBulkKind("pause"),
            },
            ...extensionActions,
          ]}
          onClear={() => clearSel("runs")}
        />
      )}
      {runBulkKind === "cleanup" && (
        <BulkActionModal
          destructive
          runningLabel="Cleaning up"
          title={cleanupTitle(cleanupItems.length, cleanupCascade.finished)}
          description={
            <>
              This removes the selected runs' worktrees from disk and archives them.
              {runningWillStop > 0
                ? ` ${runningWillStop} running run${runningWillStop === 1 ? "" : "s"} will be stopped first.`
                : ""}
              {cleanupCascade.live > 0
                ? ` ${cleanupCascade.live} child run${cleanupCascade.live === 1 ? " is" : "s are"} still live: the daemon will refuse to archive ${cleanupCascade.live === 1 ? "its" : "their"} parent until ${cleanupCascade.live === 1 ? "it is" : "they are"} stopped.`
                : ""}{" "}
              Completed outputs are kept — each run stays viewable (read-only). This can't be
              undone.
            </>
          }
          confirmLabel="Cleanup"
          items={cleanupItems}
          run={(id) => cleanupRun(id)}
          onClose={() => setRunBulkKind(null)}
          onSettled={handleRunsSettled}
        />
      )}
      {runBulkKind === "retry" && (
        <BulkActionModal
          destructive
          runningLabel="Retrying"
          title={`Retry ${retryItems.length} run${retryItems.length === 1 ? "" : "s"}?`}
          description="This archives each selected run and starts a fresh run of the same pipeline. The archived runs stay viewable (read-only)."
          confirmLabel="Retry all"
          items={retryItems}
          run={(id) => retryAll(id).then(() => {})}
          onClose={() => setRunBulkKind(null)}
          onSettled={handleRunsSettled}
        />
      )}
      {runBulkKind === "pause" && (
        <BulkActionModal
          skipConfirm
          runningLabel="Pausing"
          title=""
          description=""
          confirmLabel="Pause"
          items={pauseItems}
          run={(id) => pauseRun(id)}
          onClose={() => setRunBulkKind(null)}
          onSettled={handleRunsSettled}
        />
      )}
    </>
  );
}
