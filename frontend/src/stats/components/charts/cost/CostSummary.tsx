import type { StatsCostAggregate } from "../../../../shared/generated/StatsCostAggregate";
import type { StatsCostPeriod } from "../../../../shared/generated/StatsCostPeriod";
import { formatCostAmount } from "../../../../run/model/costLabel";
import { HarnessBars } from "../common/ChartPrimitives";
import { HarnessCards } from "./CostCells";

/** What the drill level on screen cost: the headline (total and median per
 *  Run or execution), one card per harness, the Runs without a computable cost
 *  and the cost over time. */
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
      <div className="mt-4 text-fg" data-testid="stats-selection-headline">
        {formatCostAmount(
          aggregate.usd,
          aggregate.partial,
          aggregate.estimated,
        )}{" "}
        total
        {" · "}
        {formatCostAmount(
          aggregate.median_usd,
          aggregate.partial,
          aggregate.estimated,
        )}{" "}
        median per {unit}
      </div>
      <div className="mt-4">
        <HarnessCards aggregate={aggregate} />
      </div>
      {aggregate.unknown > 0 && (
        <div className="mt-4 text-st-await" style={{ fontSize: "10.5px" }}>
          {aggregate.unknown} {unit}
          {aggregate.unknown === 1 ? "" : "s"} without computable cost
        </div>
      )}
      <div className="mt-4">
        <HarnessBars
          periods={periods}
          harnesses={harnesses}
          value="usd"
        />
      </div>
    </>
  );
}
