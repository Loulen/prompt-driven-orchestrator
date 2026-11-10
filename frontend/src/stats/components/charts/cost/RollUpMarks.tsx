import { GitFork } from "lucide-react";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import { plural } from "../../../../shared/lib/plural";

/** A small uppercase tag after a roll-up row's name (« this run's own nodes »,
 *  « 14 runs · 2 levels »). */
export function RollUpTag({ children }: { children: React.ReactNode }) {
  return (
    <span className="sx-tag" data-testid="stats-roll-up-tag">
      {children}
    </span>
  );
}

/** « ⑂ N » after a root Pipeline that orchestrated N child runs. */
export function RollUpBadge({ row }: { row: StatsCostEntity }) {
  const childRuns = row.roll_up?.children.runs ?? 0;
  if (childRuns === 0) return null;
  return (
    <span
      role="img"
      aria-label={`Orchestrated ${plural(childRuns, "child run")}`}
      data-testid="stats-roll-up-badge"
      className="sx-fork"
    >
      <GitFork size={10} strokeWidth={1.5} aria-hidden="true" />
      {childRuns}
    </span>
  );
}
