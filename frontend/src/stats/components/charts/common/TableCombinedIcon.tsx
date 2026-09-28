import { CombinedIcon } from "../../StatsAbsorption";
import type { MasterSelection } from "../../../store/absorptionSelection";
import type { AbsorbableRow } from "../../../model/statsAbsorption";

/** The `[⧉ N]` icon of an absorbent row of a detail table. */
export function TableCombinedIcon({
  selection,
  row,
}: {
  selection: MasterSelection | undefined;
  row: AbsorbableRow;
}) {
  const absorbed = row.absorbed?.length ?? 0;
  if (!selection || absorbed === 0) return null;
  return (
    <CombinedIcon
      count={absorbed}
      dimension={selection.dimension}
      onOpen={() => selection.onOpenMembers(row.id)}
    />
  );
}

/** A Pipeline row's name, with the `[⧉ N]` icon when it absorbs others. */
export function PipelineName({
  row,
  onOpenMembers,
}: {
  row: AbsorbableRow;
  onOpenMembers?: (id: string) => void;
}) {
  const absorbed = row.absorbed?.length ?? 0;
  if (!onOpenMembers || absorbed === 0) return <>{row.name}</>;
  return (
    <span className="inline-flex items-center gap-1.5">
      {row.name}
      <CombinedIcon count={absorbed} onOpen={() => onOpenMembers(row.id)} />
    </span>
  );
}
