import { Fragment, type CSSProperties } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import { rowMetric } from "../../../model/cost";
import { CostCell } from "./CostCells";
import { treeKey, type CostTree } from "./costTree";

/** The chevron that unfolds a row's level in place. */
export function TreeToggle({
  name,
  open,
  onToggle,
}: {
  name: string;
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={`${open ? "Collapse" : "Expand"} ${name}`}
      aria-expanded={open}
      data-testid="stats-tree-toggle"
      onClick={onToggle}
      className="sx-chev"
    >
      {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
    </button>
  );
}

export function CostTreeRows({
  rows,
  ancestors,
  tree,
  expanded,
  onToggle,
  harnesses,
  unit,
}: {
  rows: StatsCostEntity[];
  ancestors: StatsCostEntity[];
  tree: CostTree;
  expanded: Set<string>;
  onToggle: (key: string) => void;
  harnesses: string[];
  unit: "Run" | "execution";
}) {
  return rows.map((row) => {
    const key = treeKey(ancestors, row);
    const kids = tree.children(row, ancestors) ?? [];
    const open = tree.open(row, ancestors);
    const isOpen = expanded.has(key);
    const name = tree.renderName(row);
    const rowUnit = tree.unit(row, ancestors);
    return (
      <Fragment key={key}>
        <tr
          className="group sx-tree"
          style={{ "--sx-level": ancestors.length - tree.base.length } as CSSProperties}
          data-testid="stats-tree-row"
        >
          <td>
            <span className="sx-name">
              {kids.length > 0 ? (
                <TreeToggle name={row.name} open={isOpen} onToggle={() => onToggle(key)} />
              ) : (
                <span className="sx-chev-gap" aria-hidden="true" />
              )}
              {open ? (
                <button
                  type="button"
                  aria-label={`Open ${row.name}`}
                  title={row.name}
                  onClick={open}
                  className="sx-open"
                >
                  {name}
                </button>
              ) : (
                name
              )}
            </span>
          </td>
          <td className="sx-r">
            <CostCell unit={rowUnit} metric={rowMetric(row)} />
          </td>
          {harnesses.map((harness) => (
            <td key={harness} className="sx-r">
              <CostCell unit={rowUnit} metric={row.harnesses.find((item) => item.harness === harness)} />
            </td>
          ))}
        </tr>
        {isOpen && kids.length > 0 ? (
          <CostTreeRows
            rows={kids}
            ancestors={[...ancestors, row]}
            tree={tree}
            expanded={expanded}
            onToggle={onToggle}
            harnesses={harnesses}
            unit={unit}
          />
        ) : null}
      </Fragment>
    );
  });
}
