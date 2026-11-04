import { GitFork, Play, Square } from "lucide-react";
import type { NodeType } from "../../shared/generated/NodeType";
import type { ElementMarks } from "../../hooks/useElementMarks";
import { nodeTypeGlyph } from "../model/nodeTypeGlyph";

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
    default: {
      const { icon: Icon, label } = nodeTypeGlyph(type, interactive, orchestrator);
      const testId = type === "script" ? "node-icon-script" : "node-icon-agent";
      return <Icon data-testid={testId} aria-label={label} role="img" {...props} />;
    }
  }
}

/**
 * #653 / ADR-0060: the canvas answers "does this Node fork its own worktree?"
 * without opening the inspector. One glyph, present or absent — it replaced the
 * pair of non-isolated / isolated markers, which read as two pseudo-types.
 */
export function IsolationMarker({ isolated, marks }: { isolated: boolean; marks?: ElementMarks }) {
  if (!isolated) return null;
  return (
    <span data-testid="isolation-marker" title="Isolated worktree" className="cv-n__flag" {...marks}>
      <GitFork size={11} strokeWidth={1.5} aria-label="Isolated worktree" />
    </span>
  );
}
