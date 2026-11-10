/**
 * Where a model or effort value was read from (ADR-0065 §1, #736, #737): the
 * copy every provenance tooltip of the Cost and Performance tabs says.
 */

import type { StatsHarnessCost } from "../../shared/generated/StatsHarnessCost";
import type { StatsProvenance } from "../../shared/generated/StatsProvenance";

/** Where a requested value was read from — the « ? » tooltip copy (ADR-0065 §1).
 *  Observed values are the norm and carry nothing; the vocabulary never says
 *  "real". */
export const PROVENANCE_COPY: Record<Exclude<StatsProvenance, "observed">, string> = {
  requested: "requested at node startup, not observed in transcripts",
  mixed: "partly requested at node startup, not observed in every transcript",
};

/** Hovering the italic "not set" effort says why the bucket exists. */
export const NOT_SET_COPY =
  "no effort requested at node startup nor observed in transcripts";

/** The Performance « By model » tooltip (#737): the wire carries the provenance
 *  but not the per-harness source lines Cost shows, so the copy is the axis's
 *  own honest summary (ADR-0065 §1). */
export function performanceProvenanceCopy(
  provenance: StatsProvenance | null | undefined,
): string {
  if (!provenance || provenance === "observed") {
    return "observed — the harness's source named the value";
  }
  return PROVENANCE_COPY[provenance];
}

// Per-harness source of a model/effort value (#736): the wire carries
// `provenance` (+ `effort_provenance`) and `provider` on StatsHarnessCost.

export const OBSERVED_HOW: Record<"model" | "effort", Record<string, string>> = {
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

export function sourceLines(
  harnesses: StatsHarnessCost[],
  target: "model" | "effort",
): string[] {
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
      const via =
        target === "model" && h.provider ? ` · via ${h.provider}` : "";
      return `${h.harness}: ${how}${via}`;
    })
    .filter((line): line is string => line !== null);
}
