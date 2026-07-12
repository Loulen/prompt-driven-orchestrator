import { ChevronDown, X } from "lucide-react";
import type { RunListEntry, Trigger } from "../types";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "./ui/dropdown-menu";

/* #336 — client-side filters for the Runs list. Option sets are derived from
   the runs themselves (never from the live library/trigger stores) so runs of
   renamed or deleted pipelines/triggers stay filterable. The daemon already
   ships every axis on `GET /runs` (`effective_repo`, `pipeline_name`,
   `triggered_by`); filtering is a pure view-layer computation. */

/** Sentinel filter values for rows whose axis key is absent/empty. */
export const MANUAL_TRIGGER = "__manual__";
const NONE = "__none__";

export interface RunFilterValue {
  repo: string | null;
  pipeline: string | null;
  /** A trigger id, or MANUAL_TRIGGER for manually launched runs. */
  trigger: string | null;
}

export const EMPTY_RUN_FILTER: RunFilterValue = {
  repo: null,
  pipeline: null,
  trigger: null,
};

/** The filter key for a run on each axis (empty/missing values bucket to a sentinel). */
function repoKey(r: RunListEntry): string {
  return r.effective_repo && r.effective_repo.length > 0 ? r.effective_repo : NONE;
}
function pipelineKey(r: RunListEntry): string {
  const name = r.pipeline_name ?? "";
  return name.trim().length > 0 ? name : NONE;
}
function triggerKey(r: RunListEntry): string {
  return r.triggered_by ?? MANUAL_TRIGGER;
}

/** AND predicate over the three axes; a null axis means "All". */
export function runMatchesFilter(r: RunListEntry, f: RunFilterValue): boolean {
  if (f.repo !== null && repoKey(r) !== f.repo) return false;
  if (f.pipeline !== null && pipelineKey(r) !== f.pipeline) return false;
  if (f.trigger !== null && triggerKey(r) !== f.trigger) return false;
  return true;
}

function uniqueSorted(values: string[]): string[] {
  return [...new Set(values)].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

/** Basename of a repo path, for compact trigger-button / option labels. */
function repoLabel(path: string): string {
  if (path === NONE) return "(no project)";
  const segs = path.split("/").filter((s) => s.length > 0);
  return segs.length ? segs[segs.length - 1] : path;
}

interface Option {
  value: string;
  label: string;
}

function FilterDropdown({
  axis,
  placeholder,
  options,
  selected,
  onSelect,
}: {
  axis: "project" | "pipeline" | "trigger";
  placeholder: string;
  options: Option[];
  selected: string | null;
  onSelect: (v: string | null) => void;
}) {
  const selectedLabel =
    selected !== null ? options.find((o) => o.value === selected)?.label ?? selected : null;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        data-testid={`run-filter-${axis}`}
        title={placeholder}
        className={`flex min-w-0 flex-1 cursor-pointer items-center justify-between gap-1 rounded border px-1.5 py-0.5 text-left outline-none transition-colors hover:bg-bg-4 data-[popup-open]:border-acc ${
          selected !== null
            ? "border-acc bg-bg-3 text-acc"
            : "border-line-strong bg-bg-3 text-fg-4"
        }`}
        style={{ fontSize: "10px" }}
      >
        <span className="truncate">{selectedLabel ?? placeholder}</span>
        <ChevronDown size={9} className="shrink-0" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        className="min-w-[160px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
        side="bottom"
        align="start"
      >
        <DropdownMenuItem
          data-testid={`run-filter-option-all`}
          className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4 ${
            selected === null ? "bg-bg-4" : ""
          }`}
          style={{ fontSize: "11px" }}
          onClick={() => onSelect(null)}
        >
          All
        </DropdownMenuItem>
        {options.map((o) => (
          <DropdownMenuItem
            key={o.value}
            data-testid={`run-filter-option-${o.value}`}
            className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4 ${
              selected === o.value ? "bg-bg-4" : ""
            }`}
            style={{ fontSize: "11px" }}
            title={o.value}
            onClick={() => onSelect(o.value)}
          >
            <span className="truncate">{o.label}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export default function RunFilters({
  runs,
  triggers,
  value,
  onChange,
}: {
  runs: RunListEntry[];
  triggers: Trigger[];
  value: RunFilterValue;
  onChange: (v: RunFilterValue) => void;
}) {
  const repoOptions: Option[] = uniqueSorted(runs.map(repoKey)).map((v) => ({
    value: v,
    label: repoLabel(v),
  }));
  const pipelineOptions: Option[] = uniqueSorted(runs.map(pipelineKey)).map((v) => ({
    value: v,
    label: v === NONE ? "(unnamed)" : v,
  }));
  // "Manual" is offered only when at least one manual run exists; trigger ids
  // resolve to names via the triggers prop, falling back to the raw id when the
  // trigger was deleted.
  const triggerOptions: Option[] = uniqueSorted(runs.map(triggerKey)).map((v) => ({
    value: v,
    label:
      v === MANUAL_TRIGGER
        ? "Manual"
        : triggers.find((t) => t.id === v)?.name ?? v,
  }));

  const anyActive = value.repo !== null || value.pipeline !== null || value.trigger !== null;

  return (
    <div className="flex shrink-0 items-center gap-1 border-b border-line px-2 py-1.5">
      <FilterDropdown
        axis="project"
        placeholder="Project"
        options={repoOptions}
        selected={value.repo}
        onSelect={(repo) => onChange({ ...value, repo })}
      />
      <FilterDropdown
        axis="pipeline"
        placeholder="Pipeline"
        options={pipelineOptions}
        selected={value.pipeline}
        onSelect={(pipeline) => onChange({ ...value, pipeline })}
      />
      <FilterDropdown
        axis="trigger"
        placeholder="Trigger"
        options={triggerOptions}
        selected={value.trigger}
        onSelect={(trigger) => onChange({ ...value, trigger })}
      />
      {anyActive && (
        <button
          data-testid="run-filter-clear"
          title="Clear filters"
          className="grid h-4 w-4 shrink-0 cursor-pointer place-items-center rounded text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg"
          onClick={() => onChange(EMPTY_RUN_FILTER)}
        >
          <X size={10} />
        </button>
      )}
    </div>
  );
}
