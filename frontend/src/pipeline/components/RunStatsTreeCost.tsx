/**
 * The Est. cost cell of Run info for a Run that has child runs: its Coût de
 * l'arbre (ADR-0101), then « ⑂ N », the number of
 * descendants. The tooltip breaks it into Parent run / Child runs (N), and
 * each depth when the tree is deeper than one level. A Run without child runs
 * keeps the plain cell.
 */
import { Coins, GitFork } from "lucide-react";
import type { TreeCost } from "../../shared/generated/TreeCost";
import type { ElementMarks } from "../../hooks/useElementMarks";
import { runTreeCostLines, treeCostLabel } from "../../run/model/treeCost";
import { Tooltip } from "../../shared/ui/tooltip";

export function TreeCostCell({ tree, marks }: { tree: TreeCost; marks: ElementMarks }) {
  const label = treeCostLabel(tree);
  const lines = runTreeCostLines(label);
  return (
    <Tooltip
      content={
        <span className="pi-tree-tip" data-testid="stat-cost-tooltip">
          {lines.map((line) => (
            <span key={line}>{line}</span>
          ))}
        </span>
      }
    >
      {/* Focusable: the breakdown lives in the tooltip, the keyboard reaches it too. */}
      <span className="pi-stats__cell" tabIndex={0} data-testid="stat-cost" data-tree {...marks}>
        <Coins size={13} strokeWidth={1.5} aria-hidden />
        <span className="d-sr">Est. cost: {lines.join(" ")}</span>
        <span aria-hidden data-testid="stat-cost-total">
          {label.total.text}
          {label.total.dagger && <span className="ei-warn">†</span>}
        </span>
        {/* An unavailable total reads « — » alone: the count goes with an amount. */}
        {!label.unavailable && (
          <span className="pi-stats__tree" aria-hidden data-testid="stat-cost-descendants">
            <GitFork size={11} strokeWidth={1.5} />
            {label.descendants}
          </span>
        )}
      </span>
    </Tooltip>
  );
}
