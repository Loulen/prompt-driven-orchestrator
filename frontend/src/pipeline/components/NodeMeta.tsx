/**
 * The parts of a node card under its head: the meta grid (what runs the node,
 * then on a template its worktree and one line per output, in a Run its
 * iteration, time and cost) and the images of the Run's input on the Start
 * marker.
 */
import { Coins, FileCode, FileImage, Files, FileText, GitFork, Images, Repeat, Timer, type LucideIcon } from "lucide-react";
import type { PortType } from "../../shared/generated/PortType";
import { formatDuration } from "../../run/model/runDuration";
import { formatCostAmount } from "../../run/model/costLabel";
import { useNow } from "../../run/model/iterationDuration";
import type { ElementMarks } from "../../hooks/useElementMarks";
import { RUN_CARD } from "../model/runCardElements";
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
function Cell({
  icon: Icon,
  title,
  value,
  marks,
}: {
  icon: LucideIcon;
  title: string;
  value: string | null | undefined;
  marks?: ElementMarks;
}) {
  return (
    <span className="cv-n__cell" title={value ? `${title}: ${value}` : title} {...marks}>
      <Icon size={11} strokeWidth={1.5} aria-hidden />
      <span>{value || "·"}</span>
    </span>
  );
}

/** The icon of each port type, on the card's output lines. */
const PORT_TYPE_ICON: Record<PortType, LucideIcon> = {
  markdown: FileText,
  html: FileCode,
  image: FileImage,
  image_list: Images,
  files: Files,
};

/** One output of a template's card: its port type's icon and its name, on a line of its own. */
function OutputLine({ name, type }: { name: string; type: PortType }) {
  const Icon = PORT_TYPE_ICON[type] ?? FileText;
  return (
    <span className="cv-n__cell cv-n__out" title={`${name} · ${type}`} data-testid="node-output-line" data-port-type={type}>
      <Icon size={11} strokeWidth={1.5} role="img" aria-label={type} />
      <span>{name}</span>
    </span>
  );
}

const SHOWN: ElementMarks = {};

/**
 * The card's meta grid, two columns: what runs the node, then on a template
 * its worktree and one line per output (the card grows with them), in a Run
 * its iteration, time and cost. In a Run, `marks` leaves out the cells the
 * Apparence hides (`null`); with none left, there is no grid.
 */
export function NodeMeta({
  data,
  run,
  marks = () => SHOWN,
}: {
  data: EditNodeData;
  run: EditNodeData["run"];
  marks?: (id: string) => ElementMarks | null;
}) {
  const live = run != null && (data.status === "running" || data.status === "awaiting_user");
  const now = useNow(live && run?.completedAt == null);
  const agent = data.agent ?? { short: "", title: "Agent" };
  const AgentIcon = nodeTypeGlyph(data.nodeType, data.interactive, data.orchestrator === true).icon;
  if (data.inRun !== true) {
    return (
      <div className="cv-n__grid">
        <Cell icon={AgentIcon} title={agent.title} value={agent.short} />
        <Cell icon={GitFork} title="Worktree" value={data.isolated === true ? "isolated" : "run"} />
        {data.outputs.map((p) => (
          <OutputLine key={p.name} name={p.name} type={p.port_type ?? "markdown"} />
        ))}
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
  const cells = [
    { id: RUN_CARD.agent, icon: AgentIcon, title: agent.title, value: agent.short },
    { id: RUN_CARD.iteration, icon: Repeat, title: "Iteration", value: iter },
    { id: RUN_CARD.elapsed, icon: Timer, title: "Elapsed", value: elapsed },
    { id: RUN_CARD.cost, icon: Coins, title: "Cost", value: cost },
  ].flatMap((cell) => {
    const m = marks(cell.id);
    return m ? [{ ...cell, marks: m }] : [];
  });
  if (cells.length === 0) return null;
  return (
    <div className="cv-n__grid">
      {cells.map(({ id, ...cell }) => (
        <Cell key={id} {...cell} />
      ))}
    </div>
  );
}
