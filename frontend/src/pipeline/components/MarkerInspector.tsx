import { FileText, Inbox, Lock, Play, Square } from "lucide-react";
import { Chip, EmptyState, Section } from "../../shared/ui/kit";
import type { EditorNodeDef } from "../model/pipeline";
import "./inspect.css";

interface Props {
  node: EditorNodeDef;
}

/**
 * Read-only pane for a `start` / `end` marker selected outside a run (#684).
 * Markers are structural — no agent, model or prompt, and their ports are the
 * pipeline's contract — so nothing here is editable. Inside a run the pane is
 * `StartInspector` / `EndInspector` instead (see `resolveNodeInspector`).
 */
export default function MarkerInspector({ node }: Props) {
  const isStart = node.type === "start";
  const ports = isStart ? node.outputs : node.inputs;
  const help = isStart
    ? "Every run begins here: the prompt enters the pipeline. Structural: it cannot be edited, deleted or duplicated. Open a run to see its input."
    : "Every run ends here: the result leaves the pipeline. Structural: it cannot be edited, deleted or duplicated. Open a run to see why it ended.";
  return (
    <aside className="ei" data-testid="marker-inspector">
      <div className="ei-bar">
        <span className="ei-bar__title" title={help}>
          {isStart ? <Play size={14} strokeWidth={1.5} aria-hidden /> : <Square size={14} strokeWidth={1.5} aria-hidden />}
          <span>{isStart ? "Pipeline start" : "Pipeline end"}</span>
          <span className="ei-bar__sub">{node.id}</span>
        </span>
        <span className="ei-bar__actions">
          <span title="Structural marker: it cannot be edited, deleted or duplicated">
            <Chip icon={Lock}>marker</Chip>
          </span>
        </span>
      </div>
      <Section title={isStart ? "Outputs" : "Inputs"} count={ports.length} flush>
        {ports.length === 0 ? (
          <EmptyState icon={Inbox} layout="row">
            None
          </EmptyState>
        ) : (
          ports.map((p) => (
            <div key={p.name} className="ei-row">
              <FileText size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
              <span className="ei-row__name d-mono">{p.name}</span>
            </div>
          ))
        )}
      </Section>
    </aside>
  );
}
