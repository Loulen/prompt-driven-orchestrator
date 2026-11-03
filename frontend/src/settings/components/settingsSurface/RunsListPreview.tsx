import { createRef, useMemo } from "react";
import { AppearancePreviewContext } from "../../../hooks/useElementMarks";
import RunRow, { type RunRowPane } from "../../../run/components/runsList/RunRow";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import { runsListPreview } from "../../model/runsListPreview";

const nothing = () => {};

/** The Runs tab's state a preview row reads: nothing selected, nothing done. */
function previewPane(runs: RunListEntry[], expanded: ReadonlySet<string>): RunRowPane {
  return {
    isExpanded: (id: string) => expanded.has(id),
    setExpanded: nothing,
    runSel: new Set<string>(),
    renamingRunId: null,
    renameValue: "",
    setRenameValue: nothing,
    renameInputRef: createRef<HTMLInputElement>(),
    commitRename: async () => {},
    cancelRename: nothing,
    startRename: nothing,
    handlePause: async () => {},
    handleResume: async () => {},
    setConfirmRetryAll: nothing,
    setConfirmForget: nothing,
    setConfirmCleanup: nothing,
    runsScrollRef: createRef<HTMLDivElement>(),
    selectRange: nothing,
    toggleSel: nothing,
    visibleRunIds: [],
    runById: new Map(runs.map((run) => [run.run_id, run])),
    mayOnRow: () => false,
    filteredRuns: runs,
    setRunFilter: nothing,
    setAttributeFilter: nothing,
    setProjectFilter: nothing,
    runShell: { shell: null, open: async () => {}, close: nothing },
  };
}

/**
 * The preview of the Runs list block: the list's own rows (`RunRow`) on the most
 * recent Run the user sees and a parent with its child, or on a sample Run
 * without extension marks, at the list's width. Inert: nothing in it reacts.
 * `pointed` is the element whose switch the pointer or the focus is on.
 */
export default function RunsListPreview({
  runs,
  triggers,
  pointed,
}: {
  runs: RunListEntry[];
  triggers: TriggerListEntry[];
  pointed: string | null;
}) {
  const shown = useMemo(() => runsListPreview(runs), [runs]);
  const pane = useMemo(() => {
    const parents = new Set(shown.nodes.filter((node) => node.depth === 0 && node.children.length > 0).map((n) => n.run.run_id));
    return previewPane(runs, parents);
  }, [runs, shown]);
  const context = useMemo(() => ({ pointed, sample: shown.sample }), [pointed, shown.sample]);
  return (
    <AppearancePreviewContext.Provider value={context}>
      <figure className="set-preview" data-testid="appearance-preview-runs-list" data-sample={shown.sample || undefined}>
        <div className="lp-pane set-preview__list" inert>
          <div className="lp-rows">
            {shown.nodes.map((node) => (
              <RunRow
                key={node.run.run_id}
                node={node}
                pane={pane}
                selectedRunId={null}
                triggers={triggers}
                onSelectRun={nothing}
                onShowTriggers={nothing}
                sampleCost={shown.sampleCost}
              />
            ))}
          </div>
        </div>
        <figcaption className="set-preview__caption">{shown.caption}</figcaption>
      </figure>
    </AppearancePreviewContext.Provider>
  );
}
