import type { PooledInput } from "../lib/derivePooledInputs";
import { Tooltip } from "./ui/tooltip";

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
  /** Per-source delete (#339): deletes the contributing edge — the canonical
   * "delete an input" since inputs are emergent. Absent → read-only render. */
  onDeleteSource?: (edgeIndex: number) => void;
}) {
  const pooled = input.sources.length > 1;
  return (
    <div
      data-port={input.name}
      data-testid={`pooled-input-${input.name}`}
      className={`flex items-start gap-2 px-1 py-2 transition-colors ${
        isLast ? "" : "border-b border-line-soft"
      } ${highlighted ? "bg-acc-bg" : ""}`}
    >
      <span className="mt-1 h-1.5 w-1.5 shrink-0 rounded-full bg-fg-4" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-center gap-1.5">
          <span className="font-mono text-fg" style={{ fontSize: "11px" }}>
            {input.name}
          </span>
          {input.repeated && (
            <Tooltip content="repeated: accumulates iter-* artifacts from the source (read off the edge).">
              <span className="text-acc" style={{ fontSize: "9px" }}>
                rpt
              </span>
            </Tooltip>
          )}
          {pooled && (
            <Tooltip content="Pooled input: several same-named edges land here as one logical list.">
              <span
                className="rounded bg-bg-4 px-1 text-fg-4"
                style={{ fontSize: "9px" }}
              >
                pooled · {input.sources.length}
              </span>
            </Tooltip>
          )}
        </div>
        <div className="flex min-w-0 items-baseline gap-1 text-fg-3" style={{ fontSize: "10px" }}>
          <span className="shrink-0 text-fg-4 font-mono">{"←"}</span>
          <span className="flex min-w-0 flex-wrap items-baseline font-mono">
            {input.sources.map((s, i) => (
              <span key={s.edgeIndex} className="flex min-w-0 items-baseline break-words">
                {s.label}
                {onDeleteSource && (
                  <Tooltip content="Delete this input source (removes the incoming edge).">
                    <button
                      data-testid={`pooled-input-${input.name}-delete-${s.nodeId}`}
                      onClick={() => onDeleteSource(s.edgeIndex)}
                      className="ml-0.5 cursor-pointer rounded px-0.5 text-fg-4 transition-colors hover:bg-bg-4 hover:text-fg"
                      aria-label={`Delete input source ${s.label}`}
                    >
                      ×
                    </button>
                  </Tooltip>
                )}
                {i < input.sources.length - 1 && <span className="mr-1">,</span>}
              </span>
            ))}
          </span>
        </div>
      </div>
    </div>
  );
}
