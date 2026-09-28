import type { StatsHarnessCost } from "../../../../shared/generated/StatsHarnessCost";
import type { StatsProvenance } from "../../../../shared/generated/StatsProvenance";
import { Tooltip } from "../../../../shared/ui/tooltip";
import { PROVENANCE_COPY, sourceLines } from "../../../model/provenance";

export function ProvenanceMark({
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

/** The hoverable name of a model or effort: the value itself is the trigger, the
 *  tooltip says, harness by harness, where it was read and (models) the provider.
 *  The « ? » superscript stays as the at-a-glance mark for a requested/mixed value. */
export function ProvenanceName({
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
  const tooltip =
    content ?? (lines.length ? lines.join("\n") : (provenance ?? "observed"));
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
