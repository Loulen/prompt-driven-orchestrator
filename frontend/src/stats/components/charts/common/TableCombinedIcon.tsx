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

/** A Pipeline row's name, cut with an ellipsis, with the `[⧉ N]` icon when it
 *  absorbs others. Sits in a flex row (`.sx-name`). */
export function PipelineName({
  row,
  onOpenMembers,
}: {
  row: AbsorbableRow;
  onOpenMembers?: (id: string) => void;
}) {
  const absorbed = row.absorbed?.length ?? 0;
  const name = (
    <span className="sx-ellipsis" title={row.name}>
      {row.name}
    </span>
  );
  if (!onOpenMembers || absorbed === 0) return name;
  return (
    <>
      {name}
      <CombinedIcon count={absorbed} onOpen={() => onOpenMembers(row.id)} />
    </>
  );
}
