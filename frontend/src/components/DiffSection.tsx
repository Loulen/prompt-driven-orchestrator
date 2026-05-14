import { useState, useEffect, useMemo } from "react";
import { ChevronRight, ChevronDown } from "lucide-react";
import { fetchRunDiff, fetchNodeDiff } from "../api";
import type { RunState } from "../types";

interface Props {
  run: RunState | null;
}

export default function DiffSection({ run }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [diff, setDiff] = useState("");
  const [loading, setLoading] = useState(false);
  const [selectedNode, setSelectedNode] = useState<string>("");

  const codeMutatingNodes = useMemo(() => {
    if (!run) return [];
    return run.node_defs
      .filter((nd) => nd.node_type === "code-mutating")
      .filter((nd) => {
        const ns = run.nodes[nd.id];
        return ns && ns.status === "completed";
      });
  }, [run]);

  useEffect(() => {
    if (!expanded || !run) return;
    setLoading(true);
    const promise =
      selectedNode === ""
        ? fetchRunDiff(run.run_id)
        : fetchNodeDiff(run.run_id, selectedNode);
    promise
      .then(setDiff)
      .catch(() => setDiff(""))
      .finally(() => setLoading(false));
  }, [expanded, run, selectedNode]);

  if (!run) return null;

  return (
    <div data-testid="diff-section" className="border-t border-line">
      <button
        onClick={() => setExpanded((e) => !e)}
        className="flex w-full items-center gap-1.5 px-3 py-2 text-fg-2 transition-colors hover:bg-bg-3 cursor-pointer"
        style={{ fontSize: "11.5px" }}
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <span className="font-medium">Diff</span>
      </button>

      {expanded && (
        <div data-testid="diff-content" className="px-3 pb-3">
          {codeMutatingNodes.length > 0 && (
            <select
              data-testid="diff-node-select"
              value={selectedNode}
              onChange={(e) => setSelectedNode(e.target.value)}
              className="mb-2 w-full rounded border border-line bg-bg-3 px-2 py-1 font-mono text-fg-2"
              style={{ fontSize: "10.5px" }}
            >
              <option value="">Aggregate (all changes)</option>
              {codeMutatingNodes.map((nd) => (
                <option key={nd.id} value={nd.id}>
                  {nd.name ?? nd.id}
                </option>
              ))}
            </select>
          )}

          {loading ? (
            <div className="text-fg-4" style={{ fontSize: "11px" }}>
              Loading…
            </div>
          ) : diff ? (
            <pre
              className="overflow-auto rounded bg-bg-1 p-2 font-mono text-fg-3 select-text"
              style={{ fontSize: "10.5px", lineHeight: "1.5", maxHeight: "400px" }}
            >
              {renderDiff(diff)}
            </pre>
          ) : (
            <div
              className="text-fg-4"
              style={{ fontSize: "11px" }}
            >
              No changes
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function renderDiff(diff: string): React.ReactNode {
  return diff.split("\n").map((line, i) => {
    let className = "text-fg-3";
    if (line.startsWith("+") && !line.startsWith("+++")) {
      className = "text-st-done";
    } else if (line.startsWith("-") && !line.startsWith("---")) {
      className = "text-st-failed";
    } else if (line.startsWith("@@")) {
      className = "text-acc";
    } else if (line.startsWith("diff ") || line.startsWith("index ")) {
      className = "text-fg-4";
    }
    return (
      <span key={i} className={className}>
        {line}
        {"\n"}
      </span>
    );
  });
}
