import { User, GitMerge, Play, Square, Code, FileText, SquareTerminal } from "lucide-react";
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

export function CodeDocMarker({ type }: { type: NodeType }) {
  if (type === "code-mutating") {
    return (
      <span data-testid="code-doc-marker" data-marker-type="code-mutating" className="ml-auto flex shrink-0 items-center">
        <Code size={11} className="text-acc" />
      </span>
    );
  }
  if (type === "doc-only") {
    return (
      <span data-testid="code-doc-marker" data-marker-type="doc-only" className="ml-auto flex shrink-0 items-center">
        <FileText size={11} className="text-fg-4" />
      </span>
    );
  }
  return null;
}
