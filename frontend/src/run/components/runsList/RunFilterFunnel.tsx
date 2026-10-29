import { useMemo } from "react";
import { Funnel, X } from "lucide-react";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { IconButton, Popover, type Icon } from "../../../shared/ui/kit";
import { activeFilterCount, runFilterOptions } from "../../model/runFilter";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import RunFilterMenu from "./RunFilterMenu";

/** The funnel, filled while a filter is in force. */
const FilledFunnel: Icon = (p) => <Funnel {...p} fill="currentColor" />;

/**
 * CORE-110 — the Runs header's funnel: it opens the filter menu, fills and
 * counts the filters in force, and a cross beside it clears them all.
 */
export default function RunFilterFunnel({
  pane,
  runs,
  triggers,
}: {
  pane: RunsPaneState;
  runs: RunListEntry[];
  triggers: { id: string; name: string }[];
}) {
  const { runFilter, setRunFilter, attributeFilter, setAttributeFilter, projectFilter, setProjectFilter, clearFilters } =
    pane;
  const options = useMemo(
    () => runFilterOptions(runs, triggers, pane.projectOfRun),
    [runs, triggers, pane.projectOfRun],
  );
  const count = activeFilterCount(runFilter, attributeFilter, projectFilter);
  const label = count > 0 ? `Filters · ${count} active` : "Filter";
  return (
    <>
      <Popover
        label="Filter runs"
        align="end"
        className="sh-rf-pop"
        trigger={({ open, toggle }) => (
          <IconButton
            icon={count > 0 ? FilledFunnel : Funnel}
            label={label}
            badge={count}
            aria-expanded={open}
            aria-haspopup="dialog"
            className="sh-rf-funnel"
            data-on={count > 0 || undefined}
            data-testid="run-filter-funnel"
            onClick={toggle}
          />
        )}
      >
        <RunFilterMenu
          options={options}
          value={runFilter}
          onChange={setRunFilter}
          attributeFilter={attributeFilter}
          onAttributeFilterChange={setAttributeFilter}
          projectFilter={projectFilter}
          onProjectFilterChange={setProjectFilter}
          count={count}
          onClearAll={clearFilters}
        />
      </Popover>
      {count > 0 && (
        <IconButton icon={X} size="sm" label="Clear filters" data-testid="run-filter-clear" onClick={clearFilters} />
      )}
    </>
  );
}
