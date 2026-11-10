import { Bar, BarChart, ResponsiveContainer, Tooltip as RTooltip } from "recharts";
import type { StatsCostPeriod } from "../../../../shared/generated/StatsCostPeriod";
import type { StatsSessionPeriod } from "../../../../shared/generated/StatsSessionPeriod";
import { formatCostAmount } from "../../../../run/model/costLabel";
import { harnessColor } from "../../../../harness/model/color";
import { flattenPeriods, isHarnessCost } from "../../../model/chartData";
import { chartAxes } from "./chartAxes";
import { CHART, bucketLabel } from "./chartTheme";

/** The box recharts measures: its height comes from the sheet (`.sx-chart`). */
export function ChartFrame({ children }: { children: React.ReactElement }) {
  return (
    <div className="sx-chart">
      <ResponsiveContainer width="100%" height="100%">
        {children}
      </ResponsiveContainer>
    </div>
  );
}

export function EmptyNote({ children }: { children: React.ReactNode }) {
  return <div className="sx-empty">{children}</div>;
}

/** Which Runs the figures on screen describe (#810). Always visible, never a
 *  silent filter: amber once « completed runs only » narrows the cohort, muted
 *  otherwise, and explicit about what was left out. */
export function CohortLine({
  completedOnly,
  rolledUp = false,
  note,
}: {
  completedOnly: boolean;
  /** « Roll up child runs »: the cohort is the root runs, each with its
   *  tree — amber, like any cohort that is not the default one, and saying
   *  the totals no longer match the default view. */
  rolledUp?: boolean;
  /** An extra clause for a tab the cohort does not govern. */
  note?: string;
}) {
  return (
    <div
      data-testid="stats-cohort-line"
      className={completedOnly || rolledUp ? "sx-note sx-note--warn" : "sx-note"}
    >
      {rolledUp ? (
        <>
          Cohort: <span className="font-medium">root runs</span> started in the
          period · child runs counted under their root run · totals per
          pipeline and project differ from the default view
        </>
      ) : (
        "Cohort: runs started in the period"
      )}
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

// --- Panels ---------------------------------------------------------------------

/** A hairline panel: a 28 px head (ruby square, title, count, anything after),
 *  then its body. Charts and tables sit in one. */
export function Panel({
  title,
  count,
  aside,
  children,
  flush = false,
  testid,
  className,
}: {
  title: React.ReactNode;
  count?: number;
  aside?: React.ReactNode;
  children: React.ReactNode;
  /** No padding around the body (a table). */
  flush?: boolean;
  testid?: string;
  className?: string;
}) {
  return (
    <section className={`sx-panel ${className ?? ""}`} data-testid={testid}>
      <header className="sx-panel__head">
        <span className="sx-panel__title">{title}</span>
        {count !== undefined && <span className="d-chip sx-num">{count}</span>}
        <span className="sx-grow" />
        {aside}
      </header>
      <div className={flush ? "sx-panel__body--flush" : "sx-panel__body"}>{children}</div>
    </section>
  );
}

export interface LegendItem {
  key: string;
  label: string;
  color: string;
  /** The series' total over the period, after the label. */
  total?: string;
  testid?: string;
}

/** A chart's legend, in its panel head: swatch, name, total. */
export function ChartLegend({ items }: { items: LegendItem[] }) {
  return (
    <span className="sx-legend">
      {items.map((item) => (
        <span key={item.key} className="sx-legend__item">
          <span
            className="sx-swatch"
            style={{ backgroundColor: item.color }}
            data-testid={item.testid}
            aria-hidden
          />
          {item.label}
          {item.total !== undefined && <span className="sx-num">{item.total}</span>}
        </span>
      ))}
    </span>
  );
}

/** The harnesses as a legend (#638): one swatch per harness. */
export function HarnessLegend({
  harnesses,
  totals,
}: {
  harnesses: string[];
  totals?: Record<string, string>;
}) {
  return (
    <ChartLegend
      items={harnesses.map((harness) => ({
        key: harness,
        label: harness,
        color: harnessColor(harness),
        total: totals?.[harness],
        testid: `stats-harness-legend-${harness}`,
      }))}
    />
  );
}

/** What recharts hands a custom tooltip. */
interface TooltipPayload {
  name?: string | number;
  value?: unknown;
  color?: string;
  dataKey?: string | number;
}

/**
 * The tooltip of every Stats chart: the bucket, then one row per series with
 * its swatch and value. Drawn by the sheet (`.sx-tip`), so the type scale and
 * the UI scale apply to it like to the rest of the page.
 */
export function ChartTooltip({
  active,
  payload,
  label,
  format = (value) => (value == null ? "—" : String(value)),
}: {
  active?: boolean;
  payload?: readonly TooltipPayload[];
  label?: string | number;
  format?: (value: unknown, name: string) => string;
}) {
  if (!active || !payload?.length) return null;
  return (
    <div className="sx-tip" data-testid="stats-chart-tooltip">
      <div className="sx-tip__head">{bucketLabel(String(label ?? ""))}</div>
      {payload.map((item) => (
        <div key={String(item.dataKey ?? item.name)} className="sx-tip__row">
          <span className="sx-swatch" style={{ backgroundColor: item.color }} aria-hidden />
          <span>{String(item.name ?? "")}</span>
          <span className="sx-num">{format(item.value, String(item.name ?? ""))}</span>
        </div>
      ))}
    </div>
  );
}

export function HarnessBars({
  periods,
  harnesses,
  value,
  title,
}: {
  periods: (StatsSessionPeriod | StatsCostPeriod)[];
  harnesses: string[];
  value: "executions" | "usd";
  title: string;
}) {
  const totals = Object.fromEntries(
    harnesses.map((harness) => {
      const metrics = periods.flatMap((period) =>
        period.harnesses.filter((item) => item.harness === harness),
      );
      if (value === "usd") {
        const priced = metrics.filter(isHarnessCost).filter((m) => m.usd !== null);
        if (priced.length === 0) return [harness, "—"];
        const sum = priced.reduce((acc, m) => acc + (m.usd ?? 0), 0);
        return [harness, formatCostAmount(sum, priced.some((m) => m.partial), true)];
      }
      return [
        harness,
        String(metrics.reduce((acc, m) => acc + ("executions" in m ? m.executions : 0), 0)),
      ];
    }),
  );
  return (
    <Panel title={title} aside={<HarnessLegend harnesses={harnesses} totals={totals} />}>
      {periods.length === 0 ? (
        <EmptyNote>No activity in this period.</EmptyNote>
      ) : (
        <ChartFrame>
          <BarChart data={flattenPeriods(periods, value)} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
            {chartAxes(value === "usd")}
            <RTooltip
              cursor={{ fill: CHART.cursor }}
              content={
                <ChartTooltip
                  format={(raw, name) => {
                    if (raw == null) return "—";
                    const amount = typeof raw === "number" ? raw : Number(raw);
                    if (value !== "usd") return String(amount);
                    const metric = periods
                      .flatMap((period) => period.harnesses)
                      .find((harness) => harness.harness === name);
                    const costMetric = isHarnessCost(metric) ? metric : undefined;
                    return formatCostAmount(
                      amount,
                      costMetric?.partial ?? false,
                      costMetric?.estimated ?? true,
                    );
                  }}
                />
              }
            />
            {harnesses.map((harness) => (
              <Bar
                key={harness}
                dataKey={harness}
                name={harness}
                stackId="harness"
                fill={harnessColor(harness)}
                maxBarSize={28}
                isAnimationActive={false}
              />
            ))}
          </BarChart>
        </ChartFrame>
      )}
    </Panel>
  );
}

// --- Figures on one line ---------------------------------------------------------

export interface KpiItem {
  key: string;
  label: string;
  value: React.ReactNode;
  /** A short qualifier after the figure: « $1.75 median ». */
  sub?: React.ReactNode;
  /** The qualifier needs attention (amber). */
  warn?: boolean;
  /** A harness swatch before the label. */
  swatch?: string;
  testid?: string;
  title?: string;
}

/** The figures of a tab on one line (`overlays.statsKpi`): label, figure in
 *  mono, qualifier; the first figure — the headline — in ruby. */
export function Kpis({ items, testid }: { items: KpiItem[]; testid?: string }) {
  return (
    <div className="sx-kpi" data-testid={testid}>
      {items.map((item) => (
        <div key={item.key} className="sx-kpi__cell" data-testid={item.testid} title={item.title}>
          {item.swatch && (
            <span className="sx-swatch" style={{ backgroundColor: item.swatch }} aria-hidden />
          )}
          {item.label && <span className="sx-kpi__label">{item.label} </span>}
          <span className="sx-kpi__value">{item.value}</span>
          {item.sub !== undefined && (
            <span
              className="sx-kpi__sub"
              data-tone={item.warn ? "warn" : undefined}
              // The qualifier is the part a narrow line cuts short: its tooltip reads it whole.
              title={typeof item.sub === "string" ? item.sub : undefined}
            >
              {" "}
              {item.sub}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
