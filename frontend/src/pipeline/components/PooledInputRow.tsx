import type { PooledInput, PooledInputSource } from "../../lib/derivePooledInputs";
import { FileText, X } from "lucide-react";
import { Chip } from "../../shared/ui/kit";

/**
 * One derived (emergent) input in the node inspector. Inputs are NOT declared
 * (#149, CONTEXT.md § Node): they are read-only, derived from incoming edges.
 * A pooled input — several same-named edges — spells out every contributing
 * source node, e.g. `review ← security-reviewer, perf-reviewer`.
 */
export default function PooledInputRow({
  input,
  highlighted,
  isLast,
  onDeleteSource,
}: {
  input: PooledInput;
  highlighted?: boolean;
  isLast?: boolean;
  /** Per-source delete (#339): removes the contributing edge — or, when that
   * edge carries other outputs that each feed their own input here (ADR-0073 /
   * #843), unticks just this output. The canonical "delete an input" since
   * inputs are emergent. Absent → read-only render. */
  onDeleteSource?: (source: PooledInputSource) => void;
}) {
  const pooled = input.sources.length > 1;
  return (
    <div
      data-port={input.name}
      data-testid={`pooled-input-${input.name}`}
      className={`ei-input${highlighted ? " ei-row--hl" : ""}`}
      data-last={isLast || undefined}
    >
      <span className="ei-line">
        <FileText size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
        <span className="d-mono ei-trunc" title={input.name}>
          {input.name}
        </span>
        {input.repeated && (
          <span title="repeated: accumulates iter-* artifacts from the source (read off the edge)">
            <Chip mono>rpt</Chip>
          </span>
        )}
        {pooled && (
          <span title="Pooled input: several same-named edges land here as one logical list">
            <Chip mono>pooled · {input.sources.length}</Chip>
          </span>
        )}
      </span>
      <span className="ei-input__srcs">
        <span aria-hidden>←</span>
        {input.sources.map((s, i) => (
          <span key={s.edgeIndex} className="ei-input__src">
            {s.label}
            {onDeleteSource && (
              <button
                type="button"
                data-testid={`pooled-input-${input.name}-delete-${s.nodeId}`}
                onClick={() => onDeleteSource(s)}
                className="ei-x"
                aria-label={`Delete input source ${s.label}`}
                title={
                  s.sharedEdge
                    ? `Drop this input: unticks '${s.port}' on the incoming edge`
                    : "Delete this input source: removes the incoming edge"
                }
              >
                <X size={10} strokeWidth={2} aria-hidden />
              </button>
            )}
            {i < input.sources.length - 1 && <span>,</span>}
          </span>
        ))}
      </span>
    </div>
  );
}
