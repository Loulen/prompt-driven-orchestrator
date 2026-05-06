import { useEffect, useRef, useState } from "react";
import { Terminal, ExternalLink } from "lucide-react";
import type { NodeState, NodeStatus } from "../types";

const STATUS_LABELS: Record<NodeStatus, string> = {
  pending: "Pending",
  running: "Running",
  completed: "Completed",
  failed: "Failed",
};

interface Props {
  node: NodeState;
  runId: string;
  isArchived?: boolean;
}

export default function NodeDetailPanel({ node, runId, isArchived }: Props) {
  const [terminalContent, setTerminalContent] = useState<string>("");
  const terminalRef = useRef<HTMLPreElement>(null);

  // Poll tmux capture-pane for terminal preview
  useEffect(() => {
    if (node.status !== "running") return;

    const sessionName = `maestro-${runId}-${node.node_id}-iter-${node.iter}`;

    async function poll() {
      try {
        // In the real implementation, this would be an API endpoint
        // For now we just show a placeholder
        setTerminalContent(
          `[tmux session: ${sessionName}]\n\nTerminal preview will appear here when the session is active.`,
        );
      } catch {
        // ignore fetch errors
      }
    }

    poll();
    const interval = setInterval(poll, 2000);
    return () => clearInterval(interval);
  }, [node.status, node.node_id, node.iter, runId]);

  return (
    <aside className="flex w-[340px] shrink-0 flex-col border-l border-line bg-bg-2">
      {/* Header */}
      <div className="border-b border-line px-3 py-2">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg" style={{ fontSize: "12.5px" }}>
            {node.node_id}
          </span>
          <span
            className="rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3"
            style={{ fontSize: "10px", fontWeight: 500 }}
          >
            {STATUS_LABELS[node.status] ?? node.status}
          </span>
        </div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "10px" }}>
          iter {node.iter}
          {node.started_at && ` · started ${formatTime(node.started_at)}`}
          {node.completed_at && ` · ended ${formatTime(node.completed_at)}`}
        </div>
      </div>

      {/* Terminal preview */}
      <div className="flex-1 overflow-hidden">
        <div
          className="flex items-center gap-1.5 border-b border-line px-3 py-1.5 text-fg-3"
          style={{ fontSize: "11px" }}
        >
          <Terminal size={12} />
          Terminal Preview
        </div>
        <pre
          ref={terminalRef}
          className="h-[200px] overflow-auto bg-bg-0 p-2 font-mono text-fg-2"
          style={{ fontSize: "10.5px", lineHeight: "1.5" }}
        >
          {terminalContent || (
            <span className="text-fg-4">
              {terminalPlaceholder(node)}
            </span>
          )}
        </pre>
      </div>

      {/* Actions */}
      <div className="border-t border-line px-3 py-2">
        <button
          className={`flex w-full items-center justify-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 transition-colors ${
            isArchived
              ? "cursor-not-allowed text-fg-4"
              : "text-fg-2 hover:bg-bg-4 hover:text-fg"
          }`}
          style={{ fontSize: "11.5px" }}
          title={
            isArchived
              ? "Run is archived. History is queryable via GET /runs/:id/events."
              : "Open terminal — full implementation in Slice 8"
          }
          disabled={isArchived}
        >
          <ExternalLink size={12} />
          Open terminal
        </button>
      </div>

      {/* Initial prompt section */}
      <div className="border-t border-line">
        <div
          className="flex items-center gap-1.5 px-3 py-1.5 text-fg-3"
          style={{ fontSize: "11px" }}
        >
          Initial Prompt
        </div>
        <pre
          className="max-h-[200px] overflow-auto bg-bg-0 p-2 font-mono text-fg-3"
          style={{ fontSize: "10px", lineHeight: "1.5" }}
        >
          Prompt preview available in run events.
        </pre>
      </div>
    </aside>
  );
}

function terminalPlaceholder(node: NodeState): string {
  switch (node.status) {
    case "pending":
      return "Waiting to start...";
    case "completed":
      return "Session ended.";
    case "failed":
      return `Failed: ${node.failure_reason ?? "unknown reason"}`;
    case "running":
      return "Connecting...";
  }
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return iso;
  }
}
