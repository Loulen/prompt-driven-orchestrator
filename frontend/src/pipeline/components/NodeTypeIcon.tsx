import { Bot, GitFork, Network, Play, Square, SquareTerminal, UserRound } from "lucide-react";
import type { NodeType } from "../../shared/generated/NodeType";

interface IconProps {
  type: NodeType;
  /** An agent the user talks with. */
  interactive?: boolean;
  /** An agent that orchestrates child Runs (ADR-0064). */
  orchestrator?: boolean;
  size?: number;
  className?: string;
}

/** What kind of node this is: an agent (interactive, orchestrator), a script, Start or End. */
export function NodeTypeIcon({ type, interactive, orchestrator, size = 14, className }: IconProps) {
  const props = { size, strokeWidth: 1.5, className };
  switch (type) {
    case "start":
      return <Play data-testid="node-icon-start" {...props} />;
    case "end":
      return <Square data-testid="node-icon-end" {...props} />;
    case "script":
      // #248: a script node runs deterministic bash, not an agent.
      return <SquareTerminal data-testid="node-icon-script" aria-label="Script" role="img" {...props} />;
    default: {
      const [Icon, label] = orchestrator ? [Network, "Orchestrator"] : interactive ? [UserRound, "Interactive agent"] : [Bot, "Agent"];
      return <Icon data-testid="node-icon-agent" aria-label={label} role="img" {...props} />;
    }
  }
}

/**
 * #653 / ADR-0060: the canvas answers "does this Node fork its own worktree?"
 * without opening the inspector. One glyph, present or absent — it replaced the
 * pair of non-isolated / isolated markers, which read as two pseudo-types.
 */
export function IsolationMarker({ isolated }: { isolated: boolean }) {
  if (!isolated) return null;
  return (
    <span data-testid="isolation-marker" title="Isolated worktree" className="cv-n__flag">
      <GitFork size={11} strokeWidth={1.5} aria-label="Isolated worktree" />
    </span>
  );
}
