import { Bot, Network, SquareTerminal, UserRound, type LucideIcon } from "lucide-react";
import type { NodeType } from "../../shared/generated/NodeType";

/** The glyph and name of a work node's type: an orchestrator, a script, an interactive agent, an agent. */
export function nodeTypeGlyph(
  type: NodeType,
  interactive?: boolean,
  orchestrator?: boolean,
): { icon: LucideIcon; label: string } {
  // #248: a script node runs deterministic bash, not an agent.
  if (type === "script") return { icon: SquareTerminal, label: "Script" };
  if (orchestrator) return { icon: Network, label: "Orchestrator" };
  if (interactive) return { icon: UserRound, label: "Interactive agent" };
  return { icon: Bot, label: "Agent" };
}
