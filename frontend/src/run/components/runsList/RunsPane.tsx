import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import ExtensionListFilters from "../../../shared/extensions/ExtensionListFilters";
import RunFilters from "../RunFilters";
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

/** The Runs tab of the left panel: header, filters, run tree and bulk bar. */
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
  const { runFilter, setRunFilter, allParentIds, allExpanded, setAllExpanded } = pane;
  const { attributeFilter, setAttributeFilter, projectFilter, setProjectFilter } = pane;
  return (
    <div className="flex min-h-0 flex-1 flex-col" role="tabpanel">
      <RunsPaneHeader pane={pane} onNewRun={onNewRun} />
      {runs.length > 0 && (
        <RunFilters
          runs={runs}
          triggers={triggers}
          value={runFilter}
          onChange={setRunFilter}
          treeToggle={
            allParentIds.length > 0
              ? { allExpanded, onToggle: () => setAllExpanded(!allExpanded) }
              : null
          }
        />
      )}
      <ExtensionListFilters filter={attributeFilter} onChange={setAttributeFilter} />
      <ExtensionListFilters
        location="projects/filter"
        filter={projectFilter}
        onChange={setProjectFilter}
      />
      <RunTreeList
        pane={pane}
        runs={runs}
        triggers={triggers}
        selectedRunId={selectedRunId}
        onSelectRun={onSelectRun}
        onSelectTrigger={onSelectTrigger}
        onShowTriggers={onShowTriggers}
      />
      <RunsBulkActions pane={pane} />
    </div>
  );
}
