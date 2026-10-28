import type { TreeRow } from "../model/skillTree";
import { checkState, skillIdsOf } from "../model/skillExport";
import type { SkillBankPanelState } from "../store/useSkillBankPanel";
import type { SkillBank } from "../model/skill";

/**
 * The box at the start of a tree row: the bank's multiple selection, for an
 * export. A folder's box checks every skill under it (half-checked when some
 * are); ⇧-click checks the rows between the last box clicked and this one.
 */
export default function TreeRowCheck({
  row,
  bank,
  panel,
}: {
  row: TreeRow;
  bank: SkillBank;
  panel: SkillBankPanelState;
}) {
  const ids = skillIdsOf(row.ref, bank);
  const state = checkState(panel.checked, ids);
  const name = row.folder?.name ?? row.skill?.name ?? "";
  return (
    <input
      type="checkbox"
      aria-label={row.ref.kind === "folder" ? `Select every skill of ${name}` : `Select ${name}`}
      data-testid={`tree-check-${row.ref.kind}-${row.ref.id}`}
      checked={state === "all"}
      disabled={ids.length === 0}
      ref={(el) => {
        if (el) el.indeterminate = state === "some";
      }}
      onChange={() => {
        // The click below decides: it knows whether ⇧ was held.
      }}
      onClick={(event) => {
        event.stopPropagation();
        panel.toggleCheck(row.ref, event.shiftKey);
      }}
      onDoubleClick={(event) => event.stopPropagation()}
      className="h-3 w-3 shrink-0 cursor-pointer accent-primary disabled:cursor-default disabled:opacity-40"
    />
  );
}
