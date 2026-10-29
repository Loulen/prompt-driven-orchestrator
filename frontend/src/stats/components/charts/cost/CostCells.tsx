import type { StatsHarnessCost } from "../../../../shared/generated/StatsHarnessCost";
import { formatCostAmount } from "../../../../run/model/costLabel";
import { Tooltip } from "../../../../shared/ui/tooltip";
import { coverage } from "../../../model/cost";

export function CostCell({
  metric,
  unit,
  bar,
}: {
  metric: StatsHarnessCost | undefined;
  unit: "Run" | "execution";
  /** The share bar of the column, behind the figure only. */
  bar?: React.ReactNode;
}) {
  if (!metric) return <span className="text-text-3">—</span>;
  const detail = coverage(metric, unit);
  return (
    <span className="sx-cell2">
      <span className="sx-cell2__v">
        {bar}
        <span>{formatCostAmount(metric.usd, metric.partial, metric.estimated)}</span>
      </span>
      <Tooltip content={detail} side="top">
        <button type="button" aria-label={detail} className="sx-cell2__sub">
          {metric.median_usd === null
            ? "— median"
            : `${formatCostAmount(metric.median_usd, metric.partial, metric.estimated)} median`}
        </button>
      </Tooltip>
    </span>
  );
}
