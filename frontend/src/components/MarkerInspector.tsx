import type { NodeDef } from "../types";

interface Props {
  node: NodeDef;
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
  return (
    <aside
      className="marker-inspector flex h-full flex-col bg-bg-2"
      data-testid="marker-inspector"
    >
      <div className="border-b border-line px-3 py-2">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg" style={{ fontSize: "12.5px" }}>
            {isStart ? "Pipeline start" : "Pipeline end"}
          </span>
          <span
            className="rounded border border-line bg-bg-3 px-1.5 py-0.5 text-fg-3"
            style={{ fontSize: "10px", fontWeight: 500 }}
          >
            marker
          </span>
        </div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {node.id}
        </div>
      </div>

      <div className="flex-1 overflow-auto p-3 text-fg-3" style={{ fontSize: "11.5px" }}>
        <p>
          {isStart
            ? "Every run begins here: the user's prompt enters the pipeline through this marker."
            : "Every run ends here: the pipeline's result leaves through this marker."}
        </p>
        <p className="mt-2 text-fg-4">
          Structural marker — it cannot be edited, deleted or duplicated.
          {isStart ? " Open a run to see its actual input." : " Open a run to see why it ended."}
        </p>

        {ports.length > 0 && (
          <>
            <div className="mb-1 mt-3 font-medium text-fg-3" style={{ fontSize: "11px" }}>
              {isStart ? "Outputs" : "Inputs"}
            </div>
            <ul className="flex flex-col gap-1">
              {ports.map((p) => (
                <li key={p.name} className="font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
                  {p.name}
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
    </aside>
  );
}
