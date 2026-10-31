/**
 * The parts of a node card under its head: the meta grid (what runs the node,
 * then on a template its inputs, worktree and outputs, in a Run its iteration,
 * time and cost) and the images of the Run's input on the Start marker.
 */
import { ArrowDownToLine, ArrowUpFromLine, Coins, GitFork, Repeat, Timer, type LucideIcon } from "lucide-react";
import { formatDuration } from "../../run/model/runDuration";
import { formatCostAmount } from "../../run/model/costLabel";
import { useNow } from "../../run/model/iterationDuration";
import type { EditNodeData } from "./EditNode";
import { nodeTypeGlyph } from "../model/nodeTypeGlyph";

/** Images uploaded with the Run's input, named, on the Start marker (#145). */
export function InputImages({ names }: { names: string[] }) {
  return (
    <div className="cv-n__images" data-testid="start-node-images">
      {names.map((name) => (
        <div key={name} data-testid="start-node-image-chip" title={name} className="cv-n__img">
          <span>{name}</span>
        </div>
      ))}
    </div>
  );
}

/** One cell of the card's meta grid: an icon, a value, the name in the tooltip. */
function Cell({ icon: Icon, title, value }: { icon: LucideIcon; title: string; value: string | null | undefined }) {
  return (
    <span className="cv-n__cell" title={value ? `${title}: ${value}` : title}>
      <Icon size={11} strokeWidth={1.5} aria-hidden />
      <span>{value || "·"}</span>
    </span>
  );
}

/**
 * The card's meta grid, two columns: what runs the node, then on a template
 * its inputs, worktree and outputs, in a Run its iteration, time and cost.
 */
export function NodeMeta({ data, run }: { data: EditNodeData; run: EditNodeData["run"] }) {
  const live = run != null && (data.status === "running" || data.status === "awaiting_user");
  const now = useNow(live && run?.completedAt == null);
  const agent = data.agent ?? { short: "", title: "Agent" };
  const AgentIcon = nodeTypeGlyph(data.nodeType, data.interactive, data.orchestrator === true).icon;
  if (data.inRun !== true) {
    return (
      <div className="cv-n__grid">
        <Cell icon={AgentIcon} title={agent.title} value={agent.short} />
        <Cell icon={ArrowDownToLine} title="Inputs" value={data.inputs.map((p) => p.name).join(", ")} />
        <Cell icon={GitFork} title="Worktree" value={data.isolated === true ? "isolated" : "run"} />
        <Cell icon={ArrowUpFromLine} title="Outputs" value={data.outputs.map((p) => p.name).join(", ")} />
      </div>
    );
  }
  const elapsed =
    run?.startedAt != null
      ? formatDuration(
          (run.completedAt ? Date.parse(run.completedAt) : live ? now : Number.NaN) - Date.parse(run.startedAt),
        )
      : null;
  const cost =
    run?.cost && run.cost.usd != null
      ? formatCostAmount(run.cost.usd, run.cost.partial, run.cost.form !== "reported")
      : null;
  const iter = run && run.iter > 0 ? (data.loopBadge || run.iterations > 1 ? `×${run.iter}` : String(run.iter)) : null;
  return (
    <div className="cv-n__grid">
      <Cell icon={AgentIcon} title={agent.title} value={agent.short} />
      <Cell icon={Repeat} title="Iteration" value={iter} />
      <Cell icon={Timer} title="Elapsed" value={elapsed} />
      <Cell icon={Coins} title="Cost" value={cost} />
    </div>
  );
}
