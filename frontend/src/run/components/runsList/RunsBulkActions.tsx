import { Pause, RotateCcw, Trash2 } from "lucide-react";
import { cleanupRun, pauseRun, retryAll } from "../../api/commands";
import BulkActionBar from "../../../components/BulkActionBar";
import BulkActionModal from "../../../components/BulkActionModal";
import { cleanupTitle } from "./runsListTables";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";

/** The Runs tab's bulk-action bar and the confirm / progress modal it opens. */
export default function RunsBulkActions({ pane }: { pane: RunsPaneState }) {
  const {
    runSelIds, runningWillStop, cleanupItems, retryItems, pauseItems, setRunBulkKind, clearSel,
    runBulkKind, cleanupCascade, handleRunsSettled,
  } = pane;
  return (
    <>
      {/* #577 — floating bulk-action bar for the Runs tab. */}
      {runSelIds.length > 0 && (
        <BulkActionBar
          count={runSelIds.length}
          note={runningWillStop > 0 ? `${runningWillStop} running will stop` : null}
          actions={[
            {
              key: "cleanup",
              label: "Cleanup",
              icon: <Trash2 size={13} />,
              destructive: true,
              disabled: cleanupItems.length === 0,
              onClick: () => setRunBulkKind("cleanup"),
            },
            {
              key: "retry",
              label: "Retry",
              icon: <RotateCcw size={13} />,
              disabled: retryItems.length === 0,
              onClick: () => setRunBulkKind("retry"),
            },
            {
              key: "pause",
              label: "Pause",
              icon: <Pause size={13} />,
              disabled: pauseItems.length === 0,
              onClick: () => setRunBulkKind("pause"),
            },
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
