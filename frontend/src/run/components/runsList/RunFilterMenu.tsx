import type { ReactNode } from "react";
import { ArrowRight, Calendar, ExternalLink, FolderGit2, UserRound, Workflow, X, Zap } from "lucide-react";
import type { Attributes } from "../../../shared/extensions/attributes";
import ExtensionBoundary from "../../../shared/extensions/ExtensionBoundary";
import { useListFilters } from "../../../shared/extensions/listFilters";
import { Button, Segmented, Select, TextInput, type Icon, type SelectOption } from "../../../shared/ui/kit";
import { cx } from "../../../shared/ui/kit/util";
import {
  DATE_PRESETS,
  dateModeHint,
  dayString,
  rangeDays,
  type DateMode,
  type DatePreset,
  type DateRange,
  type RunDateFilter,
} from "../../model/runDateFilter";
import {
  ANY_SOURCE,
  ANY_TRIGGER,
  MANUAL_TRIGGER,
  type RunFilterOption,
  type RunFilterOptions,
  type RunFilterValue,
} from "../../model/runFilter";

/* CORE-110 — the Runs filters, in the popover the header's funnel opens: one
   row per filter (Project, Pipeline, Launched by, Date), then the extensions'
   list filters of the Runs (`runs/filter`) and of their Projet
   (`projects/filter`), then the count and « Clear all ». */

const ANY = "";
const CUSTOM = "custom";

const DATE_OPTIONS: SelectOption[] = [
  { value: ANY, label: "Any time" },
  ...DATE_PRESETS.map(({ preset, label }) => ({ value: preset, label })),
  { value: CUSTOM, label: "Custom range" },
];

const MODES: { value: DateMode; label: string; title: string }[] = [
  { value: "active", label: "Active", title: dateModeHint("active") },
  { value: "started", label: "Started", title: dateModeHint("started") },
];

function rangeKey(range: DateRange | null): string {
  if (range === null) return ANY;
  return "preset" in range ? range.preset : CUSTOM;
}

/** The range a choice of the Date select stands for; a custom range starts from the current one. */
function rangeOf(key: string, current: DateRange | null, now: Date): DateRange | null {
  if (key === ANY) return null;
  if (key === CUSTOM) {
    if (current && "from" in current) return current;
    return current ? rangeDays(current, now) : { from: dayString(now), to: dayString(now) };
  }
  return { preset: key as DatePreset };
}

function Row({ icon: RowIcon, label, testId, top, children }: {
  icon: Icon;
  label: string;
  testId: string;
  /** The control spans two lines: the label stays on the first. */
  top?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={cx("sh-rf__row", top && "sh-rf__row--top")} data-testid={testId}>
      <span className="sh-rf__label" title={label}>
        <RowIcon size={12} strokeWidth={1.5} aria-hidden />
        <span className="sh-rf__text">{label}</span>
      </span>
      <div className="sh-rf__ctl">{children}</div>
    </div>
  );
}

/** The icon of a « Launched by » choice that stands for a whole population. */
const LAUNCH_ICONS: Record<string, Icon> = {
  [MANUAL_TRIGGER]: UserRound,
  [ANY_TRIGGER]: Zap,
  [ANY_SOURCE]: ExternalLink,
};

function AxisSelect({ label, options, value, onChange, all = "All" }: {
  label: string;
  options: RunFilterOption[];
  value: string | null;
  onChange: (v: string | null) => void;
  /** The label of the choice that narrows nothing. */
  all?: string;
}) {
  return (
    <Select
      size="sm"
      className="sh-rf__sel"
      active={value !== null}
      label={label}
      placeholder={all}
      value={value ?? ANY}
      options={[
        { value: ANY, label: all },
        ...options.map((o) => ({ ...o, title: o.title ?? o.label, icon: LAUNCH_ICONS[o.value] })),
      ]}
      onChange={(v) => onChange(v === ANY ? null : v)}
    />
  );
}

function DateControl({ value, onChange }: { value: RunDateFilter; onChange: (v: RunDateFilter) => void }) {
  const now = new Date();
  const range = value.range;
  const days = range && "from" in range ? range : null;
  const setDay = (edge: "from" | "to", day: string) => {
    if (days && day.length > 0) onChange({ ...value, range: { ...days, [edge]: day } });
  };
  return (
    <div className="sh-rf__date">
      <div className="sh-rf__date-row">
        <Segmented
          size="sm"
          label="Date mode"
          items={MODES}
          value={value.mode}
          onChange={(mode) => onChange({ ...value, mode })}
        />
        <Select
          size="sm"
          className="sh-rf__sel"
          active={range !== null}
          label="Date"
          value={rangeKey(range)}
          options={DATE_OPTIONS}
          onChange={(key) => onChange({ ...value, range: rangeOf(key, range, now) })}
        />
      </div>
      {days && (
        <div className="sh-rf__date-row">
          <TextInput
            small
            mono
            type="date"
            aria-label="From"
            data-testid="run-filter-date-from"
            className="sh-rf__day"
            value={days.from}
            onChange={(e) => setDay("from", e.target.value)}
          />
          <ArrowRight size={12} strokeWidth={1.5} aria-hidden className="sh-rf__dim" />
          <TextInput
            small
            mono
            type="date"
            aria-label="To"
            data-testid="run-filter-date-to"
            className="sh-rf__day"
            value={days.to}
            onChange={(e) => setDay("to", e.target.value)}
          />
        </div>
      )}
    </div>
  );
}

type ListFilters = ReturnType<typeof useListFilters>;

/**
 * The rows of one list-filter location: each contribution's control on the
 * whole row, since it draws its own caption (its `label`); the tooltip names
 * the extension.
 */
function ExtensionRows({ filters: { resource, context, filters }, filter, onChange, scope }: {
  filters: ListFilters;
  filter: Attributes;
  onChange: (next: Attributes) => void;
  /** What the filter applies to, when it is not the Run itself. */
  scope?: string;
}) {
  return filters.map(({ key, extension, contribution, Component }) =>
    Component ? (
      <div
        key={key}
        className="sh-rf__row sh-rf__row--ext"
        title={[contribution.label, scope, extension.name].filter(Boolean).join(" · ")}
        data-testid={`run-filter-ext-${key}`}
      >
        <ExtensionBoundary extensionId={extension.id}>
          <Component
            extension={{ id: extension.id, name: extension.name }}
            context={context}
            resource={resource}
            label={contribution.label}
            filter={filter}
            setFilter={onChange}
          />
        </ExtensionBoundary>
      </div>
    ) : null,
  );
}

export default function RunFilterMenu({
  options,
  value,
  onChange,
  attributeFilter,
  onAttributeFilterChange,
  projectFilter,
  onProjectFilterChange,
  count,
  onClearAll,
}: {
  options: RunFilterOptions;
  value: RunFilterValue;
  onChange: (v: RunFilterValue) => void;
  /** The extensions' filter on the Runs' attributes (`runs/filter`). */
  attributeFilter: Attributes;
  onAttributeFilterChange: (next: Attributes) => void;
  /** The extensions' filter on their Projet's attributes (`projects/filter`). */
  projectFilter: Attributes;
  onProjectFilterChange: (next: Attributes) => void;
  /** The filters in force. */
  count: number;
  onClearAll: () => void;
}) {
  const runFilters = useListFilters("runs/filter", attributeFilter);
  const projectFilters = useListFilters("projects/filter", projectFilter);
  const anyExtension = [runFilters, projectFilters].some((l) => l.filters.some((f) => f.Component));
  return (
    <div className="sh-rf" data-testid="run-filter-menu">
      <Row icon={FolderGit2} label="Project" testId="run-filter-project">
        <AxisSelect
          label="Project"
          options={options.project}
          value={value.repo}
          onChange={(repo) => onChange({ ...value, repo })}
        />
      </Row>
      <Row icon={Workflow} label="Pipeline" testId="run-filter-pipeline">
        <AxisSelect
          label="Pipeline"
          options={options.pipeline}
          value={value.pipeline}
          onChange={(pipeline) => onChange({ ...value, pipeline })}
        />
      </Row>
      <Row icon={UserRound} label="Launched by" testId="run-filter-trigger">
        <AxisSelect
          label="Launched by"
          all="Anyone"
          options={options.trigger}
          value={value.trigger}
          onChange={(trigger) => onChange({ ...value, trigger })}
        />
      </Row>
      <Row icon={Calendar} label="Date" testId="run-filter-date" top>
        <DateControl value={value.date} onChange={(date) => onChange({ ...value, date })} />
      </Row>
      {anyExtension && <div className="sh-rf__rule" role="separator" />}
      <ExtensionRows filters={runFilters} filter={attributeFilter} onChange={onAttributeFilterChange} />
      <ExtensionRows
        filters={projectFilters}
        filter={projectFilter}
        onChange={onProjectFilterChange}
        scope="Project"
      />
      <div className="sh-rf__foot">
        <span className="d-meta" data-testid="run-filter-count">
          {count > 0 ? `${count} active` : "No filter"}
        </span>
        <Button size="sm" tone="ghost" icon={X} disabled={count === 0} onClick={onClearAll} data-testid="run-filter-clear-all">
          Clear all
        </Button>
      </div>
    </div>
  );
}
