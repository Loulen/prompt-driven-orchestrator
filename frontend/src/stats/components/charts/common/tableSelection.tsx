import SelectControl from "../../../../components/SelectControl";
import { GlobalAbsorptionIcon } from "../../StatsAbsorption";
import type { MasterSelection } from "../../../store/absorptionSelection";
import type { CoupleRow } from "../../../model/statsAbsorption";
import { TableCombinedIcon } from "./TableCombinedIcon";

/**
 * The Node rows of a detail table carry the same selection as the master list
 * (#892): the ring before the name, Ctrl/Cmd-click and Shift-click on the row,
 * Space on the focused row, the selected background, the `[⧉ N]` icon. A plain
 * click is left to the row.
 */
export function tableSelection(
  selection: MasterSelection | undefined,
  { id, name }: { id: string; name: string },
  order: string[],
) {
  const selectable = selection?.isSelectable(id) ?? false;
  const checked = selectable && (selection?.selected.has(id) ?? false);
  return {
    className: `${checked ? "bg-acc-bg" : ""} ${selection?.flashId === id ? "ring-1 ring-acc" : ""}`,
    rowProps: selectable
      ? {
          "data-checked": checked,
          onClick: (event: React.MouseEvent) => {
            if (event.ctrlKey || event.metaKey || event.shiftKey) {
              event.preventDefault();
              selection!.toggle(id, event.shiftKey, order);
            }
          },
          onKeyDown: (event: React.KeyboardEvent) => {
            if (event.key === " " && event.target === event.currentTarget) {
              event.preventDefault();
              selection!.toggle(id, false, order);
            }
          },
        }
      : {},
    control: !selection ? null : selectable ? (
      <SelectControl
        selected={checked}
        dotClass={null}
        label={`Select ${name}`}
        testId="stats-row-select"
        onSelect={(event) => selection.toggle(id, event.shiftKey, order)}
      />
    ) : (
      <span className="w-4 shrink-0" aria-hidden="true" />
    ),
  };
}

/**
 * What a Node row's couples carry of the absorption (#906): the selection of
 * that one Node's couples — its own scope, so couples of two Nodes never mix —
 * and, per couple, the ring, the `[⧉ N]` of a local absorbent and the greyed
 * mark of a global absorption.
 */
export type CoupleSelection = (node: {
  id: string;
  name: string;
}) => MasterSelection | undefined;

export function coupleLine(
  selection: MasterSelection | undefined,
  couples: CoupleRow[],
): {
  rows: CoupleRow[];
  pick: (couple: CoupleRow) => ReturnType<typeof tableSelection>;
  marks: (couple: CoupleRow) => React.ReactNode;
} {
  const order = couples.map((couple) => couple.id);
  return {
    rows: couples,
    pick: (couple) => tableSelection(selection, couple, order),
    marks: (couple) => (
      <>
        <TableCombinedIcon selection={selection} row={couple} />
        {selection && (couple.global_absorbed?.length ?? 0) > 0 && (
          <GlobalAbsorptionIcon
            count={couple.global_absorbed!.length}
            onOpen={() => selection.onOpenGlobal(couple.id)}
          />
        )}
      </>
    ),
  };
}
