import type { EndNodeInfo } from "../../shared/generated/EndNodeInfo";

interface Props {
  endNode: EndNodeInfo;
}

export default function EndInspector({ endNode }: Props) {
  return (
    <aside className="end-inspector flex h-full flex-col bg-surface-1">
      <div className="border-b border-line px-3 py-2">
        <div className="flex items-center gap-2">
          <span className="font-medium text-text" style={{ fontSize: "12.5px" }}>
            Run end
          </span>
          <span
            className="runtime-badge rounded border border-primary/40 bg-primary/10 px-1.5 py-0.5 text-primary"
            style={{ fontSize: "10px", fontWeight: 500 }}
          >
            runtime
          </span>
        </div>
        <div
          className="mt-0.5 font-mono text-text-3"
          style={{ fontSize: "10px" }}
        >
          {endNode.id}
        </div>
      </div>

      <div className="flex-1 overflow-auto p-3">
        <div
          className="mb-2 text-text-3"
          style={{ fontSize: "11px", fontWeight: 500 }}
        >
          Termination reasons
        </div>
        <div className="flex flex-col gap-2">
          {endNode.ports.map((port) => (
            <div
              key={port.port_name}
              className="rounded border border-line bg-surface-2 px-2.5 py-2"
            >
              <div className="flex items-center gap-2">
                <span
                  className={`h-2 w-2 shrink-0 rounded-full ${
                    port.status === "received" ? "bg-st-done" : "bg-text-3"
                  }`}
                />
                <span className="font-mono text-text-2" style={{ fontSize: "11px" }}>
                  {port.port_name}
                </span>
                <span
                  className={`ml-auto rounded px-1.5 py-0.5 ${
                    port.status === "received"
                      ? "bg-st-done/10 text-st-done"
                      : "bg-surface-2 text-text-3"
                  }`}
                  style={{ fontSize: "9px", fontWeight: 500 }}
                >
                  {port.status}
                </span>
              </div>
              {port.reason && (
                <div
                  className="mt-1.5 rounded border border-line-soft bg-surface-0 px-2 py-1 font-mono text-text-3"
                  style={{ fontSize: "10.5px", lineHeight: "1.5" }}
                >
                  {port.reason}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </aside>
  );
}
