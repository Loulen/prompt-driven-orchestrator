import { Fragment, useMemo, useState } from "react";
import { Check, ChevronDown, ChevronRight, Info } from "lucide-react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from "recharts";
import type {
  PerformanceEffortEntity,
  StatsCost,
  StatsCostAggregate,
  StatsCostEntity,
  StatsCostPeriod,
  StatsEffortCostEntity,
  StatsHarnessCost,
  StatsHarnessPerformance,
  StatsModelEffortPair,
  StatsOverview,
  StatsProvenance,
  StatsDistribution,
  StatsPerformance,
  StatsPerformanceAggregate,
  StatsPerformanceEntity,
  StatsProjectCostEntity,
  StatsAbsorbedMember,
  StatsSessionEntity,
  StatsSessionHarness,
  StatsSessionPeriod,
  StatsSteeredRate,
} from "../types";
import { formatCostAmount } from "../lib/costLabel";
import { harnessColor } from "../lib/harness";
import { cssColor } from "../lib/cssColor";
import { useTheme } from "../hooks/useTheme";
import { Tooltip, TooltipProvider } from "./ui/tooltip";
import SelectControl from "./SelectControl";
import { CombinedIcon } from "./StatsAbsorption";
import {
  usePipelineAbsorption,
  type MasterSelection,
} from "../hooks/usePipelineAbsorption";
import type { AbsorbableRow } from "../lib/statsAbsorption";
import {
  ALL_NODE_KINDS,
  DEFAULT_PERFORMANCE_BAND,
  DURATION_MODES,
  performanceBandDeviates,
  type DurationMode,
  type NodeKind,
  type PerformanceBand,
  type StatsAxis,
  type StatsZoom,
} from "../lib/statsFilters";

export type StatsTab = "runs" | "sessions" | "triggers" | "cost" | "performance";

/**
 * Chart colours (#759). Getters, not constants: recharts takes resolved colours
 * as props, so each read must happen at RENDER time against the live palette.
 * The call sites are unchanged — `CHART.grid` and `{...AXIS_PROPS}` now resolve
 * the token instead of returning a frozen hex. `StatsCharts` subscribes to the
 * theme so a switch re-renders the subtree and these are read again.
 */
const CHART = {
  get runs() {
    return cssColor("--color-chart-runs", "#58a6ff");
  },
  get errors() {
    return cssColor("--color-chart-errors", "#f85149");
  },
  get fires() {
    return cssColor("--color-chart-fires", "#3fb950");
  },
  get grid() {
    return cssColor("--color-chart-grid", "#30363d");
  },
  get axis() {
    return cssColor("--color-chart-axis", "#8b949e");
  },
  get tooltipBg() {
    return cssColor("--color-chart-tooltip-bg", "#161b22");
  },
};

const AXIS_PROPS = {
  get stroke() {
    return CHART.axis;
  },
  get tick() {
    return { fill: CHART.axis, fontSize: 10 };
  },
};

function ChartFrame({ children }: { children: React.ReactElement }) {
  return (
    <div style={{ width: "100%", height: 220 }}>
      <ResponsiveContainer width="100%" height="100%">
        {children}
      </ResponsiveContainer>
    </div>
  );
}

function EmptyNote({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-1 py-8 text-center text-fg-4" style={{ fontSize: "11.5px" }}>
      {children}
    </div>
  );
}

/** Which Runs the figures on screen describe (#810). Always visible, never a
 *  silent filter: amber once « completed runs only » narrows the cohort, muted
 *  otherwise, and explicit about what was left out. */
function CohortLine({
  completedOnly,
  note,
}: {
  completedOnly: boolean;
  /** An extra clause for a tab the cohort does not govern. */
  note?: string;
}) {
  return (
    <div
      data-testid="stats-cohort-line"
      className={completedOnly ? "text-st-await" : "text-fg-4"}
      style={{ fontSize: "10.5px" }}
    >
      Cohort: runs started in the period
      {completedOnly ? (
        <>
          {" · "}
          <span className="font-medium">completed runs only</span> (failed,
          stopped and running runs left out)
        </>
      ) : null}
      {note ? ` · ${note}` : ""}
    </div>
  );
}

function HarnessLegend({ harnesses }: { harnesses: string[] }) {
  return (
    <div className="flex flex-wrap gap-3 text-fg-3" style={{ fontSize: "10.5px" }}>
      {harnesses.map((harness) => (
        <span key={harness} className="flex items-center gap-1.5">
          <span
            className="h-2 w-2 rounded-full"
            style={{ backgroundColor: harnessColor(harness) }}
            data-testid={`stats-harness-legend-${harness}`}
          />
          {harness}
        </span>
      ))}
    </div>
  );
}

function flattenPeriods(
  periods: (StatsSessionPeriod | StatsCostPeriod)[],
  value: "executions" | "usd",
) {
  return periods.map((period) => ({
    bucket: period.bucket,
    ...Object.fromEntries(
      period.harnesses.map((harness) => [
        harness.harness,
        value === "usd" ? ("usd" in harness ? harness.usd : null) : harness.executions,
      ]),
    ),
  }));
}

function isHarnessCost(
  metric: StatsSessionHarness | StatsHarnessCost | undefined,
): metric is StatsHarnessCost {
  return metric !== undefined && "average_usd" in metric;
}

function HarnessBars({
  periods,
  harnesses,
  value,
}: {
  periods: (StatsSessionPeriod | StatsCostPeriod)[];
  harnesses: string[];
  value: "executions" | "usd";
}) {
  if (periods.length === 0) return <EmptyNote>No activity in this period.</EmptyNote>;
  return (
    <ChartFrame>
      <BarChart data={flattenPeriods(periods, value)} margin={{ top: 8, right: 8, left: -12 }}>
        <CartesianGrid stroke={CHART.grid} strokeDasharray="3 3" vertical={false} />
        <XAxis dataKey="bucket" {...AXIS_PROPS} />
        <YAxis allowDecimals={value === "usd"} {...AXIS_PROPS} />
        <RTooltip
          contentStyle={{ background: CHART.tooltipBg, border: `1px solid ${CHART.grid}`, fontSize: 11 }}
          formatter={(raw, name) => {
            if (raw == null) return ["—", String(name)];
            const amount = typeof raw === "number" ? raw : Number(raw);
            const metric = periods
              .flatMap((period) => period.harnesses)
              .find((harness) => harness.harness === String(name));
            const costMetric = isHarnessCost(metric) ? metric : undefined;
            const label =
              value === "usd"
                ? formatCostAmount(
                    amount,
                    costMetric?.partial ?? false,
                    costMetric?.estimated ?? true,
                  )
                : amount;
            return [label, String(name)];
          }}
        />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        {harnesses.map((harness) => (
          <Bar
            key={harness}
            dataKey={harness}
            name={harness}
            stackId="harness"
            fill={harnessColor(harness)}
          />
        ))}
      </BarChart>
    </ChartFrame>
  );
}

function RunsTab({
  overview,
  completedOnly,
}: {
  overview: StatsOverview;
  completedOnly: boolean;
}) {
  if (overview.buckets.length === 0)
    return <EmptyNote>No runs in this period.</EmptyNote>;
  const runs = new Map(overview.runs.map((row) => [row.bucket, row.count]));
  const errors = new Map(overview.errors.map((row) => [row.bucket, row.count]));
  const data = overview.buckets.map((bucket) => ({
    bucket,
    runs: runs.get(bucket) ?? 0,
    errors: errors.get(bucket) ?? 0,
  }));
  const totalErrors = overview.errors.reduce((sum, row) => sum + row.count, 0);
  return (
    <div data-testid="stats-chart-runs">
      {completedOnly && (
        // #810: the card stays, at zero — nothing is masked. A completed Run
        // carries no `run_failed` of its own, so the series is 0 by
        // construction; a Run that failed, was reopened and then completed is
        // the one shape that can still count.
        <TooltipProvider>
          <div className="mb-3" style={{ fontSize: "11px" }}>
            <Tooltip
              content="0 by construction: a completed run has no run_failed of its own."
              side="top"
            >
              <span
                className="rounded bg-bg-3 px-2 py-1 text-fg-2"
                data-testid="stats-kpi-errors-completed-only"
              >
                Errors: <span className="font-mono text-fg">{totalErrors}</span>{" "}
                (completed only)
              </span>
            </Tooltip>
          </div>
        </TooltipProvider>
      )}
      <ChartFrame>
        <BarChart data={data} margin={{ top: 8, right: 8, left: -18 }}>
          <CartesianGrid stroke={CHART.grid} strokeDasharray="3 3" vertical={false} />
          <XAxis dataKey="bucket" {...AXIS_PROPS} />
          <YAxis allowDecimals={false} {...AXIS_PROPS} />
          <RTooltip
            contentStyle={{ background: CHART.tooltipBg, border: `1px solid ${CHART.grid}`, fontSize: 11 }}
          />
          <Legend wrapperStyle={{ fontSize: 11 }} />
          <Bar dataKey="runs" name="Runs" fill={CHART.runs} />
          <Bar dataKey="errors" name="Errors (failed)" fill={CHART.errors} />
        </BarChart>
      </ChartFrame>
    </div>
  );
}

function MasterList<T extends AbsorbableRow>({
  rows,
  selected,
  valueLabel,
  onSelect,
  ariaLabel = "Spenders",
  monoName = false,
  selection,
}: {
  rows: T[];
  selected: string | null;
  /** A node, not a string: a partially filtered Performance row renders an
   *  italic « filtered » here instead of a number (#810). */
  valueLabel: (row: T) => React.ReactNode;
  onSelect: (id: string | null) => void;
  ariaLabel?: string;
  /** Model ids are ids — render them mono (ADR-0065 §2). */
  monoName?: boolean;
  /** The Pipeline multi-select of the absorption (#890). A plain click still
   *  opens the detail; the ring, Ctrl/Cmd-click, Shift-click and Space select.
   *  Absent on axes whose rows are not Pipelines. */
  selection?: MasterSelection;
}) {
  const options = [{ id: "__total__", name: "Total" } as T, ...rows];
  const order = rows.map((row) => row.id);
  const selectedIndex = Math.max(
    0,
    options.findIndex((row) => (selected === null ? row.id === "__total__" : row.id === selected)),
  );
  const [focusIndex, setFocusIndex] = useState(selectedIndex);
  const activeFocusIndex = Math.min(focusIndex, options.length - 1);

  return (
    <div
      role="listbox"
      aria-label={ariaLabel}
      className="flex flex-col gap-1"
      onKeyDown={(event) => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const delta = event.key === "ArrowDown" ? 1 : -1;
          setFocusIndex((activeFocusIndex + delta + options.length) % options.length);
        } else if (event.key === "Enter") {
          event.preventDefault();
          const row = options[activeFocusIndex];
          onSelect(row.id === "__total__" ? null : row.id);
        } else if (event.key === "Backspace" || event.key === "ArrowLeft") {
          event.preventDefault();
          onSelect(null);
        } else if (event.key === " " && selection) {
          event.preventDefault();
          const row = options[activeFocusIndex];
          if (selection.isSelectable(row.id)) selection.toggle(row.id, false);
        }
      }}
    >
      {options.map((row, index) => {
        const isSelected = row.id === (selected ?? "__total__");
        const isTotal = row.id === "__total__";
        const selectable = selection?.isSelectable(row.id) ?? false;
        const checked = selectable && (selection?.selected.has(row.id) ?? false);
        const absorbed = row.absorbed?.length ?? 0;
        return (
          <button
            key={row.id}
            type="button"
            role="option"
            aria-selected={isSelected}
            data-checked={selectable ? checked : undefined}
            tabIndex={index === activeFocusIndex ? 0 : -1}
            onFocus={() => setFocusIndex(index)}
            onClick={(event) => {
              if (selection && selectable && (event.ctrlKey || event.metaKey || event.shiftKey)) {
                selection.toggle(row.id, event.shiftKey, order);
                return;
              }
              onSelect(isTotal ? null : row.id);
            }}
            className={`group flex items-center justify-between gap-3 rounded px-2 py-2 text-left ${
              checked
                ? "bg-acc-bg text-fg"
                : isSelected
                  ? "bg-bg-5 text-fg"
                  : "text-fg-3 hover:bg-bg-3"
            } ${selection?.flashId === row.id ? "ring-1 ring-acc" : ""}`}
            style={{ fontSize: "11.5px" }}
          >
            <span className="flex min-w-0 items-center gap-2">
              {selection &&
                (selectable ? (
                  <SelectControl
                    selected={checked}
                    dotClass={null}
                    label={`Select ${row.name}`}
                    testId="stats-row-select"
                    onSelect={(event) => selection.toggle(row.id, event.shiftKey, order)}
                  />
                ) : (
                  <span className="w-4 shrink-0" aria-hidden="true" />
                ))}
              <span className={`truncate ${monoName ? "font-mono" : ""}`}>{row.name}</span>
              {selection && absorbed > 0 && (
                <CombinedIcon
                  count={absorbed}
                  onOpen={() => selection.onOpenMembers(row.id)}
                />
              )}
            </span>
            <span className="shrink-0 font-mono text-fg-2">
              {isTotal ? "" : valueLabel(row)}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/** Sessions counts a Pipeline in executions, like its column. */
const SESSIONS_COUNT = {
  of: (row: StatsSessionEntity) => row.executions,
  ofMember: (member: StatsAbsorbedMember) => member.executions ?? member.runs,
  word: "execution" as const,
};

/** Cost and Performance count a Pipeline in Runs. */
const RUNS_COUNT = {
  of: (row: AbsorbableRow) => row.runs ?? 0,
  ofMember: (member: StatsAbsorbedMember) => member.runs,
  word: "run" as const,
};

function SessionsTab({
  overview,
  onAbsorptionsChanged,
}: {
  overview: StatsOverview;
  onAbsorptionsChanged: () => void;
}) {
  const rows = useMemo(
    () => [...overview.sessions_by_pipeline].sort((a, b) => b.executions - a.executions),
    [overview.sessions_by_pipeline],
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = rows.find((row) => row.id === selectedId) ?? null;
  const periods = selected ? selected.by_period : overview.sessions_by_period;
  const detailRows = selected?.nodes ?? rows;
  const absorption = usePipelineAbsorption({
    rows,
    enabled: true,
    count: SESSIONS_COUNT,
    onChanged: onAbsorptionsChanged,
  });

  return (
    <div className="flex flex-col gap-4" data-testid="stats-chart-sessions">
      <HarnessLegend harnesses={overview.session_harnesses} />
      <HarnessBars periods={periods} harnesses={overview.session_harnesses} value="executions" />
      <div className="flex min-h-[220px] gap-4">
        <div className="min-w-[250px] border-r border-line pr-3">
          <MasterList
            rows={rows}
            selected={selectedId}
            valueLabel={(row) => String(row.executions)}
            onSelect={setSelectedId}
            selection={absorption.selection}
          />
        </div>
        <div className="min-w-0 flex-1">
          <div className="mb-3 text-fg-4" style={{ fontSize: "10.5px" }}>
            Total{selected ? ` / ${selected.name}` : ""}
          </div>
          <SessionTable
            rows={detailRows}
            harnesses={overview.session_harnesses}
            onOpenMembers={selected ? undefined : absorption.openMembers}
          />
        </div>
      </div>
      {absorption.overlay}
    </div>
  );
}

/** A Pipeline row's name, with the `[⧉ N]` icon when it absorbs others. */
function PipelineName({
  row,
  onOpenMembers,
}: {
  row: AbsorbableRow;
  onOpenMembers?: (id: string) => void;
}) {
  const absorbed = row.absorbed?.length ?? 0;
  if (!onOpenMembers || absorbed === 0) return <>{row.name}</>;
  return (
    <span className="inline-flex items-center gap-1.5">
      {row.name}
      <CombinedIcon count={absorbed} onOpen={() => onOpenMembers(row.id)} />
    </span>
  );
}

function SessionTable({
  rows,
  harnesses,
  onOpenMembers,
}: {
  rows: StatsSessionEntity[];
  harnesses: string[];
  /** Pipeline rows at Total level: the combined icon opens the members. */
  onOpenMembers?: (id: string) => void;
}) {
  return (
    <table className="w-full table-fixed text-left" style={{ fontSize: "11px" }}>
      <thead className="text-fg-4">
        <tr>
          <th className="pb-2 font-medium">Name</th>
          <th className="w-20 pb-2 text-right font-medium">Total</th>
          {harnesses.map((harness) => (
            <th key={harness} className="w-24 pb-2 text-right font-medium">
              <span className="inline-flex items-center gap-1">
                <span
                  className="h-2 w-2 rounded-full"
                  style={{ backgroundColor: harnessColor(harness) }}
                />
                {harness}
              </span>
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.id} className="border-t border-line text-fg-2" tabIndex={0}>
            <td className="py-2 pr-2">
              <PipelineName row={row} onOpenMembers={onOpenMembers} />
            </td>
            <td className="py-2 text-right font-mono">{row.executions}</td>
            {harnesses.map((harness) => (
              <td
                key={harness}
                className="py-2 text-right font-mono"
                data-harness={harness}
              >
                {row.harnesses.find((item) => item.harness === harness)?.executions ?? "—"}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function TriggersTab({ overview }: { overview: StatsOverview }) {
  const kpi = overview.triggers_created_runs;
  return (
    <div data-testid="stats-chart-triggers" className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2" style={{ fontSize: "11px" }}>
        <span className="rounded bg-bg-3 px-2 py-1 text-fg-2" data-testid="stats-kpi-created-runs">
          Fires that created a run: <span className="font-mono text-fg">{kpi.fired}</span>
        </span>
        <span className="rounded bg-bg-3 px-2 py-1 text-fg-2" data-testid="stats-kpi-distinct">
          <span className="font-mono text-fg">{kpi.distinct_triggers}</span> of{" "}
          <span className="font-mono text-fg">{kpi.enabled_triggers}</span> enabled triggers fired
        </span>
      </div>
      {overview.fires_by_pipeline.length === 0 ? (
        <EmptyNote>No trigger fires in this period.</EmptyNote>
      ) : (
        <ChartFrame>
          <BarChart data={overview.fires_by_pipeline}>
            <CartesianGrid stroke={CHART.grid} strokeDasharray="3 3" vertical={false} />
            <XAxis dataKey="pipeline_id" {...AXIS_PROPS} />
            <YAxis allowDecimals={false} {...AXIS_PROPS} />
            <RTooltip
              contentStyle={{
                background: CHART.tooltipBg,
                border: `1px solid ${CHART.grid}`,
                fontSize: 11,
              }}
            />
            <Bar dataKey="count" name="Fires" fill={CHART.fires} />
          </BarChart>
        </ChartFrame>
      )}
    </div>
  );
}

function coverage(metric: StatsHarnessCost, unit: "Run" | "execution"): string {
  const parts = [
    `${metric.readable} readable ${metric.readable === 1 ? "cost" : "costs"} of ${metric.executions} ${
      metric.executions === 1 ? unit : `${unit}s`
    }`,
  ];
  if (metric.unpriced_models.length) {
    parts.push(`Lower bound; unpriced: ${metric.unpriced_models.join(", ")}`);
  }
  if (metric.missing_reasons.length) parts.push(metric.missing_reasons.join("; "));
  return parts.join(". ");
}

function HarnessCards({ aggregate }: { aggregate: StatsCostAggregate }) {
  return (
    <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
      {aggregate.harnesses.map((metric) => (
        <div
          key={metric.harness}
          className="rounded-md border border-line bg-bg-3 p-3"
          data-testid={`stats-harness-card-${metric.harness}`}
        >
          <div className="mb-2 flex items-center gap-1.5 text-fg-3" style={{ fontSize: "10.5px" }}>
            <span
              className="h-2 w-2 rounded-full"
              style={{ backgroundColor: harnessColor(metric.harness) }}
            />
            {metric.harness}
          </div>
          <div className="font-mono text-fg" style={{ fontSize: "15px" }}>
            {formatCostAmount(metric.usd, metric.partial, metric.estimated)}
          </div>
          <div className="mt-1 text-fg-4" style={{ fontSize: "10px" }}>
            {metric.median_usd === null
              ? "— median"
              : `${formatCostAmount(metric.median_usd, metric.partial, metric.estimated)} median`}
          </div>
        </div>
      ))}
    </div>
  );
}

function CostCell({
  metric,
  unit,
}: {
  metric: StatsHarnessCost | undefined;
  unit: "Run" | "execution";
}) {
  if (!metric) return <span className="font-mono text-fg-4">—</span>;
  const detail = coverage(metric, unit);
  return (
    <div className="flex flex-col items-end font-mono">
      <span>{formatCostAmount(metric.usd, metric.partial, metric.estimated)}</span>
      <Tooltip content={detail} side="top">
        <button
          type="button"
          aria-label={detail}
          className="text-fg-4 underline decoration-dotted underline-offset-2"
          style={{ fontSize: "9.5px" }}
        >
          {metric.median_usd === null
            ? "— median"
            : `${formatCostAmount(metric.median_usd, metric.partial, metric.estimated)} median`}
        </button>
      </Tooltip>
    </div>
  );
}

/** Where a requested value was read from — the « ? » tooltip copy (ADR-0065 §1).
 *  Observed values are the norm and carry nothing; the vocabulary never says
 *  "real". */
const PROVENANCE_COPY: Record<Exclude<StatsProvenance, "observed">, string> = {
  requested: "requested at node startup, not observed in transcripts",
  mixed: "partly requested at node startup, not observed in every transcript",
};

/** Hovering the italic "not set" effort says why the bucket exists. */
const NOT_SET_COPY = "no effort requested at node startup nor observed in transcripts";

/** The Performance « By model » tooltip (#737): the wire carries the provenance
 *  but not the per-harness source lines Cost shows, so the copy is the axis's
 *  own honest summary (ADR-0065 §1). */
function performanceProvenanceCopy(provenance: StatsProvenance | null | undefined): string {
  if (!provenance || provenance === "observed") {
    return "observed — the harness's source named the value";
  }
  return PROVENANCE_COPY[provenance];
}

function ProvenanceMark({
  provenance,
  target,
}: {
  provenance: Exclude<StatsProvenance, "observed">;
  target: "model" | "effort";
}) {
  const copy = PROVENANCE_COPY[provenance];
  // A plain span, not a button: the mark often lands inside the row-name drill
  // button (« Open claude-… »), and interactive-inside-interactive is invalid
  // DOM (FP #737 finding). The tooltip is a description, not a control — same
  // voice as the « not set » bucket's italic word.
  return (
    <Tooltip content={copy} side="top">
      <span
        role="img"
        aria-label={copy}
        data-testid={`stats-provenance-${target}`}
        className="ml-0.5 align-super text-fg-4"
        style={{ fontSize: "8.5px" }}
      >
        ?
      </span>
    </Tooltip>
  );
}

// Per-harness source of a model/effort value (#736): the wire carries
// `provenance` (+ `effort_provenance`) and `provider` on StatsHarnessCost.

const OBSERVED_HOW: Record<"model" | "effort", Record<string, string>> = {
  model: {
    claude: "observed — each message in the transcript",
    pi: "observed — each message in the session",
    copilot: "observed — session open, then each usage point",
  },
  effort: {
    pi: "observed — thinking-level change event",
    copilot: "observed — reasoning effort at session open",
  },
};

function sourceLines(harnesses: StatsHarnessCost[], target: "model" | "effort"): string[] {
  return harnesses
    .filter((h) => h.usd !== null || h.executions > 0)
    .map((h) => {
      const prov = target === "model" ? h.provenance : h.effort_provenance;
      if (!prov) return null;
      const how =
        prov === "observed"
          ? (OBSERVED_HOW[target][h.harness] ?? "observed — harness source")
          : prov === "requested"
            ? "requested — node startup event, the source is silent"
            : "mixed — requested in some executions, observed in others";
      const via = target === "model" && h.provider ? ` · via ${h.provider}` : "";
      return `${h.harness}: ${how}${via}`;
    })
    .filter((line): line is string => line !== null);
}

/** The hoverable name of a model or effort: the value itself is the trigger, the
 *  tooltip says, harness by harness, where it was read and (models) the provider.
 *  The « ? » superscript stays as the at-a-glance mark for a requested/mixed value. */
function ProvenanceName({
  name,
  mono,
  provenance,
  harnesses,
  target,
  content,
}: {
  name: React.ReactNode;
  mono?: boolean;
  provenance: StatsProvenance | null | undefined;
  harnesses: StatsHarnessCost[];
  target: "model" | "effort";
  /** Overrides the hover copy — the Performance axis carries provenance but no
   *  per-harness source lines (#737). */
  content?: string;
}) {
  const lines = sourceLines(harnesses, target);
  const tooltip = content ?? (lines.length ? lines.join("\n") : (provenance ?? "observed"));
  return (
    <span className="inline-flex items-baseline gap-0.5">
      <Tooltip content={tooltip} side="top">
        <span
          className={`${mono ? "font-mono" : ""} cursor-help whitespace-pre-line underline decoration-dotted decoration-transparent underline-offset-2 hover:decoration-fg-4`}
          data-testid={`stats-${target}-name`}
        >
          {name}
        </span>
      </Tooltip>
      {provenance && provenance !== "observed" && (
        <ProvenanceMark provenance={provenance} target={target} />
      )}
    </span>
  );
}

function Breadcrumb({
  crumbs,
  testid = "stats-cost-breadcrumb",
}: {
  crumbs: { label: string; onClick?: () => void }[];
  /** The Performance « By model » axis reuses the same breadcrumb under its own
   *  test id (#737). */
  testid?: string;
}) {
  return (
    <div
      className="mb-3 text-fg-4"
      style={{ fontSize: "10.5px" }}
      data-testid={testid}
    >
      {crumbs.map((crumb, index) => (
        <span key={`${crumb.label}-${index}`}>
          {index > 0 ? " / " : ""}
          {crumb.onClick ? (
            <button
              type="button"
              aria-label={`Back to ${crumb.label}`}
              onClick={crumb.onClick}
              className="hover:text-fg-2 underline decoration-dotted underline-offset-2"
            >
              {crumb.label}
            </button>
          ) : (
            <span className="text-fg-3">{crumb.label}</span>
          )}
        </span>
      ))}
    </div>
  );
}

function pairMetric(pair: StatsModelEffortPair): StatsHarnessCost {
  return {
    harness: "total",
    usd: pair.usd,
    estimated: pair.estimated,
    partial: pair.partial,
    executions: pair.executions,
    readable: pair.readable,
    unknown: pair.unknown,
    average_usd: pair.average_usd,
    median_usd: pair.median_usd,
    unpriced_models: pair.unpriced_models,
    missing_reasons: pair.missing_reasons,
  };
}

function effortNameCell(row: StatsEffortCostEntity): React.ReactNode {
  if (row.effort === null) {
    // The "not set" bucket: hovering the italic word explains it (ADR-0065 §1).
    // A plain span — the row-name button around it stays the single interactive
    // element, and the tooltip is a description, not a second control.
    return (
      <Tooltip content={NOT_SET_COPY} side="top">
        <span className="italic text-fg-4">not set</span>
      </Tooltip>
    );
  }
  return (
    <ProvenanceName
      name={row.name}
      provenance={row.provenance}
      harnesses={row.harnesses}
      target="effort"
    />
  );
}

function CostTable({
  rows,
  harnesses,
  unit,
  onOpen,
  renderName,
  expandablePairs = false,
}: {
  rows: StatsCostEntity[];
  harnesses: string[];
  unit: "Run" | "execution";
  onOpen?: (row: StatsCostEntity) => void;
  /** Replaces the default (button-or-plain) name cell — the model axis marks
   *  provenance and renders "not set" in its own voice. */
  renderName?: (row: StatsCostEntity) => React.ReactNode;
  /** Node rows gain a chevron unfolding their model × effort pairs (ADR-0065).
   *  Off on the model axis, where the model × effort path is already the drill. */
  expandablePairs?: boolean;
}) {
  const sorted = [...rows].sort((a, b) => (b.usd ?? -1) - (a.usd ?? -1));
  // Expansion state lives HERE and dies with the table: the parent keys the
  // table on the drill path, so a stale expanded set never leaks across Nodes.
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  return (
    <TooltipProvider>
      <table className="w-full table-fixed text-left" style={{ fontSize: "11px" }}>
        <thead className="text-fg-4">
          <tr>
            <th className="pb-2 font-medium">Name</th>
            <th className="w-28 pb-2 text-right font-medium">Total</th>
            {harnesses.map((harness) => (
              <th key={harness} className="w-28 pb-2 text-right font-medium">
                <span className="inline-flex items-center gap-1">
                  <span
                    className="h-2 w-2 rounded-full"
                    style={{ backgroundColor: harnessColor(harness) }}
                  />
                  {harness}
                </span>
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {sorted.map((row) => {
            const pairs = expandablePairs ? (row.models ?? []) : [];
            return (
              <Fragment key={row.id}>
                <tr
                  className="border-t border-line text-fg-2"
                  tabIndex={0}
                  data-testid="stats-detail-row"
                >
                  <td className="py-2 pr-2">
                    <span className="inline-flex items-center gap-1">
                      {pairs.length > 0 ? (
                        <button
                          type="button"
                          aria-label={`${expanded.has(row.id) ? "Collapse" : "Expand"} ${row.name} models`}
                          data-testid="stats-node-toggle"
                          onClick={() =>
                            setExpanded((current) => {
                              const next = new Set(current);
                              if (next.has(row.id)) next.delete(row.id);
                              else next.add(row.id);
                              return next;
                            })
                          }
                          className="shrink-0 hover:text-fg"
                        >
                          {expanded.has(row.id) ? (
                            <ChevronDown size={12} />
                          ) : (
                            <ChevronRight size={12} />
                          )}
                        </button>
                      ) : null}
                      {(() => {
                        const content = renderName ? renderName(row) : row.name;
                        return onOpen ? (
                          <button
                            type="button"
                            aria-label={`Open ${row.name}`}
                            onClick={() => onOpen(row)}
                            className="text-left hover:text-fg"
                          >
                            {content}
                          </button>
                        ) : (
                          <span>{content}</span>
                        );
                      })()}
                    </span>
                  </td>
                  <td className="py-2 text-right">
                    <CostCell
                      unit={unit}
                      metric={{
                        harness: "total",
                        usd: row.usd,
                        estimated: row.estimated,
                        partial: row.partial,
                        executions: row.executions,
                        readable: row.readable,
                        unknown: row.unknown,
                        average_usd: row.average_usd,
                        median_usd: row.median_usd,
                        unpriced_models: row.unpriced_models,
                        missing_reasons: row.missing_reasons,
                      }}
                    />
                  </td>
                  {harnesses.map((harness) => (
                    <td key={harness} className="py-2 text-right">
                      <CostCell
                        unit={unit}
                        metric={row.harnesses.find((item) => item.harness === harness)}
                      />
                    </td>
                  ))}
                </tr>
                {expanded.has(row.id)
                  ? pairs.map((pair) => (
                      <tr
                        key={`${row.id}-${pair.model}-${pair.effort ?? ""}`}
                        className="border-t border-line bg-bg-3/40 text-fg-3"
                        data-testid="stats-model-effort-row"
                      >
                        <td className="py-2 pl-7 pr-2">
                          <span className="inline-flex items-baseline gap-1">
                            <ProvenanceName
                              name={pair.model}
                              mono
                              provenance={pair.model_provenance}
                              harnesses={pair.harnesses}
                              target="model"
                            />
                            <span className="text-fg-4">·</span>
                            {pair.effort === null ? (
                              <Tooltip content={NOT_SET_COPY} side="top">
                                <span className="italic text-fg-4">not set</span>
                              </Tooltip>
                            ) : (
                              <ProvenanceName
                                name={pair.effort}
                                provenance={pair.effort_provenance}
                                harnesses={pair.harnesses}
                                target="effort"
                              />
                            )}
                          </span>
                        </td>
                        <td className="py-2 text-right">
                          <CostCell unit={unit} metric={pairMetric(pair)} />
                        </td>
                        {harnesses.map((harness) => (
                          <td key={harness} className="py-2 text-right">
                            <CostCell
                              unit={unit}
                              metric={pair.harnesses.find(
                                (item) => item.harness === harness,
                              )}
                            />
                          </td>
                        ))}
                      </tr>
                    ))
                  : null}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </TooltipProvider>
  );
}

type CostAxis = "pipeline" | "project" | "model";

function CostTab({
  cost,
  error,
  onAbsorptionsChanged,
}: {
  cost: StatsCost | null;
  error: string | null;
  onAbsorptionsChanged: () => void;
}) {
  const [axis, setAxis] = useState<CostAxis>("pipeline");
  // #890: only the « By pipeline » rows are Pipelines to combine; switching the
  // grouping drops the selection with them.
  const absorption = usePipelineAbsorption({
    rows: cost?.by_pipeline ?? [],
    enabled: axis === "pipeline",
    count: RUNS_COUNT,
    onChanged: onAbsorptionsChanged,
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [drilledPipelineId, setDrilledPipelineId] = useState<string | null>(null);
  // The model axis drills Model → Effort → Pipeline → Node (ADR-0065). The
  // effort id is "" for the "not set" bucket, so selection is `null` vs value,
  // never falsy-compared.
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const [selectedEffortId, setSelectedEffortId] = useState<string | null>(null);
  const [selectedPipelineId, setSelectedPipelineId] = useState<string | null>(null);

  if (error) {
    return (
      <div className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed">
        {error}
      </div>
    );
  }
  if (!cost) return <EmptyNote>Loading cost…</EmptyNote>;

  const toTotal = () => {
    setSelectedId(null);
    setDrilledPipelineId(null);
    setSelectedModelId(null);
    setSelectedEffortId(null);
    setSelectedPipelineId(null);
  };

  // Model axis: resolve the drill path.
  const model =
    axis === "model"
      ? (cost.by_model.find((row) => row.id === selectedModelId) ?? null)
      : null;
  const effort =
    model && selectedEffortId !== null
      ? (model.efforts.find((row) => row.id === selectedEffortId) ?? null)
      : null;
  const modelPipeline =
    effort && selectedPipelineId !== null
      ? (effort.pipelines.find((row) => row.id === selectedPipelineId) ?? null)
      : null;

  const rows: StatsCostEntity[] =
    axis === "pipeline"
      ? cost.by_pipeline
      : axis === "project"
        ? cost.by_project
        : cost.by_model;
  const selected =
    axis === "model" ? null : (rows.find((row) => row.id === selectedId) ?? null);
  const drilledPipeline =
    axis === "project" && selected
      ? ((selected as StatsProjectCostEntity).pipelines.find(
          (pipeline) => pipeline.id === drilledPipelineId,
        ) ?? null)
      : null;

  const aggregate =
    axis === "model"
      ? (modelPipeline ?? effort ?? model ?? cost.total)
      : (drilledPipeline ?? selected ?? cost.total);
  const periods =
    axis === "model"
      ? (modelPipeline ?? effort ?? model)?.by_period ?? cost.by_period
      : drilledPipeline
        ? drilledPipeline.by_period
        : selected
          ? selected.by_period
          : cost.by_period;

  // The denominator the headline names (ADR-0065 §3): a model bucket counts
  // executions — the whole model axis reads "per execution" — and so does the
  // Node level of the other axes.
  const atNodeLevel =
    axis === "model" ? true : axis === "pipeline" ? selected !== null : drilledPipeline !== null;
  const detailUnit: "Run" | "execution" = atNodeLevel ? "execution" : "Run";

  let detailRows: StatsCostEntity[];
  let detailRenderName: ((row: StatsCostEntity) => React.ReactNode) | undefined;
  let onOpen: ((row: StatsCostEntity) => void) | undefined;
  if (axis === "model") {
    if (modelPipeline) {
      detailRows = modelPipeline.nodes;
    } else if (effort) {
      detailRows = effort.pipelines;
      onOpen = (row) => setSelectedPipelineId(row.id);
    } else if (model) {
      detailRows = model.efforts;
      detailRenderName = (row) => effortNameCell(row as StatsEffortCostEntity);
      onOpen = (row) => setSelectedEffortId(row.id);
    } else {
      detailRows = cost.by_model;
      detailRenderName = (row) => (
        <ProvenanceName
          name={row.name}
          mono
          provenance={cost.by_model.find((m) => m.id === row.id)?.provenance}
          harnesses={row.harnesses}
          target="model"
        />
      );
      onOpen = (row) => setSelectedModelId(row.id);
    }
  } else if (drilledPipeline) {
    detailRows = drilledPipeline.nodes;
  } else if (!selected) {
    detailRows = rows;
    if (axis === "pipeline") {
      detailRenderName = (row) => (
        <PipelineName row={row} onOpenMembers={absorption.openMembers} />
      );
    }
  } else if (axis === "project") {
    detailRows = (selected as StatsProjectCostEntity).pipelines;
    onOpen = (pipeline) => setDrilledPipelineId(pipeline.id);
  } else {
    detailRows = selected.nodes;
  }

  // #736: a harness without a cost source (opencode) has no model row,
  // so the model axis drops its column; the other axes keep it and show « — ».
  // The daemon's reason string is "harness has no cost source" — match on the
  // stable substring, not the full wire value.
  const noCostSource = cost.harnesses.filter((h) =>
    cost.total.harnesses
      .find((m) => m.harness === h)
      ?.missing_reasons.some((r) => r.includes("no cost source")),
  );
  const tableHarnesses =
    axis === "model" ? cost.harnesses.filter((h) => !noCostSource.includes(h)) : cost.harnesses;

  const crumbs: { label: string; onClick?: () => void }[] = [
    { label: "Total", onClick: toTotal },
  ];
  if (axis === "model") {
    if (model)
      crumbs.push({
        label: model.name,
        onClick: () => {
          setSelectedEffortId(null);
          setSelectedPipelineId(null);
        },
      });
    if (effort)
      crumbs.push({ label: effort.name, onClick: () => setSelectedPipelineId(null) });
    if (modelPipeline) crumbs.push({ label: modelPipeline.name });
  } else {
    if (selected) crumbs.push({ label: selected.name, onClick: () => setDrilledPipelineId(null) });
    if (drilledPipeline) crumbs.push({ label: drilledPipeline.name });
  }
  // Every crumb but the last pops the levels it shadows (design: clickable
  // breadcrumb for all three axes once there are four levels).
  const clickableCrumbs = crumbs.map((crumb, index) =>
    index === crumbs.length - 1 ? { label: crumb.label } : crumb,
  );

  return (
    <div className="relative flex min-h-full" data-testid="stats-chart-cost">
      <aside
        className="w-[290px] shrink-0 border-r border-line pr-4"
        data-testid="stats-drilldown-navigation"
      >
        <div className="mb-2 flex items-center justify-between gap-2">
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Ranked by cost
          </span>
          <select
            aria-label="Cost grouping"
            value={axis}
            onChange={(event) => {
              setAxis(event.target.value as CostAxis);
              toTotal();
            }}
            className="rounded border border-line bg-bg-3 px-2 py-1 text-fg-2"
          >
            <option value="pipeline">By pipeline</option>
            <option value="project">By project</option>
            <option value="model">By model</option>
          </select>
        </div>
        <MasterList
          rows={rows}
          selected={axis === "model" ? selectedModelId : selectedId}
          monoName={axis === "model"}
          valueLabel={(row) => formatCostAmount(row.usd, row.partial, row.estimated)}
          selection={absorption.selection}
          onSelect={(id) => {
            if (axis === "model") {
              setSelectedModelId(id);
              setSelectedEffortId(null);
              setSelectedPipelineId(null);
            } else {
              setSelectedId(id);
              setDrilledPipelineId(null);
            }
          }}
        />
        {axis === "model" && (
          <div className="mt-3 text-fg-4" style={{ fontSize: "10.5px" }}>
            Model ids verbatim, one row per id — the same id run through two
            harnesses is one row, one column per harness. Hover a model or an
            effort for where the value was read and its provider.
            {noCostSource.length > 0 && (
              <>
                {" "}
                <span className="font-mono">{noCostSource.join(", ")}</span> has no cost
                source and is not on this axis.
              </>
            )}
          </div>
        )}
      </aside>

      <div
        className="min-w-0 flex-1 pl-5"
        data-testid="stats-drilldown-detail"
      >
        <Breadcrumb crumbs={clickableCrumbs} />
        <HarnessLegend harnesses={cost.harnesses} />
        <div className="mt-4 text-fg" data-testid="stats-selection-headline">
          {formatCostAmount(aggregate.usd, aggregate.partial, aggregate.estimated)} total
          {" · "}
          {formatCostAmount(aggregate.median_usd, aggregate.partial, aggregate.estimated)} median
          per {detailUnit}
        </div>
        <div className="mt-4">
          <HarnessCards aggregate={aggregate} />
        </div>
        {aggregate.unknown > 0 && (
          <div className="mt-4 text-st-await" style={{ fontSize: "10.5px" }}>
            {aggregate.unknown} {detailUnit}
            {aggregate.unknown === 1 ? "" : "s"} without computable cost
          </div>
        )}
        <div className="mt-4">
          <HarnessBars periods={periods} harnesses={cost.harnesses} value="usd" />
        </div>
        <div className="mt-4 min-h-[240px]">
          <CostTable
            key={`${axis}-${selectedId ?? ""}-${drilledPipelineId ?? ""}-${selectedModelId ?? ""}-${selectedEffortId ?? "total"}-${selectedPipelineId ?? ""}`}
            rows={detailRows}
            harnesses={tableHarnesses}
            unit={detailUnit}
            onOpen={onOpen}
            renderName={detailRenderName}
            expandablePairs={axis !== "model"}
          />
        </div>
      </div>
      {absorption.overlay}
    </div>
  );
}

type PerformanceMetric = "context" | "duration" | "steering";

const PERFORMANCE_METRICS: readonly PerformanceMetric[] = ["context", "duration", "steering"];

/**
 * The box-plot zoom level (#811, CONTEXT.md « Niveau de zoom d'un box-plot »):
 * one setting for the whole Performance section, deciding what a plot draws AND
 * where the axis tops out. The three levels differ only by which pair of
 * statistics bounds the drawing — everything below reads these two tables
 * rather than branching on the level by hand.
 *
 * The daemon never sends the observations (ADR-0029 / the glossary), so a level
 * can only be a choice among the statistics it already sent: that is why
 * `fence_low`/`fence_high` are computed server-side.
 */
const ZOOM_LOW: Record<StatsZoom, "min" | "fence_low" | "q1"> = {
  full: "min",
  fenced: "fence_low",
  box: "q1",
};

const ZOOM_HIGH: Record<StatsZoom, "max" | "fence_high" | "q3"> = {
  full: "max",
  fenced: "fence_high",
  box: "q3",
};

const ZOOM_LEVELS: readonly StatsZoom[] = ["full", "fenced", "box"];

const ZOOM_LABEL: Record<StatsZoom, string> = {
  full: "Full",
  fenced: "Fenced",
  box: "Box",
};

/** What the shared axis is capped at, said in the toolbar's caption. */
const ZOOM_CAP_COPY: Record<StatsZoom, string> = {
  full: "max",
  fenced: "fence high",
  box: "Q3",
};

/** A 16×8 miniature of what the level draws — whiskers to the extremes (Full),
 *  shorter whiskers with a point left outside (Fenced), the bare box (Box). */
function ZoomGlyph({ level }: { level: StatsZoom }) {
  return (
    <svg
      width="16"
      height="8"
      viewBox="0 0 16 8"
      aria-hidden="true"
      className="shrink-0 overflow-visible"
      data-testid={`stats-zoom-glyph-${level}`}
    >
      {level !== "box" && (
        <line
          x1={level === "full" ? 0.5 : 3}
          x2={level === "full" ? 15.5 : 12}
          y1="4"
          y2="4"
          stroke="currentColor"
          strokeWidth="1"
          opacity="0.55"
        />
      )}
      <rect
        x={level === "box" ? 3.5 : 5.5}
        y="1.5"
        width={level === "box" ? 9 : 5}
        height="5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1"
      />
      <line x1="8" y1="0.5" x2="8" y2="7.5" stroke="currentColor" strokeWidth="1" />
      {level === "fenced" && <circle cx="15" cy="4" r="1" fill="currentColor" opacity="0.55" />}
    </svg>
  );
}

/** The three-position zoom control: one radiogroup, roving tabindex, ← → walk
 *  the levels (a select would hide two of the three states behind a click). */
function ZoomSegments({
  value,
  onChange,
}: {
  value: StatsZoom;
  onChange: (zoom: StatsZoom) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label="Box-plot zoom"
      data-testid="stats-performance-zoom"
      className="flex items-center gap-0.5 rounded border border-line bg-bg-3 p-0.5"
      onKeyDown={(event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
        event.preventDefault();
        const delta = event.key === "ArrowRight" ? 1 : -1;
        const index = ZOOM_LEVELS.indexOf(value);
        const next = ZOOM_LEVELS[(index + delta + ZOOM_LEVELS.length) % ZOOM_LEVELS.length];
        onChange(next);
        // Roving tabindex: the selection carries the focus, so a second arrow
        // press keeps walking instead of falling back to the group.
        event.currentTarget
          .querySelector<HTMLButtonElement>(`[data-zoom="${next}"]`)
          ?.focus();
      }}
    >
      {ZOOM_LEVELS.map((level) => {
        const selected = level === value;
        return (
          <button
            key={level}
            type="button"
            role="radio"
            aria-checked={selected}
            data-zoom={level}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(level)}
            className={`flex items-center gap-1.5 rounded px-2 py-0.5 transition-colors ${
              selected ? "bg-bg-5 text-fg" : "text-fg-3 hover:text-fg-2"
            }`}
          >
            <ZoomGlyph level={level} />
            {ZOOM_LABEL[level]}
          </button>
        );
      })}
    </div>
  );
}

/** « Independent scales »: each row on its own axis instead of the shared one.
 *  Same switch idiom as Settings › Interface — this is a per-browser reading
 *  preference, not a daemon knob. */
function IndependentScalesSwitch({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <span className="flex items-center gap-2">
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label="Independent scales"
        data-testid="stats-performance-independent-scales"
        onClick={() => onChange(!checked)}
        className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
          checked ? "bg-acc" : "bg-fg-5"
        }`}
      >
        <span
          className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
            checked ? "left-3" : "left-0.5"
          }`}
        />
      </button>
      <span className="text-fg-3">Independent scales</span>
    </span>
  );
}

/** The wire field a metric actually reads. The **duration mode** (#819) picks
 *  which of Duration's three distributions is read — wall-clock, minus the
 *  declared wait, or that wait alone — with **no refetch**: the three always
 *  travel together, over the same executions and the same coverage. */
type PerformanceField =
  | "context"
  | "duration"
  | "active_duration"
  | "wait_duration"
  | "steering";

/** Which duration field each mode reads. */
const MODE_FIELD: Record<DurationMode, PerformanceField> = {
  total: "duration",
  active: "active_duration",
  waiting: "wait_duration",
};

function metricField(
  metric: PerformanceMetric,
  mode: DurationMode,
): PerformanceField {
  return metric === "duration" ? MODE_FIELD[mode] : metric;
}

/** The logical metric a field belongs to — what decides its unit and format.
 *  All three duration readings are durations: same milliseconds, same `6m20s`. */
function fieldMetric(field: PerformanceField): PerformanceMetric {
  return field === "active_duration" || field === "wait_duration"
    ? "duration"
    : field;
}

const PERFORMANCE_FIELD_LABEL: Record<PerformanceField, string> = {
  context: "Context",
  duration: "Duration",
  active_duration: "Active",
  wait_duration: "Waiting",
  steering: "Steering",
};

/** What a **declared wait** is, said in words (#819): the rule, not a decision
 *  reference. The mode's « i », the column header and the cards share it. */
const DECLARED_WAIT_COPY =
  "a declared wait is a node waiting for you (pdo wait-user) or a completion you have not released yet";

/** Why a partially filtered row cannot show a number (#810): six stats are not
 *  observations, so no honest total can be rebuilt from the visible nodes. */
const FILTERED_BY_KIND_COPY =
  "Filtered by node kind: totals are not recomputed from the visible nodes' six stats";

/** « duration » / « active duration » / « waiting » — the word every Performance
 *  label uses under the current mode, so the headline, the cards, the sort
 *  select and the aside never disagree about what is being measured. */
const DURATION_WORD: Record<DurationMode, string> = {
  total: "duration",
  active: "active duration",
  waiting: "waiting",
};

function durationWord(mode: DurationMode): string {
  return DURATION_WORD[mode];
}

function performanceSortLabel(
  metric: PerformanceMetric,
  mode: DurationMode,
): string {
  return metric === "duration" ? durationWord(mode) : metric;
}

// --- Node kind (#810, CONTEXT.md § Genre de nœud) ------------------------------

/** Does this row carry a kind? Only Node rows do — a Pipeline, a subagent
 *  group, an Infrastructure role and every level of the « By model » tree
 *  answer `false` and are never touched by the filter. */
function isNodeRow(row: StatsPerformanceEntity): boolean {
  return row.interactive !== undefined || row.orchestrator !== undefined;
}

/** The kinds a Node row matches. Both flags count: a node that is interactive
 *  AND orchestrator appears under either chip, so no execution is hidden by an
 *  exclusive classification. */
function rowKinds(row: StatsPerformanceEntity): NodeKind[] {
  const kinds: NodeKind[] = [];
  if (row.interactive) kinds.push("interactive");
  if (row.orchestrator) kinds.push("orchestrator");
  if (kinds.length === 0) kinds.push("standard");
  return kinds;
}

function matchesKinds(row: StatsPerformanceEntity, kinds: NodeKind[]): boolean {
  if (!isNodeRow(row)) return true;
  return rowKinds(row).some((kind) => kinds.includes(kind));
}

/** How many Node rows of the whole period carry each kind — the counts on the
 *  chips. Read from the Pipeline tree, the canonical node population; the
 *  « By model » axis re-buckets those same executions. */
function nodeKindCounts(
  rows: StatsPerformanceEntity[],
): Record<NodeKind, number> {
  const counts: Record<NodeKind, number> = {
    interactive: 0,
    orchestrator: 0,
    standard: 0,
  };
  for (const pipeline of rows) {
    for (const node of pipeline.nodes) {
      for (const kind of rowKinds(node)) counts[kind] += 1;
    }
  }
  return counts;
}

/** Does the kind filter hide at least one Node of this pipeline? A pipeline
 *  whose every node is visible keeps its own total; one that lost a node shows
 *  « filtered ». */
function hasHiddenNode(
  pipeline: StatsPerformanceEntity,
  kinds: NodeKind[],
): boolean {
  return pipeline.nodes.some((node) => !matchesKinds(node, kinds));
}

/** The badges the Name cell carries for a Node row (#810) — nothing at all for
 *  a standard node: an absence needs no mark. */
function KindBadges({ row }: { row: StatsPerformanceEntity }) {
  if (!isNodeRow(row)) return null;
  const kinds = rowKinds(row).filter((kind) => kind !== "standard");
  if (kinds.length === 0) return null;
  return (
    <>
      {kinds.map((kind) => (
        <span
          key={kind}
          data-testid={`stats-node-kind-${kind}`}
          className="shrink-0 rounded border border-line-strong px-1 font-mono text-fg-4"
          style={{ fontSize: "9px" }}
        >
          {kind}
        </span>
      ))}
    </>
  );
}

/** The kind filter left nothing on screen (#810) — the whole tab when every
 *  chip is off, a drill level when its every Node row is hidden. Both say the
 *  same sentence and carry the same way out: a filter that empties a table must
 *  own the emptiness rather than leave bare headers explaining nothing. */
function NoNodeOfSelectedKinds({
  where,
  onShowAll,
}: {
  /** Names what was emptied: « in this period » / « at this level ». */
  where: string;
  onShowAll: () => void;
}) {
  return (
    <EmptyNote>
      No node of the selected kinds {where}.{" "}
      <button
        type="button"
        data-testid="stats-show-all-kinds"
        onClick={onShowAll}
        className="underline decoration-dotted underline-offset-2 hover:text-fg-2"
      >
        show all kinds
      </button>
    </EmptyNote>
  );
}

/** The value a partially filtered row shows in place of a number. */
function FilteredValue({ testid }: { testid?: string }) {
  return (
    <Tooltip content={FILTERED_BY_KIND_COPY} side="top">
      <span className="italic text-fg-4" data-testid={testid}>
        filtered
      </span>
    </Tooltip>
  );
}

/** Where the Steering metric comes from (#792) — the header « i », the card
 *  « i » and every steering tooltip say it in the same words. */
const STEERING_PROVENANCE_COPY =
  "derived from harness transcripts, launch prompt and runtime messages excluded";

/** « 23 % » for a steered rate, or `null` when no execution's count was
 *  readable — the caller renders « — » and the reason, never 0 %. */
function steeredPercent(rate: StatsSteeredRate | undefined): string | null {
  if (!rate || rate.readable === 0) return null;
  return `${Math.round((rate.steered / rate.readable) * 100)} %`;
}

/** A row's ranking score on a metric: its worst harness's **median** first, the
 *  mean only as a tie-break (#811 — « Médiane, jamais la moyenne »: the median
 *  ranks and labels, the mean survives in the tooltip). `-1` for a row with no
 *  measured distribution at all, so it sinks below any real value. */
function performanceScore(
  aggregate: StatsPerformanceAggregate,
  metric: PerformanceMetric,
  mode: DurationMode = "total",
): [number, number] {
  const field = metricField(metric, mode);
  const distributions = aggregate.harnesses
    .map((item) => item[field])
    .filter((item) => item.stats !== null);
  return [
    Math.max(-1, ...distributions.map((item) => item.stats!.median)),
    Math.max(-1, ...distributions.map((item) => item.stats!.mean)),
  ];
}

function sortPerformance<
  T extends StatsPerformanceAggregate & { name: string },
>(rows: T[], metric: PerformanceMetric, mode: DurationMode = "total"): T[] {
  return [...rows].sort((a, b) => {
    const [aMedian, aMean] = performanceScore(a, metric, mode);
    const [bMedian, bMean] = performanceScore(b, metric, mode);
    return bMedian - aMedian || bMean - aMean || a.name.localeCompare(b.name);
  });
}

function formatPerformanceValue(value: number, metric: PerformanceMetric): string {
  if (metric === "context") {
    return value >= 1_000 ? `${Math.round(value / 1_000)}k` : Math.round(value).toString();
  }
  if (metric === "steering") {
    // Messages: an integer reads as one (`0`, `2`), a mean keeps one decimal (`0.8`).
    const rounded = Math.round(value * 10) / 10;
    return Number.isInteger(rounded) ? rounded.toString() : rounded.toFixed(1);
  }
  const seconds = Math.round(value / 1_000);
  const minutes = Math.floor(seconds / 60);
  return minutes ? `${minutes}m${String(seconds % 60).padStart(2, "0")}s` : `${seconds}s`;
}

function distributionDetail(
  name: string,
  harness: string,
  field: PerformanceField,
  value: StatsDistribution,
  /** The six numbers never move; the Fenced level appends the two bounds it
   *  draws its whiskers at, so the reader can name what cropped the plot. */
  zoom: StatsZoom = "full",
): string {
  const metric = fieldMetric(field);
  const label = PERFORMANCE_FIELD_LABEL[field];
  const fmt = (raw: number) => formatPerformanceValue(raw, metric);
  const stats = value.stats;
  const provenance =
    metric === "steering"
      ? ` Steering ${STEERING_PROVENANCE_COPY}.`
      : field === "active_duration"
        ? ` Active: wall-clock minus the declared waits — ${DECLARED_WAIT_COPY}.`
        : field === "wait_duration"
          ? ` Waiting: the declared waits alone — ${DECLARED_WAIT_COPY}.`
          : "";
  if (!stats) {
    return `${name} · ${harness} · ${label}. 0 measured of ${value.expected} successful executions. Missing: ${value.missing_reasons.join("; ")}.${provenance}`;
  }
  const reasons = value.missing_reasons.length
    ? ` Missing: ${value.missing_reasons.join("; ")}.`
    : "";
  const fences =
    zoom === "fenced"
      ? ` · Fence high ${fmt(stats.fence_high)} · Fence low ${fmt(stats.fence_low)}`
      : "";
  return `${name} · ${harness} · ${label}. Max ${fmt(stats.max)} · Q3 ${fmt(stats.q3)} · Mean ${fmt(stats.mean)} · Median ${fmt(stats.median)} · Q1 ${fmt(stats.q1)} · Min ${fmt(stats.min)}${fences}. ${value.measured} measured of ${value.expected} successful executions.${reasons}${provenance}`;
}

function DistributionPlot({
  name,
  harness,
  field,
  value,
  scaleMax,
  zoom,
  rowMax = null,
  steered,
  waitMillis,
  ghost,
}: {
  name: string;
  harness: string;
  field: PerformanceField;
  value: StatsDistribution;
  scaleMax: number;
  /** What the whiskers reach and where the axis tops out (#811). */
  zoom: StatsZoom;
  /** The row's own cap, printed under the plot when « independent scales » is
   *  on — without it, two rows drawn full width read as comparable when they
   *  are not. `null` on the shared axis, where the header says the cap once. */
  rowMax?: number | null;
  /** The row × harness steered rate — read for the Steering metric only, where
   *  the line under the plot adds « · 23 % steered » (#792). */
  steered?: StatsSteeredRate;
  /** How much declared wait this row × harness lost to the active reading
   *  (#810), in milliseconds — the amber « −1m02s wait » that names what moved.
   *  Absent when the toggle is off or nothing was waited. */
  waitMillis?: number;
  /** The wall-clock Q1–Q3 the active box replaced (#810), drawn behind it as a
   *  dashed ghost: the reading the toggle hid stays reachable at a glance
   *  instead of vanishing. */
  ghost?: { q1: number; q3: number } | null;
}) {
  const metric = fieldMetric(field);
  if (!value.stats) {
    const detail = distributionDetail(name, harness, field, value, zoom);
    return (
      <Tooltip content={detail} side="top">
        <button
          type="button"
          aria-label={detail}
          className="text-left text-fg-4 underline decoration-dotted underline-offset-2"
        >
          — {value.missing_reasons[0] ?? "not measurable"}
        </button>
      </Tooltip>
    );
  }
  const stats = value.stats;
  const pct = (raw: number) =>
    `${Math.max(0, Math.min(100, (raw / scaleMax) * 100))}%`;
  const detail = distributionDetail(name, harness, field, value, zoom);
  const partial = value.measured < value.expected;
  const steeredLine = metric === "steering" ? steeredPercent(steered) : null;
  const low = stats[ZOOM_LOW[zoom]];
  const high = stats[ZOOM_HIGH[zoom]];
  // A clip mark says « something exists beyond the frame » without drawing it.
  // Fenced marks both tails (an outlier on either side is news); Box marks only
  // the long tail — below Q1 half the sample is always out, which is the level's
  // whole point, not an event.
  const clippedHigh = zoom !== "full" && stats.max > high;
  const clippedLow = zoom === "fenced" && stats.min < low;
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <div
        className="relative h-4 min-w-28"
        data-testid={`performance-${metric}-boxplot`}
        data-scale-max={scaleMax}
        data-zoom={zoom}
        aria-hidden="true"
      >
        {ghost && (
          <span
            className="absolute top-[4px] h-[7px] border border-dashed border-fg-4 opacity-40"
            data-testid="performance-wallclock-ghost"
            style={{
              left: pct(ghost.q1),
              width: pct(Math.max(ghost.q3 - ghost.q1, scaleMax * 0.005)),
            }}
          />
        )}
        {zoom !== "box" && (
          <span
            className="absolute top-[7px] h-px bg-fg-4"
            data-testid={`performance-${metric}-whisker`}
            style={{ left: pct(low), width: pct(high - low) }}
          />
        )}
        <span
          className="absolute top-[4px] h-[7px] border border-current opacity-70"
          style={{
            color: harnessColor(harness),
            left: pct(stats.q1),
            width: pct(Math.max(stats.q3 - stats.q1, scaleMax * 0.005)),
          }}
        />
        <span
          className="absolute top-[3px] h-[9px] w-px bg-fg"
          style={{ left: pct(stats.median) }}
        />
        {clippedLow && (
          <span
            className="absolute left-0 top-0 font-mono leading-4 text-fg-4"
            style={{ fontSize: "9px" }}
            data-testid={`performance-${metric}-clip-low`}
          >
            ‹
          </span>
        )}
        {clippedHigh && (
          <span
            className="absolute right-0 top-0 font-mono leading-4 text-fg-4"
            style={{ fontSize: "9px" }}
            data-testid={`performance-${metric}-clip-high`}
          >
            ›
          </span>
        )}
      </div>
      <Tooltip content={detail} side="top">
        <button
          type="button"
          aria-label={detail}
          className="w-fit text-left font-mono text-fg-4 underline decoration-dotted underline-offset-2"
          style={{ fontSize: "9.5px" }}
        >
          {formatPerformanceValue(stats.median, metric)} median · n={value.measured}
          {partial ? " ⚠" : ""}
          {steeredLine ? ` · ${steeredLine} steered` : ""}
          {waitMillis ? (
            <span
              className="text-st-await"
              data-testid="performance-wait-delta"
            >
              {" "}
              −{formatPerformanceValue(waitMillis, "duration")} wait
            </span>
          ) : null}
          {rowMax !== null && (
            <span className="text-fg-5"> · ⤒ {formatPerformanceValue(rowMax, metric)}</span>
          )}
        </button>
      </Tooltip>
    </div>
  );
}

function PerformanceCards({
  aggregate,
  mode,
  partialByKind,
}: {
  aggregate: StatsPerformanceAggregate;
  mode: DurationMode;
  /** The kind filter hides part of what this aggregate pooled (#810). The head
   *  cards then fall back to « — »: they would otherwise show a number for a
   *  population that is no longer on screen, and a median of medians is not a
   *  median. */
  partialByKind: boolean;
}) {
  const durationField = metricField("duration", mode);
  const central = (value: StatsDistribution, metric: PerformanceMetric) => {
    if (partialByKind || !value.stats) return "—";
    return formatPerformanceValue(value.stats.median, metric);
  };
  return (
    <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
      {aggregate.harnesses.map((item) => (
        <div
          key={item.harness}
          className="rounded-md border border-line bg-bg-3 p-3"
          data-testid={`stats-performance-card-${item.harness}`}
        >
          <div className="mb-2 flex items-center gap-1.5 text-fg-3">
            <span
              className="h-2 w-2 rounded-full"
              style={{ backgroundColor: harnessColor(item.harness) }}
            />
            {item.harness}
          </div>
          <div className="font-mono text-fg">
            {central(item.context, "context")} median context
          </div>
          <div className="flex items-center gap-1 font-mono text-fg-3">
            <span>
              {central(item[durationField], "duration")} median{" "}
              {durationWord(mode)}
            </span>
            {mode !== "total" && (
              <Tooltip content={DECLARED_WAIT_COPY} side="top">
                <span
                  role="img"
                  aria-label={DECLARED_WAIT_COPY}
                  className="inline-flex text-fg-4"
                >
                  <Info size={11} />
                </span>
              </Tooltip>
            )}
          </div>
          <div className="font-mono text-fg-3">
            {central(item.steering, "steering")} median steering
          </div>
        </div>
      ))}
      <SteeredCard aggregate={aggregate} partialByKind={partialByKind} />
    </div>
  );
}

/** « Steered executions » (#792): one row per harness — the share of successful
 *  executions with ≥ 1 steering message, its coverage « steered / readable »
 *  and a thin bar; « — » with the reason when no count was readable. Follows
 *  the drill like the other cards (it reads the same aggregate). */
function SteeredCard({
  aggregate,
  partialByKind = false,
}: {
  aggregate: StatsPerformanceAggregate;
  partialByKind?: boolean;
}) {
  return (
    <div
      className="rounded-md border border-line bg-bg-3 p-3 sm:col-span-2"
      data-testid="stats-steered-card"
    >
      <div className="mb-2 flex items-center justify-between gap-2 text-fg-3">
        <span>Steered executions</span>
        <TooltipProvider>
          <Tooltip content={STEERING_PROVENANCE_COPY} side="top">
            <span
              role="img"
              aria-label={STEERING_PROVENANCE_COPY}
              className="inline-flex text-fg-4"
            >
              <Info size={12} />
            </span>
          </Tooltip>
        </TooltipProvider>
      </div>
      <div className="grid gap-1.5">
        {aggregate.harnesses.map((item) => {
          // #810: a partial kind filter hides part of the executions this rate
          // was computed over — « filtered », never a share of a population the
          // user cannot see.
          const percent = partialByKind ? null : steeredPercent(item.steered);
          const ratio = item.steered.readable
            ? (item.steered.steered / item.steered.readable) * 100
            : 0;
          return (
            <div
              key={item.harness}
              className="flex items-center gap-2 font-mono"
              data-testid={`stats-steered-${item.harness}`}
            >
              <span
                className="h-2 w-2 shrink-0 rounded-full"
                style={{ backgroundColor: harnessColor(item.harness) }}
              />
              <span className="w-16 shrink-0 text-fg-2">{item.harness}</span>
              {percent ? (
                <>
                  <span className="w-12 shrink-0 text-fg">{percent}</span>
                  <span className="shrink-0 text-fg-4">
                    · {item.steered.steered}/{item.steered.readable}
                  </span>
                  <span className="relative h-1 min-w-10 flex-1 rounded bg-bg-2">
                    <span
                      className="absolute inset-y-0 left-0 rounded"
                      style={{ width: `${ratio}%`, backgroundColor: harnessColor(item.harness) }}
                    />
                  </span>
                </>
              ) : partialByKind ? (
                <FilteredValue />
              ) : (
                <span className="text-fg-4">
                  — {item.steering.missing_reasons[0] ?? "no readable execution"}
                </span>
              )}
            </div>
          );
        })}
      </div>
      <div className="mt-2 text-fg-4" style={{ fontSize: "10px" }}>
        share of successful executions with ≥ 1 steering message · steered / readable
      </div>
    </div>
  );
}

function performanceEffortName(row: PerformanceEffortEntity): React.ReactNode {
  if (row.effort === null) {
    // The "not set" bucket: hovering the italic word explains it (ADR-0065 §1).
    return (
      <Tooltip content={NOT_SET_COPY} side="top">
        <span className="italic text-fg-4">not set</span>
      </Tooltip>
    );
  }
  return (
    <ProvenanceName
      name={row.name}
      provenance={row.provenance}
      harnesses={[]}
      target="effort"
      content={performanceProvenanceCopy(row.provenance)}
    />
  );
}

function PerformanceTable({
  rows,
  harnesses,
  sort,
  mode,
  zoom,
  independentAxis,
  renderName,
  onOpen,
  expandablePairs = false,
  filteredRowIds,
}: {
  rows: StatsPerformanceEntity[];
  harnesses: string[];
  sort: PerformanceMetric;
  /** Which of the three readings the Duration column shows (#819) — header,
   *  plots, tooltips and the row's « −…s wait » alike. */
  mode: DurationMode;
  /** Section-level, from `PerformanceTab` — the whole table draws one level. */
  zoom: StatsZoom;
  /** Each row on its own axis instead of one axis per metric (#811). */
  independentAxis: boolean;
  /** Replaces the default (button-or-plain) name cell — the model axis marks
   *  provenance and renders "not set" in its own voice (#737). */
  renderName?: (row: StatsPerformanceEntity) => React.ReactNode;
  onOpen?: (row: StatsPerformanceEntity) => void;
  /** Node rows gain a chevron unfolding their model × effort couples (ADR-0065).
   *  Off on the model axis, where the model × effort path is already the drill. */
  expandablePairs?: boolean;
  /** Rows whose aggregate pooled executions the kind filter now hides (#810):
   *  their cells read « filtered » rather than a total that no longer describes
   *  what is on screen. */
  filteredRowIds?: Set<string>;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  // The couples' own expansion, dying with the table like `expanded` — the
  // parent keys the table on the drill path, so a stale set never leaks.
  const [couplesExpanded, setCouplesExpanded] = useState<Set<string>>(
    new Set(),
  );
  const ordered = sortPerformance(rows, sort, mode);
  // A metric's axis tops out at the chosen level's high statistic over the
  // entities it covers — `max`, `fence_high` or `q3` (#811). `1` as a floor so
  // an all-zero metric never divides by zero.
  const axisMax = (entities: StatsPerformanceEntity[], field: PerformanceField) =>
    Math.max(
      1,
      ...entities.flatMap((row) =>
        row.harnesses.map((item) => item[field].stats?.[ZOOM_HIGH[zoom]] ?? 0),
      ),
    );
  const fieldByMetric = (metric: PerformanceMetric) => metricField(metric, mode);
  const scalesOf = (
    entities: StatsPerformanceEntity[],
  ): Record<PerformanceMetric, number> => ({
    context: axisMax(entities, "context"),
    // #810: each reading has its own max — zooming out to the wall-clock scale
    // would leave every active (or waiting) box crushed against the left edge.
    duration: axisMax(entities, fieldByMetric("duration")),
    steering: axisMax(entities, "steering"),
  });
  // The shared scale is drawn from the rows and their subagents — a couple's
  // peaks/durations come from those same session files, so they fit it. The
  // independent one applies the very same rule one row at a time, subagents
  // folded into their Node's row (issue AC).
  const sharedScales = scalesOf(rows.flatMap((row) => [row, ...row.subagents]));
  const scalesByRow = new Map(
    ordered.map((row) => [row.id, scalesOf([row, ...row.subagents])] as const),
  );
  const scalesFor = (rowId: string) =>
    independentAxis ? (scalesByRow.get(rowId) ?? sharedScales) : sharedScales;

  const visible = ordered.flatMap((row) => [
    { row, child: false, scales: scalesFor(row.id) },
    ...(expanded.has(row.id)
      ? sortPerformance(row.subagents, sort, mode).map((child) => ({
          row: child,
          child: true,
          // A subagent reads its parent's axis: it belongs to that Node's row.
          scales: scalesFor(row.id),
        }))
      : []),
  ]);

  const metricCell = (
    name: string,
    rowHarnesses: StatsHarnessPerformance[] | undefined,
    metric: PerformanceMetric,
    scales: Record<PerformanceMetric, number>,
  ) => {
    const field = fieldByMetric(metric);
    return (
      <td key={metric} className="py-2 pr-3 align-top">
        <div className="grid gap-1.5">
          {harnesses.map((harness) => {
            const item = rowHarnesses?.find(
              (entry) => entry.harness === harness,
            );
            // What the active reading took off this row × harness, on the value
            // the row displays. Shown only when it is not zero: a node nobody
            // ever waited on carries no annotation. Waiting mode shows no
            // delta — the wait IS the value there, not something removed.
            const wait =
              field === "active_duration" &&
              item?.duration.stats &&
              item.active_duration.stats
                ? item.duration.stats.median - item.active_duration.stats.median
                : 0;
            return (
              <div key={harness} className="flex items-start gap-2">
                <span
                  className="mt-1 h-[7px] w-[7px] shrink-0 rounded-full"
                  style={{ backgroundColor: harnessColor(harness) }}
                />
                <DistributionPlot
                  name={name}
                  harness={harness}
                  field={field}
                  value={
                    item?.[field] ?? {
                      stats: null,
                      measured: 0,
                      expected: 0,
                      missing_reasons: [`never ran on ${harness}`],
                    }
                  }
                  scaleMax={scales[metric]}
                  zoom={zoom}
                  rowMax={independentAxis ? scales[metric] : null}
                  steered={item?.steered}
                  waitMillis={wait > 0 ? wait : undefined}
                  ghost={wait > 0 ? (item?.duration.stats ?? null) : null}
                />
              </div>
            );
          })}
        </div>
      </td>
    );
  };

  return (
    <TooltipProvider>
      <table className="w-full table-fixed text-left" style={{ fontSize: "11px" }}>
        <thead className="text-fg-4">
          <tr>
            <th className="w-48 pb-2 font-medium">Name</th>
            <th className="pb-2 font-medium">Context (peak tokens)</th>
            <th
              className="pb-2 font-medium"
              data-testid="stats-performance-duration-header"
            >
              {mode === "total" ? (
                "Duration"
              ) : (
                <span className="inline-flex items-center gap-1">
                  {PERFORMANCE_FIELD_LABEL[MODE_FIELD[mode]]}
                  <Tooltip content={DECLARED_WAIT_COPY} side="top">
                    <span
                      role="img"
                      aria-label={DECLARED_WAIT_COPY}
                      className="inline-flex text-fg-4"
                    >
                      <Info size={11} />
                    </span>
                  </Tooltip>
                </span>
              )}
            </th>
            <th className="pb-2 font-medium">
              <span className="inline-flex items-center gap-1">
                Steering (messages / execution)
                <Tooltip content={STEERING_PROVENANCE_COPY} side="top">
                  <span
                    role="img"
                    aria-label={STEERING_PROVENANCE_COPY}
                    className="inline-flex text-fg-4"
                  >
                    <Info size={11} />
                  </span>
                </Tooltip>
              </span>
            </th>
          </tr>
        </thead>
        <tbody>
          {visible.map(({ row, child, scales }) => {
            const couples = !child && expandablePairs ? (row.models ?? []) : [];
            return (
              <Fragment key={`${child ? "subagent" : "entity"}-${row.id}`}>
                <tr className="border-t border-line" data-testid="stats-detail-row">
                  <td className={`py-2 pr-2 text-fg-2 ${child ? "pl-7" : ""}`}>
                    <span className="inline-flex items-center gap-1">
                      {couples.length > 0 ? (
                        <button
                          type="button"
                          aria-label={`${couplesExpanded.has(row.id) ? "Collapse" : "Expand"} ${row.name} models`}
                          data-testid="stats-node-toggle"
                          onClick={() =>
                            setCouplesExpanded((current) => {
                              const next = new Set(current);
                              if (next.has(row.id)) next.delete(row.id);
                              else next.add(row.id);
                              return next;
                            })
                          }
                          className="shrink-0 hover:text-fg"
                        >
                          {couplesExpanded.has(row.id) ? (
                            <ChevronDown size={12} />
                          ) : (
                            <ChevronRight size={12} />
                          )}
                        </button>
                      ) : null}
                      {!child && row.subagents.length > 0 ? (
                        <button
                          type="button"
                          aria-label={`${expanded.has(row.id) ? "Collapse" : "Expand"} ${row.name} subagents`}
                          onClick={() =>
                            setExpanded((current) => {
                              const next = new Set(current);
                              if (next.has(row.id)) next.delete(row.id);
                              else next.add(row.id);
                              return next;
                            })
                          }
                          className="shrink-0 hover:text-fg"
                        >
                          {expanded.has(row.id) ? (
                            <ChevronDown size={12} />
                          ) : (
                            <ChevronRight size={12} />
                          )}
                        </button>
                      ) : null}
                      {(() => {
                        const content = renderName ? renderName(row) : row.name;
                        return onOpen ? (
                          <button
                            type="button"
                            aria-label={`Open ${row.name}`}
                            onClick={() => onOpen(row)}
                            className="text-left hover:text-fg"
                          >
                            {content}
                          </button>
                        ) : (
                          <span>{content}</span>
                        );
                      })()}
                      {!child && <KindBadges row={row} />}
                    </span>
                  </td>
                  {filteredRowIds?.has(row.id)
                    ? PERFORMANCE_METRICS.map((metric) => (
                        <td key={metric} className="py-2 pr-3 align-top">
                          <FilteredValue testid="stats-performance-filtered-cell" />
                        </td>
                      ))
                    : PERFORMANCE_METRICS.map((metric) =>
                        metricCell(row.name, row.harnesses, metric, scales),
                      )}
                </tr>
                {couplesExpanded.has(row.id)
                  ? couples.map((pair) => (
                      <tr
                        key={`${row.id}-${pair.model}-${pair.effort ?? ""}`}
                        className="border-t border-line bg-bg-3/40 text-fg-3"
                        data-testid="stats-performance-model-effort-row"
                      >
                        <td className="py-2 pl-7 pr-2">
                          <span className="inline-flex items-baseline gap-1">
                            <ProvenanceName
                              name={pair.model}
                              mono
                              provenance={pair.model_provenance}
                              harnesses={[]}
                              target="model"
                              content={performanceProvenanceCopy(pair.model_provenance)}
                            />
                            <span className="text-fg-4">·</span>
                            {pair.effort === null ? (
                              <Tooltip content={NOT_SET_COPY} side="top">
                                <span className="italic text-fg-4">not set</span>
                              </Tooltip>
                            ) : (
                              <ProvenanceName
                                name={pair.effort}
                                provenance={pair.effort_provenance}
                                harnesses={[]}
                                target="effort"
                                content={performanceProvenanceCopy(pair.effort_provenance)}
                              />
                            )}
                          </span>
                        </td>
                        {PERFORMANCE_METRICS.map((metric) =>
                          metricCell(
                            `${pair.model} · ${pair.effort ?? "not set"}`,
                            pair.harnesses,
                            metric,
                            scales,
                          ),
                        )}
                      </tr>
                    ))
                  : null}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </TooltipProvider>
  );
}

type PerformanceAxis = "pipeline" | "model";

// --- The filter band (#819) ----------------------------------------------------

/** Every tab's filters live at the top of its own pane, where their effect is
 *  read — the shell bar keeps the period alone. One frame, one legend naming
 *  the tab, controls left → right in the order the numbers depend on them:
 *  which Runs, which duration, which Nodes, how to draw them. */
function StatsFilterBand({
  label,
  testid,
  children,
  onReset,
}: {
  label: string;
  testid: string;
  children: React.ReactNode;
  /** `undefined` until the band deviates from the tab's defaults: a reset that
   *  resets nothing is noise. */
  onReset?: () => void;
}) {
  return (
    <fieldset
      className="mb-3 rounded-md border border-line px-3 pb-2 pt-1"
      data-testid={testid}
      style={{ fontSize: "11px" }}
    >
      <legend
        className="px-1 uppercase tracking-wide text-st-done"
        style={{ fontSize: "9px" }}
      >
        {label}
      </legend>
      <div className="flex flex-wrap items-center gap-2">
        {children}
        {onReset && (
          <button
            type="button"
            data-testid="stats-reset-filters"
            onClick={onReset}
            className="ml-auto text-fg-4 underline decoration-dotted underline-offset-2 hover:text-fg-2"
          >
            reset filters
          </button>
        )}
      </div>
    </fieldset>
  );
}

/** « Runs terminés seulement », the cohort of ONE tab (#819). Ticking it on
 *  Performance leaves Cost and Overview exactly where they were. */
function CohortChip({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      data-testid="stats-completed-only"
      onClick={() => onChange(!checked)}
      className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 ${
        checked
          ? "border-st-done bg-st-done/15 text-fg"
          : "border-line-strong bg-bg-3 text-fg-2"
      }`}
    >
      {checked && <Check size={10} strokeWidth={3} aria-hidden="true" />}
      completed runs only
    </button>
  );
}

const DURATION_MODE_LABEL: Record<DurationMode, string> = {
  total: "Total",
  active: "Active",
  waiting: "Waiting",
};

/** The **mode de durée** (#819): three readings of the same executions, all
 *  three visible at once and one click apart. Same radiogroup idiom as the zoom
 *  control beside it — ← → walk the modes, roving tabindex — because a select
 *  would hide two of the three states behind a click. */
function DurationModeSegments({
  value,
  onChange,
}: {
  value: DurationMode;
  onChange: (mode: DurationMode) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label="Duration mode"
      data-testid="stats-duration-mode"
      className="flex items-center gap-0.5 rounded border border-line bg-bg-3 p-0.5"
      onKeyDown={(event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
        event.preventDefault();
        const delta = event.key === "ArrowRight" ? 1 : -1;
        const index = DURATION_MODES.indexOf(value);
        const next =
          DURATION_MODES[
            (index + delta + DURATION_MODES.length) % DURATION_MODES.length
          ];
        onChange(next);
        event.currentTarget
          .querySelector<HTMLButtonElement>(`[data-duration-mode="${next}"]`)
          ?.focus();
      }}
    >
      {DURATION_MODES.map((mode) => {
        const selected = mode === value;
        return (
          <button
            key={mode}
            type="button"
            role="radio"
            aria-checked={selected}
            data-duration-mode={mode}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(mode)}
            className={`rounded px-2 py-0.5 transition-colors ${
              selected ? "bg-bg-5 text-fg" : "text-fg-3 hover:text-fg-2"
            }`}
          >
            {DURATION_MODE_LABEL[mode]}
          </button>
        );
      })}
    </div>
  );
}

/** The wait coverage (#819), beside the mode in Active and Waiting only: in
 *  Total there is nothing to qualify. A description, not a control — hover or
 *  focus opens the same sentence, which explains why a whole period can read
 *  Active = Total and Waiting = 0. */
function WaitCoverage({
  waited,
  executions,
}: {
  waited: number;
  executions: number;
}) {
  const copy = `${waited} executions of ${executions} declared at least one wait — for the others, Active = Total and Waiting = 0. ${DECLARED_WAIT_COPY}`;
  return (
    <Tooltip content={copy} side="top">
      <span
        role="img"
        aria-label={copy}
        data-testid="stats-wait-coverage"
        className="inline-flex text-fg-4"
      >
        <Info size={12} />
      </span>
    </Tooltip>
  );
}

/** The node kind chips (#810): a multi-selection, every kind checked by default,
 *  with the count of Node rows each covers. */
function NodeKindChips({
  counts,
  nodeKinds,
  onNodeKindsChange,
}: {
  counts: Record<NodeKind, number>;
  nodeKinds: NodeKind[];
  onNodeKindsChange: (kinds: NodeKind[]) => void;
}) {
  const toggleKind = (kind: NodeKind) =>
    onNodeKindsChange(
      nodeKinds.includes(kind)
        ? nodeKinds.filter((item) => item !== kind)
        : [...ALL_NODE_KINDS].filter(
            (item) => item === kind || nodeKinds.includes(item),
          ),
    );
  return (
    <>
      <span className="ml-1 text-fg-4">Node kind</span>
      {ALL_NODE_KINDS.map((kind) => {
        const on = nodeKinds.includes(kind);
        return (
          <button
            key={kind}
            type="button"
            aria-pressed={on}
            data-testid={`stats-node-kind-chip-${kind}`}
            onClick={() => toggleKind(kind)}
            className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 capitalize ${
              on
                ? "border-st-done bg-st-done/15 text-fg"
                : "border-line bg-bg-3 text-fg-4 line-through"
            }`}
          >
            {on && <Check size={10} strokeWidth={3} aria-hidden="true" />}
            {kind}
            <span className="font-mono text-fg-4">{counts[kind]}</span>
          </button>
        );
      })}
    </>
  );
}

/** A thin divider between two groups of the band, so « which Runs » and « which
 *  duration » never read as one control. */
function BandDivider() {
  return <span className="mx-1 h-4 w-px bg-line-strong" aria-hidden="true" />;
}

/** The band of a section that filters on the cohort alone (#819): Overview,
 *  Sessions, Triggers — which share one cohort, reading one response — and
 *  Cost, which owns its own. One control whose default is « off », so unticking
 *  IS the reset and no reset link is needed. */
function CohortOnlyBand({
  label,
  completedOnly,
  onCompletedOnlyChange,
}: {
  label: string;
  completedOnly: boolean;
  onCompletedOnlyChange: (value: boolean) => void;
}) {
  return (
    <StatsFilterBand label={label} testid="stats-filter-band">
      <CohortChip checked={completedOnly} onChange={onCompletedOnlyChange} />
      <span className="text-fg-4">default: all runs</span>
    </StatsFilterBand>
  );
}

function PerformanceTab({
  performance,
  error,
  completedOnly,
  onCompletedOnlyChange,
  band,
  onBandChange,
  onResetFilters,
  onAbsorptionsChanged,
}: {
  performance: StatsPerformance | null;
  error: string | null;
  completedOnly: boolean;
  onCompletedOnlyChange: (value: boolean) => void;
  /** The band's controls, owned by the shell so a tab switch keeps them (#819). */
  band: PerformanceBand;
  onBandChange: (band: PerformanceBand) => void;
  onResetFilters: () => void;
  onAbsorptionsChanged: () => void;
}) {
  // The second select (#737): grouping (« By pipeline » / « By model »), fully
  // independent of the sort (« By context » / « By duration ») beside it.
  const [axis, setAxis] = useState<PerformanceAxis>("pipeline");
  const [sort, setSort] = useState<PerformanceMetric>("context");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // The model axis drills Model → Effort → Pipeline → Node (ADR-0065). The
  // effort id is "" for the "not set" bucket, so selection is `null` vs value,
  // never falsy-compared.
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const [selectedEffortId, setSelectedEffortId] = useState<string | null>(null);
  const [selectedPipelineId, setSelectedPipelineId] = useState<string | null>(null);
  // #819 — the band's controls come from the shell, which outlives the section:
  // they survive the drill, the grouping switch, the table's keyed remount AND
  // a trip to another tab, while a fresh Stats always opens on the defaults.
  const { durationMode: mode, nodeKinds, zoom, axis: axisMode } = band;
  const changeZoom = (next: StatsZoom) => onBandChange({ ...band, zoom: next });
  const changeAxisMode = (next: StatsAxis) =>
    onBandChange({ ...band, axis: next });
  const onDurationModeChange = (next: DurationMode) =>
    onBandChange({ ...band, durationMode: next });
  const onNodeKindsChange = (kinds: NodeKind[]) =>
    onBandChange({ ...band, nodeKinds: kinds });
  // #890: the « By pipeline » Pipeline rows are the ones to combine — never the
  // Infrastructure row beside them, never the « By model » axis.
  const absorption = usePipelineAbsorption({
    rows: performance?.by_pipeline ?? [],
    enabled: axis === "pipeline",
    count: RUNS_COUNT,
    onChanged: onAbsorptionsChanged,
  });

  if (error) {
    return (
      <div className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed">
        {error}
      </div>
    );
  }
  if (!performance) return <EmptyNote>Loading performance…</EmptyNote>;

  const toTotal = () => {
    setSelectedId(null);
    setSelectedModelId(null);
    setSelectedEffortId(null);
    setSelectedPipelineId(null);
  };

  // --- Node kind filter (#810) ------------------------------------------------
  const kindCounts = nodeKindCounts(performance.by_pipeline);
  const kindFilterActive = nodeKinds.length < ALL_NODE_KINDS.length;
  const anyHidden =
    kindFilterActive &&
    performance.by_pipeline.some((row) => hasHiddenNode(row, nodeKinds));
  const visibleNodes = (rows: StatsPerformanceEntity[]) =>
    rows.filter((row) => matchesKinds(row, nodeKinds));
  // The band, read left → right in the order the numbers depend on it: which
  // Runs, which duration (and how many executions qualify), which Nodes, how
  // the plots are drawn.
  const filterStrip = (
    <StatsFilterBand
      label="Performance filters"
      testid="stats-performance-filters"
      onReset={
        performanceBandDeviates(completedOnly, band) ? onResetFilters : undefined
      }
    >
      <CohortChip checked={completedOnly} onChange={onCompletedOnlyChange} />
      <BandDivider />
      <span className="text-fg-4">Duration</span>
      <DurationModeSegments value={mode} onChange={onDurationModeChange} />
      {mode !== "total" && (
        <WaitCoverage
          waited={performance.waited_executions ?? 0}
          executions={performance.executions ?? 0}
        />
      )}
      <BandDivider />
      <NodeKindChips
        counts={kindCounts}
        nodeKinds={nodeKinds}
        onNodeKindsChange={onNodeKindsChange}
      />
      <BandDivider />
      <ZoomSegments value={zoom} onChange={changeZoom} />
      <IndependentScalesSwitch
        checked={axisMode === "independent"}
        onChange={(checked) =>
          changeAxisMode(checked ? "independent" : "shared")
        }
      />
    </StatsFilterBand>
  );

  // Every empty state keeps the band above it: the cohort now lives there, so a
  // pane emptied BY the cohort must still carry the one control that refills it
  // (a fresh instance with no completed run would otherwise be a dead end).
  const emptyPane = (note: React.ReactNode) => (
    <TooltipProvider>
      <div data-testid="stats-chart-performance">
        {filterStrip}
        {note}
      </div>
    </TooltipProvider>
  );

  if (nodeKinds.length === 0) {
    return emptyPane(
      <NoNodeOfSelectedKinds
        where="in this period"
        onShowAll={() => onNodeKindsChange([...ALL_NODE_KINDS])}
      />,
    );
  }
  if (
    performance.by_pipeline.length === 0 &&
    performance.infrastructure.length === 0 &&
    performance.by_model.length === 0
  ) {
    return emptyPane(
      <EmptyNote>No successful executions in this period.</EmptyNote>,
    );
  }
  // A period whose observations all lack a resolvable model is an empty axis —
  // its own absence, not a broken tab.
  if (axis === "model" && performance.by_model.length === 0) {
    return emptyPane(<EmptyNote>No model observed in this period.</EmptyNote>);
  }

  const infrastructureRow: StatsPerformanceEntity = {
    id: "__infrastructure__",
    name: "Infrastructure",
    ...performance.infrastructure_total,
    nodes: performance.infrastructure,
    subagents: [],
  };

  // Model axis: resolve the drill path.
  const model =
    axis === "model"
      ? (performance.by_model.find((row) => row.id === selectedModelId) ?? null)
      : null;
  const effort =
    model && selectedEffortId !== null
      ? (model.efforts.find((row) => row.id === selectedEffortId) ?? null)
      : null;
  const modelPipeline =
    effort && selectedPipelineId !== null
      ? (effort.pipelines.find((row) => row.id === selectedPipelineId) ?? null)
      : null;

  // A Pipeline whose every node is hidden leaves the master list; one that was
  // selected then falls back to Total, because `selected` is looked up in the
  // filtered rows.
  const pipelineRows = performance.by_pipeline.filter(
    (row) =>
      row.nodes.length === 0 ||
      row.nodes.some((node) => matchesKinds(node, nodeKinds)),
  );
  const masterRows =
    axis === "model"
      ? sortPerformance(performance.by_model, sort, mode)
      : sortPerformance(
          [...pipelineRows, infrastructureRow],
          sort,
          mode,
        );
  const selected =
    axis === "pipeline" ? (masterRows.find((row) => row.id === selectedId) ?? null) : null;
  const aggregate =
    axis === "model"
      ? (modelPipeline ?? effort ?? model ?? performance.total)
      : (selected ?? performance.total);

  // Is what the head cards, the headline and the Steered card summarise still
  // the population on screen? Infrastructure is never filtered by kind, so a
  // selected Infrastructure row always reads its own true numbers.
  const partialByKind =
    axis === "model"
      ? anyHidden
      : selected === null
        ? anyHidden
        : selected.id === "__infrastructure__"
          ? false
          : hasHiddenNode(selected, nodeKinds);

  let detailRows: StatsPerformanceEntity[];
  // The Node population the level would show without the kind filter — set only
  // on a Node level, so an empty table can tell « nothing here » from « the
  // filter took everything » (#810).
  let unfilteredNodes: StatsPerformanceEntity[] | null = null;
  let detailRenderName: ((row: StatsPerformanceEntity) => React.ReactNode) | undefined;
  let onOpen: ((row: StatsPerformanceEntity) => void) | undefined;
  if (axis === "model") {
    if (modelPipeline) {
      // Node leaves are the floor: the model × effort path is the drill.
      unfilteredNodes = modelPipeline.nodes;
      detailRows = visibleNodes(modelPipeline.nodes);
    } else if (effort) {
      detailRows = effort.pipelines;
      onOpen = (row) => setSelectedPipelineId(row.id);
    } else if (model) {
      detailRows = model.efforts;
      detailRenderName = (row) => {
        const match = model.efforts.find((item) => item.id === row.id);
        return match ? performanceEffortName(match) : row.name;
      };
      onOpen = (row) => setSelectedEffortId(row.id);
    } else {
      detailRows = performance.by_model;
      detailRenderName = (row) => {
        const provenance = performance.by_model.find((m) => m.id === row.id)?.provenance;
        return (
          <ProvenanceName
            name={row.name}
            mono
            provenance={provenance}
            harnesses={[]}
            target="model"
            content={performanceProvenanceCopy(provenance)}
          />
        );
      };
      onOpen = (row) => setSelectedModelId(row.id);
    }
  } else if (selected) {
    if (selected.id === "__infrastructure__") {
      detailRows = performance.infrastructure;
    } else {
      unfilteredNodes = selected.nodes;
      detailRows = visibleNodes(selected.nodes);
    }
  } else {
    detailRows = masterRows;
    detailRenderName = (row) => (
      <PipelineName row={row} onOpenMembers={absorption.openMembers} />
    );
  }

  // A Node level the filter emptied: the table would stand there with its
  // headers and not a row, saying nothing about why. Same state, same way out
  // as every chip off.
  const detailEmptiedByKind =
    detailRows.length === 0 && (unfilteredNodes?.length ?? 0) > 0;

  // Rows whose own aggregate no longer describes what is visible under them.
  // Never a value recomputed from the visible nodes' six stats: a median of
  // medians is not a median (spec #809).
  const filteredRowIds = new Set<string>();
  if (kindFilterActive) {
    if (axis === "pipeline") {
      for (const row of performance.by_pipeline) {
        if (hasHiddenNode(row, nodeKinds)) filteredRowIds.add(row.id);
      }
    } else if (anyHidden) {
      for (const row of detailRows) {
        if (!isNodeRow(row)) filteredRowIds.add(row.id);
      }
    }
  }

  const durationField = metricField("duration", mode);
  const contexts = aggregate.harnesses.map((item) =>
    item.context.stats ? formatPerformanceValue(item.context.stats.median, "context") : "—",
  );
  const durations = aggregate.harnesses.map((item) =>
    item[durationField].stats
      ? formatPerformanceValue(item[durationField].stats!.median, "duration")
      : "—",
  );
  const steered = aggregate.harnesses.map(
    (item) => steeredPercent(item.steered) ?? "—",
  );
  const headlineValue = (values: string[]) =>
    partialByKind ? <FilteredValue /> : <>{values.join(" / ") || "—"}</>;

  // The model axis's breadcrumb; « By pipeline » keeps its one-line header.
  const crumbs: { label: string; onClick?: () => void }[] = [
    { label: "Total", onClick: toTotal },
  ];
  if (axis === "model") {
    if (model)
      crumbs.push({
        label: model.name,
        onClick: () => {
          setSelectedEffortId(null);
          setSelectedPipelineId(null);
        },
      });
    if (effort) crumbs.push({ label: effort.name, onClick: () => setSelectedPipelineId(null) });
    if (modelPipeline) crumbs.push({ label: modelPipeline.name });
  }
  // Every crumb but the last pops the levels it shadows.
  const clickableCrumbs = crumbs.map((crumb, index) =>
    index === crumbs.length - 1 ? { label: crumb.label } : crumb,
  );

  return (
    // #810 — the band, the master-list « filtered » marks and the card « i » all
    // carry tooltips outside `PerformanceTable`'s own provider.
    <TooltipProvider>
      <div
        className="relative flex min-h-full flex-col"
        data-testid="stats-chart-performance"
      >
        {/* #819 — the band spans the whole pane, above the ranking AND the
            detail: it governs both, and a control that governs the master list
            must not sit beside it. */}
        {filterStrip}
        <div className="flex min-h-0 flex-1">
          <aside className="w-[290px] shrink-0 border-r border-line pr-4">
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                Ranked by {performanceSortLabel(sort, mode)} (median)
              </span>
              <span className="flex items-center gap-1.5">
                <select
                  aria-label="Performance grouping"
                  value={axis}
                  onChange={(event) => {
                    setAxis(event.target.value as PerformanceAxis);
                    toTotal();
                  }}
                  className="rounded border border-line bg-bg-3 px-2 py-1 text-fg-2"
                >
                  <option value="pipeline">By pipeline</option>
                  <option value="model">By model</option>
                </select>
                <select
                  aria-label="Performance sort"
                  value={sort}
                  onChange={(event) =>
                    setSort(event.target.value as PerformanceMetric)
                  }
                  className="rounded border border-line bg-bg-3 px-2 py-1 text-fg-2"
                >
                  <option value="context">By context</option>
                  <option value="duration">
                    By {durationWord(mode)}
                  </option>
                  <option value="steering">By steering</option>
                </select>
              </span>
            </div>
            <MasterList
              rows={masterRows}
              selection={absorption.selection}
              selected={axis === "model" ? selectedModelId : selectedId}
              monoName={axis === "model"}
              ariaLabel="Performance groups"
              valueLabel={(row) => {
                if (
                  filteredRowIds.has(row.id) ||
                  (axis === "model" && anyHidden)
                ) {
                  return <FilteredValue />;
                }
                const [median] = performanceScore(row, sort, mode);
                return median < 0 ? "—" : formatPerformanceValue(median, sort);
              }}
              onSelect={(id) => {
                if (axis === "model") {
                  setSelectedModelId(id);
                  setSelectedEffortId(null);
                  setSelectedPipelineId(null);
                } else {
                  setSelectedId(id);
                }
              }}
            />
            {axis === "model" && (
              <div className="mt-3 text-fg-4" style={{ fontSize: "10.5px" }}>
                Model ids verbatim, one row per id — the same id run through two
                harnesses is one row, one column per harness. Hover a model or an
                effort for where the value was read.
              </div>
            )}
          </aside>
          <div className="min-w-0 flex-1 pl-5">
            {axis === "model" ? (
              <Breadcrumb
                testid="stats-performance-breadcrumb"
                crumbs={clickableCrumbs}
              />
            ) : (
              <div className="mb-3 text-fg-4" style={{ fontSize: "10.5px" }}>
                Total{selected ? ` / ${selected.name}` : ""}
              </div>
            )}
            <HarnessLegend harnesses={performance.harnesses} />
            <div
              className="mt-4 text-fg"
              data-testid="stats-performance-headline"
            >
              {headlineValue(contexts)} median peak context ·{" "}
              {headlineValue(durations)} median {durationWord(mode)} ·{" "}
              {headlineValue(steered)} steered
            </div>
            <div className="mt-4">
              <PerformanceCards
                aggregate={aggregate}
                mode={mode}
                partialByKind={partialByKind}
              />
            </div>
            <div className="mt-4">
              <CohortLine completedOnly={completedOnly} />
            </div>
            {/* #819 — the zoom and the scales moved up into the band; what is
                left here is the caption that says what the plots below draw. */}
            <div
              className="mt-5 text-fg-4"
              data-testid="stats-performance-toolbar"
              style={{ fontSize: "10.5px" }}
            >
              Distributions per node ·{" "}
              {axisMode === "independent"
                ? "each row on its own axis"
                : `shared axis per metric, capped at ${ZOOM_CAP_COPY[zoom]}`}
            </div>
            <div className="mt-2 min-h-[240px]">
              {detailEmptiedByKind ? (
                <NoNodeOfSelectedKinds
                  where="at this level"
                  onShowAll={() => onNodeKindsChange([...ALL_NODE_KINDS])}
                />
              ) : (
                <PerformanceTable
                  key={`${axis}-${selectedId ?? ""}-${selectedModelId ?? "total"}-${selectedEffortId ?? "total"}-${selectedPipelineId ?? ""}`}
                  rows={detailRows}
                  harnesses={performance.harnesses}
                  sort={sort}
                  mode={mode}
                  zoom={zoom}
                  independentAxis={axisMode === "independent"}
                  renderName={detailRenderName}
                  onOpen={onOpen}
                  expandablePairs={axis === "pipeline"}
                  filteredRowIds={filteredRowIds}
                />
              )}
            </div>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
}

export interface StatsChartsProps {
  tab: StatsTab;
  overview: StatsOverview | null;
  cost: StatsCost | null;
  costError: string | null;
  performance?: StatsPerformance | null;
  performanceError?: string | null;
  /** The cohort of the tab on screen (#819) — the chip in its band, and the
   *  cohort line under it. The narrowing itself happened in the daemon. */
  completedOnly?: boolean;
  onCompletedOnlyChange?: (value: boolean) => void;
  /** The Performance band's other controls, owned by the shell so a trip to
   *  another tab keeps them (#819). Defaulted so a caller that does not care
   *  gets the surface's own defaults. */
  band?: PerformanceBand;
  onBandChange?: (band: PerformanceBand) => void;
  onResetFilters?: () => void;
  /** A Combine or an Uncombine landed (#890): the host refetches every tab. */
  onAbsorptionsChanged?: () => void;
}

/** The legend each tab's band carries — it names the tab, not the endpoint. */
const TAB_BAND_LABEL: Record<Exclude<StatsTab, "performance">, string> = {
  runs: "Overview filters",
  sessions: "Sessions filters",
  triggers: "Triggers filters",
  cost: "Cost filters",
};

export default function StatsCharts({
  tab,
  overview,
  cost,
  costError,
  performance = null,
  performanceError = null,
  completedOnly = false,
  onCompletedOnlyChange = () => {},
  band = DEFAULT_PERFORMANCE_BAND,
  onBandChange = () => {},
  onResetFilters = () => {},
  onAbsorptionsChanged = () => {},
}: StatsChartsProps) {
  // #759: subscribing here re-renders the whole chart subtree on a theme switch,
  // so every `CHART.*` / `harnessColor()` read below resolves against the new
  // palette. Recharts bakes colours into props; nothing re-themes on its own.
  useTheme();

  if (tab === "performance") {
    // Performance places its band and its cohort line itself — the band above
    // the whole pane, the line under its head cards.
    return (
      <PerformanceTab
        performance={performance}
        error={performanceError}
        completedOnly={completedOnly}
        onCompletedOnlyChange={onCompletedOnlyChange}
        band={band}
        onBandChange={onBandChange}
        onResetFilters={onResetFilters}
        onAbsorptionsChanged={onAbsorptionsChanged}
      />
    );
  }
  const body = () => {
    if (tab === "cost")
      return (
        <CostTab cost={cost} error={costError} onAbsorptionsChanged={onAbsorptionsChanged} />
      );
    if (!overview) return <EmptyNote>Loading…</EmptyNote>;
    if (tab === "runs")
      return <RunsTab overview={overview} completedOnly={completedOnly} />;
    if (tab === "sessions")
      return <SessionsTab overview={overview} onAbsorptionsChanged={onAbsorptionsChanged} />;
    return <TriggersTab overview={overview} />;
  };
  return (
    <TooltipProvider>
      <div className="flex min-h-full flex-col gap-3">
        <CohortOnlyBand
          label={TAB_BAND_LABEL[tab]}
          completedOnly={completedOnly}
          onCompletedOnlyChange={onCompletedOnlyChange}
        />
        <CohortLine
          completedOnly={completedOnly}
          note={
            // Trigger fires are not Runs: the cohort selects which Runs are
            // counted, and says so rather than implying it filtered the fires.
            tab === "triggers"
              ? "trigger fires are not filtered by run status"
              : undefined
          }
        />
        <div className="min-h-0 flex-1">{body()}</div>
      </div>
    </TooltipProvider>
  );
}
