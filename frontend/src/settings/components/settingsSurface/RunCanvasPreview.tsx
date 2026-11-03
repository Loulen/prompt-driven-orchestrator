import { NodeCard } from "../../../pipeline/components/NodeCard";
import { NodeCardHead } from "../../../pipeline/components/NodeCardParts";
import { NodeMeta } from "../../../pipeline/components/NodeMeta";
import { NodeTypeIcon } from "../../../pipeline/components/NodeTypeIcon";
import type { EditNodeData } from "../../../pipeline/components/EditNode";
import "../../../pipeline/components/canvas.css";

const cost = (usd: number) => ({ usd, form: "reported" as const, reported_in_usd: true, partial: false, executions: 1, readable_executions: 1 });

/** A sample Run of three nodes, without extension marks. */
const SAMPLE: EditNodeData[] = [
  {
    label: "plan",
    nodeId: "plan",
    nodeType: "agent",
    interactive: true,
    status: "completed",
    inputs: [],
    outputs: [],
    inRun: true,
    agent: { short: "Sonnet", title: "Agent" },
    run: { iter: 1, iterations: 1, startedAt: "2026-01-01T10:00:00Z", completedAt: "2026-01-01T10:03:00Z", cost: cost(0.31) },
  },
  {
    label: "tdd",
    nodeId: "tdd",
    nodeType: "agent",
    interactive: false,
    status: "completed",
    inputs: [],
    outputs: [],
    inRun: true,
    agent: { short: "Opus", title: "Agent" },
    run: { iter: 2, iterations: 2, startedAt: "2026-01-01T10:03:00Z", completedAt: "2026-01-01T10:10:00Z", cost: cost(1.24) },
  },
  {
    label: "merge",
    nodeId: "merge",
    nodeType: "agent",
    interactive: false,
    status: "pending",
    inputs: [],
    outputs: [],
    inRun: true,
    agent: { short: "Sonnet", title: "Agent" },
    run: { iter: 0, iterations: 0, startedAt: null, completedAt: null, cost: null },
  },
];

/**
 * The preview of the Run canvas block: the canvas's own cards on a sample Run, detailed
 * or on one line as « Compact nodes » says.
 */
export default function RunCanvasPreview({ compact }: { compact: boolean }) {
  return (
    <figure className="set-preview" data-testid="appearance-preview-run-canvas" data-compact={compact || undefined}>
      <div className="set-preview__canvas">
        {SAMPLE.map((data, index) => (
          <div key={data.nodeId} className="set-preview__step">
            {index > 0 && <span className="set-preview__edge" aria-hidden />}
            <NodeCard status={data.status} compact={compact}>
              <NodeCardHead
                type={<NodeTypeIcon type={data.nodeType} interactive={data.interactive} size={12} className="cv-n__type" />}
                name={data.label}
                status={data.status}
              />
              {!compact && <NodeMeta data={data} run={data.run} />}
            </NodeCard>
          </div>
        ))}
      </div>
      <figcaption className="set-preview__caption">Sample run</figcaption>
    </figure>
  );
}
