import type { StatsCostPeriod } from "../../shared/generated/StatsCostPeriod";
import type { StatsHarnessCost } from "../../shared/generated/StatsHarnessCost";
import type { StatsSessionHarness } from "../../shared/generated/StatsSessionHarness";
import type { StatsSessionPeriod } from "../../shared/generated/StatsSessionPeriod";

export function flattenPeriods(
  periods: (StatsSessionPeriod | StatsCostPeriod)[],
  value: "executions" | "usd",
) {
  return periods.map((period) => ({
    bucket: period.bucket,
    ...Object.fromEntries(
      period.harnesses.map((harness) => [
        harness.harness,
        value === "usd"
          ? "usd" in harness
            ? harness.usd
            : null
          : harness.executions,
      ]),
    ),
  }));
}

export function isHarnessCost(
  metric: StatsSessionHarness | StatsHarnessCost | undefined,
): metric is StatsHarnessCost {
  return metric !== undefined && "average_usd" in metric;
}
