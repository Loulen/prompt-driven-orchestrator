import { useMemo, useState } from "react";
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
  StatsCost,
  StatsCostAggregate,
  StatsCostEntity,
  StatsCostPeriod,
  StatsHarnessCost,
  StatsOverview,
  StatsProjectCostEntity,
  StatsSessionEntity,
  StatsSessionHarness,
  StatsSessionPeriod,
} from "../types";
import { formatCostAmount } from "../lib/costLabel";
import { harnessColor } from "../lib/harness";
import { Tooltip, TooltipProvider } from "./ui/tooltip";

export type StatsTab = "runs" | "sessions" | "triggers" | "cost";

const CHART = {
  runs: "#58a6ff",
  errors: "#f85149",
  fires: "#3fb950",
  grid: "#30363d",
  axis: "#8b949e",
} as const;

const AXIS_PROPS = {
  stroke: CHART.axis,
  tick: { fill: CHART.axis, fontSize: 10 },
} as const;

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
          contentStyle={{ background: "#161b22", border: `1px solid ${CHART.grid}`, fontSize: 11 }}
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
            contentStyle={{ background: "#161b22", border: `1px solid ${CHART.grid}`, fontSize: 11 }}
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
}: {
  rows: T[];
  selected: string | null;
  valueLabel: (row: T) => string;
  onSelect: (id: string | null) => void;
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
      aria-label="Spenders"
      className="flex min-w-[250px] flex-col gap-1 border-r border-line pr-3"
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
            <span className="truncate">{row.name}</span>
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
        <MasterList
          rows={rows}
          selected={selectedId}
          valueLabel={(row) => String(row.executions)}
          onSelect={setSelectedId}
        />
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
                background: "#161b22",
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

function CostTable({
  rows,
  harnesses,
  unit,
  onOpen,
}: {
  rows: StatsCostEntity[];
  harnesses: string[];
  unit: "Run" | "execution";
  onOpen?: (row: StatsCostEntity) => void;
}) {
  const sorted = [...rows].sort((a, b) => (b.usd ?? -1) - (a.usd ?? -1));
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
          {sorted.map((row) => (
            <tr
              key={row.id}
              className="border-t border-line text-fg-2"
              tabIndex={0}
              data-testid="stats-detail-row"
            >
              <td className="py-2 pr-2">
                {onOpen ? (
                  <button
                    type="button"
                    aria-label={`Open ${row.name}`}
                    onClick={() => onOpen(row)}
                    className="text-left hover:text-fg"
                  >
                    {row.name}
                  </button>
                ) : (
                  row.name
                )}
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
          ))}
        </tbody>
      </table>
    </TooltipProvider>
  );
}

function CostTab({
  cost,
  error,
}: {
  cost: StatsCost | null;
  error: string | null;
}) {
  const [axis, setAxis] = useState<"pipeline" | "project">("pipeline");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [drilledPipelineId, setDrilledPipelineId] = useState<string | null>(null);

  if (error) {
    return (
      <div className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed">
        {error}
      </div>
    );
  }
  if (!cost) return <EmptyNote>Loading cost…</EmptyNote>;

  const rows = axis === "pipeline" ? cost.by_pipeline : cost.by_project;
  const selected = rows.find((row) => row.id === selectedId) ?? null;
  const drilledPipeline =
    axis === "project" && selected
      ? (selected as StatsProjectCostEntity).pipelines.find(
          (pipeline) => pipeline.id === drilledPipelineId,
        ) ?? null
      : null;
  const aggregate = drilledPipeline ?? selected ?? cost.total;
  const periods = drilledPipeline
    ? drilledPipeline.by_period
    : selected
      ? selected.by_period
      : cost.by_period;
  let detailRows: StatsCostEntity[];
  let detailUnit: "Run" | "execution" = "Run";
  let onOpen: ((row: StatsCostEntity) => void) | undefined;
  if (drilledPipeline) {
    detailRows = drilledPipeline.nodes;
    detailUnit = "execution";
  } else if (!selected) {
    detailRows = axis === "project" ? cost.by_project : cost.by_pipeline;
  } else if (axis === "project") {
    detailRows = (selected as StatsProjectCostEntity).pipelines;
    onOpen = (pipeline) => setDrilledPipelineId(pipeline.id);
  } else {
    detailRows = selected.nodes;
    detailUnit = "execution";
  }

  return (
    <div className="relative flex flex-col gap-4" data-testid="stats-chart-cost">
      <div className="flex items-center justify-between gap-3">
        <HarnessLegend harnesses={cost.harnesses} />
        <select
          aria-label="Cost grouping"
          value={axis}
          onChange={(event) => {
            setAxis(event.target.value as "pipeline" | "project");
            setSelectedId(null);
            setDrilledPipelineId(null);
          }}
          className="rounded border border-line bg-bg-3 px-2 py-1 text-fg-2"
        >
          <option value="pipeline">By pipeline</option>
          <option value="project">By project</option>
        </select>
      </div>
      <div className="text-fg" data-testid="stats-selection-headline">
        {formatCostAmount(aggregate.usd, aggregate.partial, aggregate.estimated)} total
        {" · "}
        {formatCostAmount(aggregate.average_usd, aggregate.partial, aggregate.estimated)} per Run
      </div>
      <HarnessCards aggregate={aggregate} />
      {aggregate.unknown > 0 && (
        <div className="text-st-await" style={{ fontSize: "10.5px" }}>
          {aggregate.unknown} Run{aggregate.unknown === 1 ? "" : "s"} without computable cost
        </div>
      )}
      <HarnessBars periods={periods} harnesses={cost.harnesses} value="usd" />
      <div className="flex min-h-[240px] gap-4">
        <MasterList
          rows={rows}
          selected={selectedId}
          valueLabel={(row) => formatCostAmount(row.usd, row.partial, row.estimated)}
          onSelect={(id) => {
            setSelectedId(id);
            setDrilledPipelineId(null);
          }}
        />
        <div className="min-w-0 flex-1">
          <div
            className="mb-3 text-fg-4"
            style={{ fontSize: "10.5px" }}
            data-testid="stats-cost-breadcrumb"
          >
            Total{selected ? ` / ${selected.name}` : ""}
            {drilledPipeline ? ` / ${drilledPipeline.name}` : ""}
          </div>
          <CostTable
            rows={detailRows}
            harnesses={cost.harnesses}
            unit={detailUnit}
            onOpen={onOpen}
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
}

export default function StatsCharts({
  tab,
  overview,
  cost,
  costError,
}: StatsChartsProps) {
  if (tab === "cost") {
    return <CostTab cost={cost} error={costError} />;
  }
  if (!overview) return <EmptyNote>Loading…</EmptyNote>;
  if (tab === "runs") return <RunsTab overview={overview} />;
  if (tab === "sessions") return <SessionsTab overview={overview} />;
  return <TriggersTab overview={overview} />;
}
