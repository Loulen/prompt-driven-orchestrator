import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import RunsBulkActions from "./RunsBulkActions";
import RunsPaneHeader from "./RunsPaneHeader";
import RunTreeList from "./RunTreeList";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";

interface Props {
  /** The tab's state, owned by the left panel (see `useRunsPane`). */
  pane: RunsPaneState;
  runs: RunListEntry[];
  triggers: TriggerListEntry[];
  selectedRunId: string | null;
  onSelectRun: (runId: string) => void;
  onNewRun: () => void;
  onSelectTrigger?: (triggerId: string) => void;
  /** Switch the left panel to its Triggers tab. */
  onShowTriggers: () => void;
}

/** The Runs tab of the left panel: header (with the filters' funnel), run tree and bulk bar. */
export default function RunsPane({
  pane,
  runs,
  triggers,
  selectedRunId,
  onSelectRun,
  onNewRun,
  onSelectTrigger,
  onShowTriggers,
}: Props) {
  return (
    <div className="lp-pane" role="tabpanel">
      <RunsPaneHeader pane={pane} runs={runs} triggers={triggers} onNewRun={onNewRun} />
      <RunTreeList
        pane={pane}
        runs={runs}
        triggers={triggers}
        selectedRunId={selectedRunId}
        onSelectRun={onSelectRun}
        onSelectTrigger={onSelectTrigger}
        onShowTriggers={onShowTriggers}
        onNewRun={onNewRun}
      />
      <RunsBulkActions pane={pane} />
    </div>
  );
}
