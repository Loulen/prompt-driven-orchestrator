import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal, ExternalLink, CheckCircle, AlertCircle } from "lucide-react";
import Convert from "ansi-to-html";
import type { NodeState, NodeStatus } from "../types";
import { markNodeDone, attachSession, fetchPane } from "../api";

const STATUS_LABELS: Record<NodeStatus, string> = {
  pending: "Pending",
  running: "Running",
  awaiting_user: "Awaiting User",
  completed: "Completed",
  failed: "Failed",
};

const ansiConverter = new Convert({
  fg: "#e6e8eb",
  bg: "#0f1115",
  newline: true,
  escapeXML: true,
});

const POLL_FAST = 1000;
const POLL_SLOW = 5000;

function pollInterval(status: NodeStatus): number | null {
  switch (status) {
    case "running":
    case "awaiting_user":
      return POLL_FAST;
    case "completed":
    case "failed":
      return POLL_SLOW;
    case "pending":
      return null;
  }
}

interface Props {
  node: NodeState;
  runId: string;
  isArchived?: boolean;
}

export default function NodeDetailPanel({ node, runId, isArchived }: Props) {
  const [terminalHtml, setTerminalHtml] = useState<string>("");
  const terminalRef = useRef<HTMLPreElement>(null);
  const sessionName = `maestro-${runId}-${node.node_id}-iter-${node.iter}`;
  const interval = pollInterval(node.status);

  useEffect(() => {
    if (interval === null) return;

    let cancelled = false;

    async function poll() {
      try {
        const resp = await fetchPane(runId, node.node_id, node.iter);
        if (!cancelled) {
          setTerminalHtml(ansiConverter.toHtml(resp.content));
        }
      } catch {
        // ignore fetch errors during polling
      }
    }

    poll();
    const timer = setInterval(poll, interval);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [interval, node.node_id, node.iter, runId]);

  // Auto-scroll terminal to bottom on content change
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [terminalHtml]);

  const handleOpenTerminal = useCallback(async () => {
    try {
      await attachSession(sessionName);
    } catch (e) {
      console.error("Failed to attach terminal:", e);
    }
  }, [sessionName]);

  const handleMarkComplete = useCallback(async () => {
    try {
      await markNodeDone(runId, node.node_id, node.iter);
    } catch (e) {
      console.error("Failed to mark node done:", e);
    }
  }, [runId, node.node_id, node.iter]);

  const showOpenTerminal = node.status !== "pending";

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

      {/* Awaiting user banner */}
      {node.status === "awaiting_user" && (
        <div className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2">
          <AlertCircle size={14} className="shrink-0 text-st-await" />
          <span className="text-st-await" style={{ fontSize: "11.5px", fontWeight: 500 }}>
            Awaiting user — attach terminal and interact, then mark complete
          </span>
        </div>
      )}

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
          className="terminal-pane h-[200px] overflow-auto bg-bg-0 p-2 font-mono text-fg-2"
          style={{ fontSize: "10.5px", lineHeight: "1.5" }}
          dangerouslySetInnerHTML={
            terminalHtml ? { __html: terminalHtml } : undefined
          }
        >
          {!terminalHtml && (
            <span className="text-fg-4">
              {terminalPlaceholder(node)}
            </span>
          )}
        </pre>
      </div>

      {/* Actions */}
      <div className="flex flex-col gap-1.5 border-t border-line px-3 py-2">
        {showOpenTerminal && (
          <button
            onClick={handleOpenTerminal}
            className={`flex w-full items-center justify-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 transition-colors ${
              isArchived
                ? "cursor-not-allowed text-fg-4"
                : "text-fg-2 hover:bg-bg-4 hover:text-fg"
            }`}
            style={{ fontSize: "11.5px" }}
            disabled={isArchived}
          >
            <ExternalLink size={12} />
            Open terminal
          </button>
        )}

        {node.status === "awaiting_user" && !isArchived && (
          <button
            onClick={handleMarkComplete}
            className="flex w-full items-center justify-center gap-1.5 rounded-md border border-st-done/40 bg-st-done-bg px-3 py-1.5 text-st-done transition-colors hover:border-st-done/60 hover:bg-st-done/20"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            <CheckCircle size={12} />
            Mark complete
          </button>
        )}
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
    case "awaiting_user":
      return "Waiting for user interaction...";
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
