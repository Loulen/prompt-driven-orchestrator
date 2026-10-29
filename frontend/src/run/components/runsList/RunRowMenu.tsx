import { useMemo } from "react";
import { Copy, Pause, Pencil, Play, RotateCcw, SquareTerminal, Trash2 } from "lucide-react";
import { isLiveRun, isTerminalRun } from "../../model/runStatus";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { ExtensionMenuGroups } from "../../../shared/extensions/ExtensionActions";
import type { RunEntity } from "../../../shared/extensions/contract";
import { DropdownMenuSeparator } from "../../../shared/ui/dropdown-menu";
import RowMenu, { ROW_MENU_EXTENSION_ITEM, RowMenuItem } from "../../../shared/ui/RowMenu";
import { toast } from "../../../shared/ui/toast";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import { copyToClipboard } from "../../../shared/lib/clipboard";

/**
 * The row's « ⋯ » menu (#30): the row's verbs (those of its hover icons, plus
 * Copy run id), the extensions' `run/item/menu` groups, then the destructive
 * entry. Its context hands the row's Run over as `entity`.
 */
export default function RunRowMenu({
  run,
  pane,
  isRenaming,
  canPause,
  canResume,
  canRetryAll,
  canCleanup,
  canRename,
  canForget,
}: {
  run: RunListEntry;
  pane: RunsPaneState;
  isRenaming: boolean;
  canPause: boolean;
  canResume: boolean;
  canRetryAll: boolean;
  canCleanup: boolean;
  /** CORE-42: the caller may rename the Run. */
  canRename: boolean;
  /** CORE-42: an archived Run the caller may forget. */
  canForget: boolean;
}) {
  const { startRename, handlePause, handleResume, setConfirmRetryAll, setConfirmCleanup, setConfirmForget, runShell } =
    pane;
  const isArchived = run.status === "archived";
  const label = run.name || run.run_id;
  const extra = useMemo(() => ({ entity: runEntity(run) }), [run]);
  return (
    <RowMenu label={`More actions for ${label}`} testId="run-row-menu">
      {!isRenaming && canRename && (
        <RowMenuItem data-testid="run-menu-rename" onClick={() => startRename(run)}>
          <Pencil /> Rename
        </RowMenuItem>
      )}
      {canPause && (
        <RowMenuItem data-testid="run-menu-pause" onClick={() => handlePause(run.run_id)}>
          <Pause /> Pause
        </RowMenuItem>
      )}
      {canResume && (
        <RowMenuItem data-testid="run-menu-resume" onClick={() => handleResume(run.run_id)}>
          <Play /> Resume
        </RowMenuItem>
      )}
      {canRetryAll && (
        <RowMenuItem data-testid="run-menu-retry-all" onClick={() => setConfirmRetryAll(run.run_id)}>
          <RotateCcw /> Retry all
        </RowMenuItem>
      )}
      {isTerminalRun(run.status) && !isArchived && (
        <RowMenuItem data-testid="run-menu-open-shell" onClick={() => void runShell.open(run.run_id)}>
          <SquareTerminal /> Open shell
        </RowMenuItem>
      )}
      <RowMenuItem data-testid="run-menu-copy-id" onClick={() => copyRunId(run.run_id)}>
        <Copy /> Copy run id
      </RowMenuItem>
      <ExtensionMenuGroups location="run/item/menu" extra={extra} itemClassName={ROW_MENU_EXTENSION_ITEM} />
      {(canCleanup || canForget) && <DropdownMenuSeparator />}
      {canCleanup && (
        <RowMenuItem
          variant="destructive"
          data-testid="run-menu-cleanup"
          data-run-id={run.run_id}
          onClick={() => setConfirmCleanup({ runId: run.run_id, status: run.status })}
        >
          <Trash2 /> {isLiveRun(run.status) ? "Stop and archive" : "Cleanup"}
        </RowMenuItem>
      )}
      {canForget && (
        <RowMenuItem variant="destructive" data-testid="run-menu-forget" onClick={() => setConfirmForget(run.run_id)}>
          <Trash2 /> Forget
        </RowMenuItem>
      )}
    </RowMenu>
  );
}

/** A row's Run, as its menu's context hands it over. */
function runEntity(run: RunListEntry): RunEntity {
  return {
    kind: "run",
    id: run.run_id,
    name: run.name ?? null,
    pipeline: run.pipeline_name,
    status: run.status,
    attributes: run.attributes ?? {},
  };
}

function copyRunId(runId: string) {
  void copyToClipboard(runId).then((ok) =>
    ok ? toast("Run id copied") : toast("The clipboard is not available", "error"),
  );
}
