import type { StatsPerformanceEntity } from "../../../../shared/generated/StatsPerformanceEntity";
import { Tooltip } from "../../../../shared/ui/tooltip";
import { FILTERED_BY_KIND_COPY } from "../../../model/performance";
import { isNodeRow, rowKinds } from "../../../model/nodeKind";
import { EmptyNote } from "../common/ChartPrimitives";

/** The badges the Name cell carries for a Node row (#810) — nothing at all for
 *  a standard node: an absence needs no mark. */
export function KindBadges({ row }: { row: StatsPerformanceEntity }) {
  if (!isNodeRow(row)) return null;
  const kinds = rowKinds(row).filter((kind) => kind !== "standard");
  if (kinds.length === 0) return null;
  return (
    <>
      {kinds.map((kind) => (
        <span
          key={kind}
          data-testid={`stats-node-kind-${kind}`}
          className="shrink-0 rounded border border-line-strong px-1 font-mono text-text-3"
          style={{ fontSize: "9px" }}
        >
          {kind}
        </span>
      ))}
    </>
  );
}

/** The kind filter left nothing on screen (#810) — the whole tab when every
 *  chip is off, a drill level when its every Node row is hidden. Both say the
 *  same sentence and carry the same way out: a filter that empties a table must
 *  own the emptiness rather than leave bare headers explaining nothing. */
export function NoNodeOfSelectedKinds({
  where,
  onShowAll,
}: {
  /** Names what was emptied: « in this period » / « at this level ». */
  where: string;
  onShowAll: () => void;
}) {
  return (
    <EmptyNote>
      No node of the selected kinds {where}.{" "}
      <button
        type="button"
        data-testid="stats-show-all-kinds"
        onClick={onShowAll}
        className="underline decoration-dotted underline-offset-2 hover:text-text-2"
      >
        show all kinds
      </button>
    </EmptyNote>
  );
}

/** The value a partially filtered row shows in place of a number. */
export function FilteredValue({ testid }: { testid?: string }) {
  return (
    <Tooltip content={FILTERED_BY_KIND_COPY} side="top">
      <span className="italic text-text-3" data-testid={testid}>
        filtered
      </span>
    </Tooltip>
  );
}
