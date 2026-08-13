import {
  BarChart,
  Bar,
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip as RTooltip,
  Legend,
  CartesianGrid,
  ResponsiveContainer,
} from "recharts";
import type { StatsOverview, StatsCost, StatsCostBucket, PriceRow } from "../types";
import { formatBucketCost } from "../lib/costLabel";

// This module is the ONLY importer of `recharts`, so it is code-split via
// `React.lazy` in StatsModal (the first lazy chunk in the repo) — recharts stays
// out of the main bundle and loads when the Stats modal first opens.

export type StatsTab = "runs" | "sessions" | "triggers" | "cost";

// Palette — reads on the dark UI; deliberately not the brand accent so series
// stay distinguishable.
const C = {
  runs: "#58a6ff",
  errors: "#f85149",
  sessions: "#a371f7",
  fires: "#3fb950",
  cost: "#d29922",
  grid: "#30363d",
  axis: "#8b949e",
} as const;

const AXIS_PROPS = {
  stroke: C.axis,
  tick: { fill: C.axis, fontSize: 10 },
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

/** Merge two per-bucket count series onto the ordered `buckets` x-axis. */
function mergeCounts(
  buckets: string[],
  a: { bucket: string; count: number }[],
  b: { bucket: string; count: number }[],
): { bucket: string; runs: number; errors: number }[] {
  const am = new Map(a.map((r) => [r.bucket, r.count]));
  const bm = new Map(b.map((r) => [r.bucket, r.count]));
  return buckets.map((bucket) => ({
    bucket,
    runs: am.get(bucket) ?? 0,
    errors: bm.get(bucket) ?? 0,
  }));
}

function RunsTab({ overview }: { overview: StatsOverview }) {
  if (overview.buckets.length === 0) return <EmptyNote>No runs in this period.</EmptyNote>;
  const data = mergeCounts(overview.buckets, overview.runs, overview.errors);
  return (
    <div data-testid="stats-chart-runs">
      <ChartFrame>
        <BarChart data={data} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
          <CartesianGrid stroke={C.grid} strokeDasharray="3 3" vertical={false} />
          <XAxis dataKey="bucket" {...AXIS_PROPS} />
          <YAxis allowDecimals={false} {...AXIS_PROPS} />
          <RTooltip
            contentStyle={{ background: "#161b22", border: `1px solid ${C.grid}`, fontSize: 11 }}
          />
          <Legend wrapperStyle={{ fontSize: 11 }} />
          <Bar dataKey="runs" name="Runs" fill={C.runs} radius={[2, 2, 0, 0]} />
          <Bar dataKey="errors" name="Errors (failed)" fill={C.errors} radius={[2, 2, 0, 0]} />
        </BarChart>
      </ChartFrame>
    </div>
  );
}

function SessionsTab({ overview }: { overview: StatsOverview }) {
  if (overview.buckets.length === 0)
    return <EmptyNote>No node sessions in this period.</EmptyNote>;
  const sm = new Map(overview.sessions.map((r) => [r.bucket, r.count]));
  const data = overview.buckets.map((bucket) => ({ bucket, sessions: sm.get(bucket) ?? 0 }));
  return (
    <div data-testid="stats-chart-sessions">
      <ChartFrame>
        <LineChart data={data} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
          <CartesianGrid stroke={C.grid} strokeDasharray="3 3" vertical={false} />
          <XAxis dataKey="bucket" {...AXIS_PROPS} />
          <YAxis allowDecimals={false} {...AXIS_PROPS} />
          <RTooltip
            contentStyle={{ background: "#161b22", border: `1px solid ${C.grid}`, fontSize: 11 }}
          />
          <Line
            type="monotone"
            dataKey="sessions"
            name="Node sessions started"
            stroke={C.sessions}
            strokeWidth={2}
            dot={{ r: 2 }}
          />
        </LineChart>
      </ChartFrame>
    </div>
  );
}

function TriggersTab({ overview }: { overview: StatsOverview }) {
  const kpi = overview.triggers_created_runs;
  return (
    <div data-testid="stats-chart-triggers" className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2" style={{ fontSize: "11px" }}>
        <span
          className="rounded bg-bg-3 px-2 py-1 text-fg-2"
          data-testid="stats-kpi-created-runs"
        >
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
          <BarChart
            data={overview.fires_by_pipeline}
            margin={{ top: 8, right: 8, left: -18, bottom: 0 }}
          >
            <CartesianGrid stroke={C.grid} strokeDasharray="3 3" vertical={false} />
            <XAxis dataKey="pipeline_id" {...AXIS_PROPS} />
            <YAxis allowDecimals={false} {...AXIS_PROPS} />
            <RTooltip
              contentStyle={{ background: "#161b22", border: `1px solid ${C.grid}`, fontSize: 11 }}
            />
            <Bar dataKey="count" name="Fires" fill={C.fires} radius={[2, 2, 0, 0]} />
          </BarChart>
        </ChartFrame>
      )}
    </div>
  );
}

/** One labelled cost row carrying the honesty vocabulary (`~$` / `†` / `—`). */
function CostRow({ label, bucket }: { label: string; bucket: StatsCostBucket }) {
  const c = formatBucketCost(
    bucket.usd,
    bucket.partial,
    bucket.null,
    bucket.runs,
    bucket.unpriced_models,
  );
  return (
    <div
      className="flex items-center justify-between rounded bg-bg-3 px-2 py-1"
      style={{ fontSize: "10.5px" }}
      data-testid="stats-cost-row"
      title={c.title}
    >
      <span className="text-fg-3">{label}</span>
      <span className={`flex items-center gap-1 font-mono ${c.empty ? "text-fg-4" : "text-fg-2"}`}>
        {c.text}
        {c.dagger && (
          <span className="text-st-await" title={c.title}>
            †
          </span>
        )}
      </span>
    </div>
  );
}

function CostSection({
  title,
  rows,
}: {
  title: string;
  rows: { label: string; bucket: StatsCostBucket }[];
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <h4 className="font-medium text-fg-2" style={{ fontSize: "11px" }}>
        {title}
      </h4>
      {rows.length === 0 ? (
        <EmptyNote>No runs in this period.</EmptyNote>
      ) : (
        <div className="flex flex-col gap-1">
          {rows.map((r) => (
            <CostRow key={r.label} label={r.label} bucket={r.bucket} />
          ))}
        </div>
      )}
    </div>
  );
}

/** The resolved price table (#528): one row per family — the WINNING tier and the
 *  $/MTok actually in force. Reads the same table the fold bills with, so pressing
 *  "Sync costs" (just above) and reading what PDO can price happen in one place.
 *  Defensive `?.` for the same `vite dev` reason as the cost buckets. */
function ResolvedPrices({ resolved }: { resolved: PriceRow[] | undefined }) {
  if (!resolved?.length) return null;
  return (
    <div className="flex flex-col gap-1.5" data-testid="stats-cost-resolved">
      <h4 className="font-medium text-fg-2" style={{ fontSize: "11px" }}>
        What PDO can price
      </h4>
      <div className="flex flex-col gap-1">
        {resolved.map((row) => (
          <div
            key={row.key}
            className="flex items-center justify-between rounded bg-bg-3 px-2 py-1 font-mono text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid={`price-row-${row.key}`}
          >
            <span>{row.key}</span>
            <span className="flex items-center gap-2">
              <span
                className="rounded bg-bg-4 px-1.5 py-0.5 text-fg-4"
                data-testid={`price-row-tier-${row.key}`}
              >
                {row.tier}
              </span>
              <span className="text-fg-2">
                ${row.input}/${row.output} /MTok
              </span>
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function CostTab({ cost, error }: { cost: StatsCost | null; error: string | null }) {
  if (error)
    return (
      <div
        className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
        style={{ fontSize: "11.5px" }}
        data-testid="stats-cost-error"
      >
        {error}
      </div>
    );
  // No synchronous loading flag (see useStats): null cost + no error = fetching.
  if (!cost) return <EmptyNote>Loading cost…</EmptyNote>;

  const periodBars = cost.by_period.map((b) => ({ label: b.bucket, usd: b.usd }));
  const hasSpend = cost.by_period.some((b) => b.usd > 0);

  return (
    <div className="flex flex-col gap-4" data-testid="stats-chart-cost">
      <div className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="stats-cost-disclaimer">
        Estimates from local Claude Code token usage × public list prices — not an invoice. A bucket
        with any partial run is a lower bound (†); runs with no transcript are excluded (shown as —).
      </div>

      {hasSpend && (
        <ChartFrame>
          <BarChart data={periodBars} margin={{ top: 8, right: 8, left: -8, bottom: 0 }}>
            <CartesianGrid stroke={C.grid} strokeDasharray="3 3" vertical={false} />
            <XAxis dataKey="label" {...AXIS_PROPS} />
            <YAxis {...AXIS_PROPS} tickFormatter={(v) => `$${v}`} />
            <RTooltip
              contentStyle={{ background: "#161b22", border: `1px solid ${C.grid}`, fontSize: 11 }}
              formatter={(value) => {
                const v = typeof value === "number" ? value : Number(value) || 0;
                return [`~$${v.toFixed(v < 1 ? 4 : 2)}`, "est. cost"];
              }}
            />
            <Bar dataKey="usd" name="Est. cost" fill={C.cost} radius={[2, 2, 0, 0]} />
          </BarChart>
        </ChartFrame>
      )}

      <CostSection
        title="By period"
        rows={cost.by_period.map((b) => ({ label: b.bucket, bucket: b }))}
      />
      <CostSection
        title="By pipeline"
        rows={cost.by_pipeline.map((b) => ({ label: b.key, bucket: b }))}
      />
      <CostSection
        title="By project"
        rows={cost.by_project.map((b) => ({ label: b.key, bucket: b }))}
      />
      <ResolvedPrices resolved={cost.resolved} />
    </div>
  );
}

export interface StatsChartsProps {
  tab: StatsTab;
  overview: StatsOverview | null;
  cost: StatsCost | null;
  costError: string | null;
}

/** The chart surface for the active tab. Lazy-loaded (so recharts is code-split
 *  out of the main bundle). */
export default function StatsCharts({ tab, overview, cost, costError }: StatsChartsProps) {
  if (tab === "cost") return <CostTab cost={cost} error={costError} />;
  if (!overview) return <EmptyNote>Loading…</EmptyNote>;
  if (tab === "runs") return <RunsTab overview={overview} />;
  if (tab === "sessions") return <SessionsTab overview={overview} />;
  return <TriggersTab overview={overview} />;
}
