import type { StatsCostAggregate } from "../../../../shared/generated/StatsCostAggregate";
import type { StatsHarnessCost } from "../../../../shared/generated/StatsHarnessCost";
import { formatCostAmount } from "../../../../run/model/costLabel";
import { harnessColor } from "../../../../harness/model/color";
import { Tooltip } from "../../../../shared/ui/tooltip";
import { coverage } from "../../../model/cost";

export function HarnessCards({ aggregate }: { aggregate: StatsCostAggregate }) {
  return (
    <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
      {aggregate.harnesses.map((metric) => (
        <div
          key={metric.harness}
          className="rounded-md border border-line bg-bg-3 p-3"
          data-testid={`stats-harness-card-${metric.harness}`}
        >
          <div
            className="mb-2 flex items-center gap-1.5 text-fg-3"
            style={{ fontSize: "10.5px" }}
          >
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

export function CostCell({
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
      <span>
        {formatCostAmount(metric.usd, metric.partial, metric.estimated)}
      </span>
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
