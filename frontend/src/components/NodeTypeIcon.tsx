import { User, GitBranch, RefreshCw, GitMerge, Play, Square, Code, FileText } from "lucide-react";
import { ForEachIcon } from "./ForEachNode";
import type { NodeType } from "../types";

interface IconProps {
  type: NodeType;
  size?: number;
  className?: string;
}

export function NodeTypeIcon({ type, size = 14, className }: IconProps) {
  switch (type) {
    case "switch":
      return <GitBranch data-testid="node-icon-switch" size={size} className={className} />;
    case "loop":
      return <RefreshCw data-testid="node-icon-loop" size={size} className={className} />;
    case "for-each":
      return <span data-testid="node-icon-foreach" className={className}><ForEachIcon /></span>;
    case "merge":
      return <GitMerge data-testid="node-icon-merge" size={size} className={className} />;
    case "start":
      return <Play data-testid="node-icon-start" size={size} className={className} />;
    case "end":
      return <Square data-testid="node-icon-end" size={size} className={className} />;
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
