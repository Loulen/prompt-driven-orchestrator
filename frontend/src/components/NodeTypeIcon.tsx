import { User, GitMerge, Play, Square, GitBranch, SquareTerminal } from "lucide-react";
import type { NodeType } from "../types";

interface IconProps {
  type: NodeType;
  size?: number;
  className?: string;
}

export function NodeTypeIcon({ type, size = 14, className }: IconProps) {
  switch (type) {
    case "merge":
      return <GitMerge data-testid="node-icon-merge" size={size} className={className} />;
    case "start":
      return <Play data-testid="node-icon-start" size={size} className={className} />;
    case "end":
      return <Square data-testid="node-icon-end" size={size} className={className} />;
    case "script":
      // #248: a script node runs deterministic bash, not an agent.
      return <SquareTerminal data-testid="node-icon-script" size={size} className={className} />;
    default:
      return <User data-testid="node-icon-agent" size={size} className={className} />;
  }
}

/**
 * #653 / ADR-0060: the canvas answers "does this Node fork its own worktree?"
 * without opening the inspector. One glyph, present or absent — it replaced the
 * pair of non-isolated / isolated markers, which read as two pseudo-types.
 *
 * A branch glyph, not a dotted card border: the border competed with the
 * selection ring and the status borders, and a marker mistaken for a selection
 * state costs more than it says.
 */
export function IsolationMarker({ isolated }: { isolated: boolean }) {
  if (!isolated) return null;
  return (
    <span
      data-testid="isolation-marker"
      title="Isolated worktree — this Node forks a sub-worktree of its own"
      className="ml-auto flex shrink-0 items-center"
    >
      <GitBranch size={11} className="text-acc" />
    </span>
  );
}
