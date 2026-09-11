import { Fragment, useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
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
  StatsSessionEntity,
  StatsSessionHarness,
  StatsSessionPeriod,
} from "../types";
import { formatCostAmount } from "../lib/costLabel";
import { harnessColor } from "../lib/harness";
import { cssColor } from "../lib/cssColor";
import { useTheme } from "../hooks/useTheme";
import { Tooltip, TooltipProvider } from "./ui/tooltip";

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

function RunsTab({ overview }: { overview: StatsOverview }) {
  if (overview.buckets.length === 0) return <EmptyNote>No runs in this period.</EmptyNote>;
  const runs = new Map(overview.runs.map((row) => [row.bucket, row.count]));
  const errors = new Map(overview.errors.map((row) => [row.bucket, row.count]));
  const data = overview.buckets.map((bucket) => ({
    bucket,
    runs: runs.get(bucket) ?? 0,
    errors: errors.get(bucket) ?? 0,
  }));
  return (
    <div data-testid="stats-chart-runs">
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

function MasterList<T extends { id: string; name: string }>({
  rows,
  selected,
  valueLabel,
  onSelect,
  ariaLabel = "Spenders",
  monoName = false,
}: {
  rows: T[];
  selected: string | null;
  valueLabel: (row: T) => string;
  onSelect: (id: string | null) => void;
  ariaLabel?: string;
  /** Model ids are ids — render them mono (ADR-0065 §2). */
  monoName?: boolean;
}) {
  const options = [{ id: "__total__", name: "Total" } as T, ...rows];
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
        }
      }}
    >
      {options.map((row, index) => {
        const isSelected = row.id === (selected ?? "__total__");
        return (
          <button
            key={row.id}
            type="button"
            role="option"
            aria-selected={isSelected}
            tabIndex={index === activeFocusIndex ? 0 : -1}
            onFocus={() => setFocusIndex(index)}
            onClick={() => onSelect(row.id === "__total__" ? null : row.id)}
            className={`flex items-center justify-between gap-3 rounded px-2 py-2 text-left ${
              isSelected ? "bg-bg-5 text-fg" : "text-fg-3 hover:bg-bg-3"
            }`}
            style={{ fontSize: "11.5px" }}
          >
            <span className={`truncate ${monoName ? "font-mono" : ""}`}>{row.name}</span>
            <span className="shrink-0 font-mono text-fg-2">
              {row.id === "__total__" ? "" : valueLabel(row)}
            </span>
          </button>
        );
      })}
    </div>
  );
}

function SessionsTab({ overview }: { overview: StatsOverview }) {
  const rows = useMemo(
    () => [...overview.sessions_by_pipeline].sort((a, b) => b.executions - a.executions),
    [overview.sessions_by_pipeline],
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = rows.find((row) => row.id === selectedId) ?? null;
  const periods = selected ? selected.by_period : overview.sessions_by_period;
  const detailRows = selected?.nodes ?? rows;

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
          />
        </div>
        <div className="min-w-0 flex-1">
          <div className="mb-3 text-fg-4" style={{ fontSize: "10.5px" }}>
            Total{selected ? ` / ${selected.name}` : ""}
          </div>
          <SessionTable rows={detailRows} harnesses={overview.session_harnesses} />
        </div>
      </div>
    </div>
  );
}

function SessionTable({ rows, harnesses }: { rows: StatsSessionEntity[]; harnesses: string[] }) {
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
            <td className="py-2 pr-2">{row.name}</td>
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
            {metric.average_usd === null
              ? "— avg"
              : `${formatCostAmount(metric.average_usd, metric.partial, metric.estimated)} avg`}
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
          {metric.average_usd === null
            ? "— avg"
            : `${formatCostAmount(metric.average_usd, metric.partial, metric.estimated)} avg`}
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
}: {
  cost: StatsCost | null;
  error: string | null;
}) {
  const [axis, setAxis] = useState<CostAxis>("pipeline");
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
          {formatCostAmount(aggregate.average_usd, aggregate.partial, aggregate.estimated)} per{" "}
          {detailUnit}
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
    </div>
  );
}

type PerformanceMetric = "context" | "duration";

function performanceScore(
  aggregate: StatsPerformanceAggregate,
  metric: PerformanceMetric,
): [number, number] {
  const distributions = aggregate.harnesses
    .map((item) => item[metric])
    .filter((item) => item.stats !== null);
  return [
    Math.max(-1, ...distributions.map((item) => item.stats!.mean)),
    Math.max(-1, ...distributions.map((item) => item.stats!.median)),
  ];
}

function sortPerformance<T extends StatsPerformanceAggregate & { name: string }>(
  rows: T[],
  metric: PerformanceMetric,
): T[] {
  return [...rows].sort((a, b) => {
    const [aMean, aMedian] = performanceScore(a, metric);
    const [bMean, bMedian] = performanceScore(b, metric);
    return bMean - aMean || bMedian - aMedian || a.name.localeCompare(b.name);
  });
}

function formatPerformanceValue(value: number, metric: PerformanceMetric): string {
  if (metric === "context") {
    return value >= 1_000 ? `${Math.round(value / 1_000)}k` : Math.round(value).toString();
  }
  const seconds = Math.round(value / 1_000);
  const minutes = Math.floor(seconds / 60);
  return minutes ? `${minutes}m${String(seconds % 60).padStart(2, "0")}s` : `${seconds}s`;
}

function distributionDetail(
  name: string,
  harness: string,
  metric: PerformanceMetric,
  value: StatsDistribution,
): string {
  const label = metric === "context" ? "Context" : "Duration";
  const fmt = (raw: number) => formatPerformanceValue(raw, metric);
  const stats = value.stats;
  if (!stats) {
    return `${name} · ${harness} · ${label}. 0 measured of ${value.expected} successful executions. Missing: ${value.missing_reasons.join("; ")}.`;
  }
  const reasons = value.missing_reasons.length
    ? ` Missing: ${value.missing_reasons.join("; ")}.`
    : "";
  return `${name} · ${harness} · ${label}. Max ${fmt(stats.max)} · Q3 ${fmt(stats.q3)} · Mean ${fmt(stats.mean)} · Median ${fmt(stats.median)} · Q1 ${fmt(stats.q1)} · Min ${fmt(stats.min)}. ${value.measured} measured of ${value.expected} successful executions.${reasons}`;
}

function DistributionPlot({
  name,
  harness,
  metric,
  value,
  scaleMax,
}: {
  name: string;
  harness: string;
  metric: PerformanceMetric;
  value: StatsDistribution;
  scaleMax: number;
}) {
  if (!value.stats) {
    const detail = `${name} · ${harness} · ${metric === "context" ? "Context" : "Duration"}. 0 measured of ${value.expected} successful executions. Missing: ${value.missing_reasons.join("; ")}.`;
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
  const pct = (raw: number) => `${Math.max(0, Math.min(100, (raw / scaleMax) * 100))}%`;
  const detail = distributionDetail(name, harness, metric, value);
  const partial = value.measured < value.expected;
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <div
        className="relative h-4 min-w-28"
        data-testid={`performance-${metric}-boxplot`}
        data-scale-max={scaleMax}
        aria-hidden="true"
      >
        <span
          className="absolute top-[7px] h-px bg-fg-4"
          style={{ left: pct(stats.min), width: pct(stats.max - stats.min) }}
        />
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
        <span
          className="absolute top-[5px] h-[5px] w-[5px] -translate-x-1/2 rounded-full bg-current"
          style={{ color: harnessColor(harness), left: pct(stats.mean) }}
        />
      </div>
      <Tooltip content={detail} side="top">
        <button
          type="button"
          aria-label={detail}
          className="w-fit text-left font-mono text-fg-4 underline decoration-dotted underline-offset-2"
          style={{ fontSize: "9.5px" }}
        >
          {formatPerformanceValue(stats.mean, metric)} avg · n={value.measured}
          {partial ? " ⚠" : ""}
        </button>
      </Tooltip>
    </div>
  );
}

function PerformanceCards({ aggregate }: { aggregate: StatsPerformanceAggregate }) {
  return (
    <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
      {aggregate.harnesses.map((item) => (
        <div key={item.harness} className="rounded-md border border-line bg-bg-3 p-3">
          <div className="mb-2 flex items-center gap-1.5 text-fg-3">
            <span
              className="h-2 w-2 rounded-full"
              style={{ backgroundColor: harnessColor(item.harness) }}
            />
            {item.harness}
          </div>
          <div className="font-mono text-fg">
            {item.context.stats
              ? formatPerformanceValue(item.context.stats.median, "context")
              : "—"}{" "}
            median context
          </div>
          <div className="font-mono text-fg-3">
            {item.duration.stats
              ? formatPerformanceValue(item.duration.stats.median, "duration")
              : "—"}{" "}
            median duration
          </div>
        </div>
      ))}
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
  renderName,
  onOpen,
  expandablePairs = false,
}: {
  rows: StatsPerformanceEntity[];
  harnesses: string[];
  sort: PerformanceMetric;
  /** Replaces the default (button-or-plain) name cell — the model axis marks
   *  provenance and renders "not set" in its own voice (#737). */
  renderName?: (row: StatsPerformanceEntity) => React.ReactNode;
  onOpen?: (row: StatsPerformanceEntity) => void;
  /** Node rows gain a chevron unfolding their model × effort couples (ADR-0065).
   *  Off on the model axis, where the model × effort path is already the drill. */
  expandablePairs?: boolean;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  // The couples' own expansion, dying with the table like `expanded` — the
  // parent keys the table on the drill path, so a stale set never leaks.
  const [couplesExpanded, setCouplesExpanded] = useState<Set<string>>(new Set());
  const ordered = sortPerformance(rows, sort);
  const visible = ordered.flatMap((row) => [
    { row, child: false },
    ...(expanded.has(row.id)
      ? sortPerformance(row.subagents, sort).map((child) => ({ row: child, child: true }))
      : []),
  ]);
  // The shared scale is drawn from the rows and their subagents — a couple's
  // peaks/durations come from those same session files, so they fit it.
  const scaleRows = rows.flatMap((row) => [row, ...row.subagents]);
  const scaleMax = (metric: PerformanceMetric) =>
    Math.max(
      1,
      ...scaleRows.flatMap((row) =>
        row.harnesses.map((item) => item[metric].stats?.max ?? 0),
      ),
    );
  const contextMax = scaleMax("context");
  const durationMax = scaleMax("duration");

  const metricCell = (
    name: string,
    rowHarnesses: StatsHarnessPerformance[] | undefined,
    metric: PerformanceMetric,
  ) => (
    <td key={metric} className="py-2 pr-3 align-top">
      <div className="grid gap-1.5">
        {harnesses.map((harness) => (
          <div key={harness} className="flex items-start gap-2">
            <span
              className="mt-1 h-[7px] w-[7px] shrink-0 rounded-full"
              style={{ backgroundColor: harnessColor(harness) }}
            />
            <DistributionPlot
              name={name}
              harness={harness}
              metric={metric}
              value={
                rowHarnesses?.find((item) => item.harness === harness)?.[metric] ?? {
                  stats: null,
                  measured: 0,
                  expected: 0,
                  missing_reasons: [`never ran on ${harness}`],
                }
              }
              scaleMax={metric === "context" ? contextMax : durationMax}
            />
          </div>
        ))}
      </div>
    </td>
  );

  return (
    <TooltipProvider>
      <table className="w-full table-fixed text-left" style={{ fontSize: "11px" }}>
        <thead className="text-fg-4">
          <tr>
            <th className="w-48 pb-2 font-medium">Name</th>
            <th className="pb-2 font-medium">Context (peak tokens)</th>
            <th className="pb-2 font-medium">Duration (wall-clock)</th>
          </tr>
        </thead>
        <tbody>
          {visible.map(({ row, child }) => {
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
                    </span>
                  </td>
                  {(["context", "duration"] as const).map((metric) =>
                    metricCell(row.name, row.harnesses, metric),
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
                        {(["context", "duration"] as const).map((metric) =>
                          metricCell(
                            `${pair.model} · ${pair.effort ?? "not set"}`,
                            pair.harnesses,
                            metric,
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

function PerformanceTab({
  performance,
  error,
}: {
  performance: StatsPerformance | null;
  error: string | null;
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

  if (error) {
    return (
      <div className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed">
        {error}
      </div>
    );
  }
  if (!performance) return <EmptyNote>Loading performance…</EmptyNote>;
  if (
    performance.by_pipeline.length === 0 &&
    performance.infrastructure.length === 0 &&
    performance.by_model.length === 0
  ) {
    return <EmptyNote>No successful executions in this period.</EmptyNote>;
  }
  // A period whose observations all lack a resolvable model is an empty axis —
  // its own absence, not a broken tab.
  if (axis === "model" && performance.by_model.length === 0) {
    return <EmptyNote>No model observed in this period.</EmptyNote>;
  }

  const toTotal = () => {
    setSelectedId(null);
    setSelectedModelId(null);
    setSelectedEffortId(null);
    setSelectedPipelineId(null);
  };

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

  const masterRows =
    axis === "model"
      ? sortPerformance(performance.by_model, sort)
      : sortPerformance([...performance.by_pipeline, infrastructureRow], sort);
  const selected =
    axis === "pipeline" ? (masterRows.find((row) => row.id === selectedId) ?? null) : null;
  const aggregate =
    axis === "model"
      ? (modelPipeline ?? effort ?? model ?? performance.total)
      : (selected ?? performance.total);

  let detailRows: StatsPerformanceEntity[];
  let detailRenderName: ((row: StatsPerformanceEntity) => React.ReactNode) | undefined;
  let onOpen: ((row: StatsPerformanceEntity) => void) | undefined;
  if (axis === "model") {
    if (modelPipeline) {
      // Node leaves are the floor: the model × effort path is the drill.
      detailRows = modelPipeline.nodes;
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
    detailRows =
      selected.id === "__infrastructure__" ? performance.infrastructure : selected.nodes;
  } else {
    detailRows = masterRows;
  }

  const contexts = aggregate.harnesses.map((item) =>
    item.context.stats ? formatPerformanceValue(item.context.stats.median, "context") : "—",
  );
  const durations = aggregate.harnesses.map((item) =>
    item.duration.stats ? formatPerformanceValue(item.duration.stats.median, "duration") : "—",
  );

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
    <div className="relative flex min-h-full" data-testid="stats-chart-performance">
      <aside className="w-[290px] shrink-0 border-r border-line pr-4">
        <div className="mb-2 flex items-center justify-between gap-2">
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Ranked by {sort}
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
              onChange={(event) => setSort(event.target.value as PerformanceMetric)}
              className="rounded border border-line bg-bg-3 px-2 py-1 text-fg-2"
            >
              <option value="context">By context</option>
              <option value="duration">By duration</option>
            </select>
          </span>
        </div>
        <MasterList
          rows={masterRows}
          selected={axis === "model" ? selectedModelId : selectedId}
          monoName={axis === "model"}
          ariaLabel="Performance groups"
          valueLabel={(row) => {
            const [mean] = performanceScore(row, sort);
            return mean < 0 ? "—" : formatPerformanceValue(mean, sort);
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
          <Breadcrumb testid="stats-performance-breadcrumb" crumbs={clickableCrumbs} />
        ) : (
          <div className="mb-3 text-fg-4" style={{ fontSize: "10.5px" }}>
            Total{selected ? ` / ${selected.name}` : ""}
          </div>
        )}
        <HarnessLegend harnesses={performance.harnesses} />
        <div className="mt-4 text-fg" data-testid="stats-performance-headline">
          {contexts.join(" / ") || "—"} median peak context · {durations.join(" / ") || "—"} median
          duration
        </div>
        <div className="mt-4">
          <PerformanceCards aggregate={aggregate} />
        </div>
        <div className="mt-4 min-h-[240px]">
          <PerformanceTable
            key={`${axis}-${selectedId ?? ""}-${selectedModelId ?? "total"}-${selectedEffortId ?? "total"}-${selectedPipelineId ?? ""}`}
            rows={detailRows}
            harnesses={performance.harnesses}
            sort={sort}
            renderName={detailRenderName}
            onOpen={onOpen}
            expandablePairs={axis === "pipeline"}
          />
        </div>
      </div>
    </div>
  );
}

export interface StatsChartsProps {
  tab: StatsTab;
  overview: StatsOverview | null;
  cost: StatsCost | null;
  costError: string | null;
  performance?: StatsPerformance | null;
  performanceError?: string | null;
}

export default function StatsCharts({
  tab,
  overview,
  cost,
  costError,
  performance = null,
  performanceError = null,
}: StatsChartsProps) {
  // #759: subscribing here re-renders the whole chart subtree on a theme switch,
  // so every `CHART.*` / `harnessColor()` read below resolves against the new
  // palette. Recharts bakes colours into props; nothing re-themes on its own.
  useTheme();

  if (tab === "performance") {
    return <PerformanceTab performance={performance} error={performanceError} />;
  }
  if (tab === "cost") {
    return <CostTab cost={cost} error={costError} />;
  }
  if (!overview) return <EmptyNote>Loading…</EmptyNote>;
  if (tab === "runs") return <RunsTab overview={overview} />;
  if (tab === "sessions") return <SessionsTab overview={overview} />;
  return <TriggersTab overview={overview} />;
}
