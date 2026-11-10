import type { StatsCostAggregate } from "../../../../shared/generated/StatsCostAggregate";
import type { StatsCostPeriod } from "../../../../shared/generated/StatsCostPeriod";
import { formatCostAmount } from "../../../../run/model/costLabel";
import { harnessColor } from "../../../../harness/model/color";
import { HarnessBars, Kpis } from "../common/ChartPrimitives";

/** What the drill level on screen cost: on one line the total and its median
 *  per Run or execution, then each harness; the Runs without a computable cost;
 *  the cost over time. */
export function CostSummary({
  aggregate,
  periods,
  harnesses,
  unit,
}: {
  aggregate: StatsCostAggregate;
  periods: StatsCostPeriod[];
  harnesses: string[];
  unit: "Run" | "execution";
}) {
  return (
    <>
      <Kpis
        items={[
          {
            key: "total",
            label: "Total",
            value: formatCostAmount(aggregate.usd, aggregate.partial, aggregate.estimated),
            sub: `${formatCostAmount(aggregate.median_usd, aggregate.partial, aggregate.estimated)} median per ${unit}`,
            testid: "stats-selection-headline",
          },
          ...aggregate.harnesses.map((metric) => ({
            key: metric.harness,
            label: metric.harness,
            swatch: harnessColor(metric.harness),
            value: formatCostAmount(metric.usd, metric.partial, metric.estimated),
            sub:
              metric.median_usd === null
                ? "— median"
                : `${formatCostAmount(metric.median_usd, metric.partial, metric.estimated)} median`,
            testid: `stats-harness-card-${metric.harness}`,
          })),
        ]}
      />
      {aggregate.unknown > 0 && (
        <div className="sx-note sx-note--warn">
          {aggregate.unknown} {unit}
          {aggregate.unknown === 1 ? "" : "s"} without computable cost
        </div>
      )}
      <HarnessBars title="Cost by harness" periods={periods} harnesses={harnesses} value="usd" />
    </>
  );
}
