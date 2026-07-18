import { useState, useEffect, useMemo } from "react";
import { ChevronRight, ChevronDown } from "lucide-react";
import { fetchRunDiff, fetchNodeDiff } from "../api";
import { parseUnifiedDiff } from "../lib/parseUnifiedDiff";
import type { RunState } from "../types";

interface Props {
  run: RunState | null;
}

export default function DiffSection({ run }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [diff, setDiff] = useState("");
  const [loading, setLoading] = useState(false);
  const [selectedNode, setSelectedNode] = useState<string>("");
  const [collapsedFiles, setCollapsedFiles] = useState<Set<number>>(new Set());

  const codeMutatingNodes = useMemo(() => {
    if (!run) return [];
    return run.node_defs.filter((nd) => {
      const ns = run.nodes[nd.id];
      return nd.node_type === "code-mutating" && ns?.status === "completed";
    });
  }, [run]);

  const files = useMemo(() => parseUnifiedDiff(diff), [diff]);

  useEffect(() => {
    if (!expanded || !run) return;
    // #376: an archived run's `pdo/run-<id>` branch is deleted at cleanup
    // (ADR-0020), so there is nothing to diff — skip the fetch entirely and
    // render an honest message instead of a lying "No changes".
    if (run.status === "archived") {
      setLoading(false);
      return;
    }
    let stale = false;
    const promise =
      selectedNode === ""
        ? fetchRunDiff(run.run_id)
        : fetchNodeDiff(run.run_id, selectedNode);
    promise
      .then((d) => {
        if (!stale) {
          setDiff(d);
          setCollapsedFiles(new Set());
        }
      })
      .catch(() => {
        if (!stale) setDiff("");
      })
      .finally(() => { if (!stale) setLoading(false); });
    return () => { stale = true; };
  }, [expanded, run, selectedNode]);

  if (!run) return null;

  const handleExpand = () => {
    if (!expanded) setLoading(true);
    setExpanded((e) => !e);
  };

  const handleNodeChange = (nodeId: string) => {
    setLoading(true);
    setSelectedNode(nodeId);
  };

  const toggleFile = (i: number) => {
    setCollapsedFiles((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };

  const isArchived = run.status === "archived";

  return (
    <div data-testid="diff-section" className="border-t border-line">
      <button
        onClick={handleExpand}
        className="flex w-full items-center gap-1.5 px-3 py-2 text-fg-2 transition-colors hover:bg-bg-3 cursor-pointer"
        style={{ fontSize: "11.5px" }}
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <span className="font-medium">Diff</span>
      </button>

      {expanded && (
        <div data-testid="diff-content" className="px-3 pb-3">
          {isArchived ? (
            <div className="text-fg-4" style={{ fontSize: "11px" }}>
              Diff not preserved for archived runs.
            </div>
          ) : (
            <>
              {codeMutatingNodes.length > 0 && (
                <select
                  data-testid="diff-node-select"
                  value={selectedNode}
                  onChange={(e) => handleNodeChange(e.target.value)}
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

              {loading && (
                <div className="text-fg-4" style={{ fontSize: "11px" }}>
                  Loading…
                </div>
              )}
              {!loading && files.length > 0 && (
                <div className="flex flex-col gap-1.5">
                  {files.map((file, i) => {
                    const isFileCollapsed = collapsedFiles.has(i);
                    return (
                      <div
                        key={i}
                        data-testid="diff-file"
                        className="overflow-hidden rounded border border-line"
                      >
                        <button
                          onClick={() => toggleFile(i)}
                          className="flex w-full items-center gap-1 bg-bg-3 px-2 py-1 text-fg-2 transition-colors hover:bg-bg-2 cursor-pointer"
                          style={{ fontSize: "10.5px" }}
                        >
                          {isFileCollapsed ? (
                            <ChevronRight size={11} />
                          ) : (
                            <ChevronDown size={11} />
                          )}
                          <span
                            className="truncate font-mono"
                            title={file.displayPath}
                          >
                            {file.displayPath || "(unknown path)"}
                          </span>
                          {file.isBinary && (
                            <span className="rounded bg-bg-1 px-1 text-fg-4">
                              binary
                            </span>
                          )}
                          {(file.status === "renamed" ||
                            file.status === "copied") && (
                            <span className="rounded bg-bg-1 px-1 text-fg-4">
                              {file.status}
                            </span>
                          )}
                          <span className="ml-auto shrink-0 font-mono tabular-nums">
                            <span className="text-st-done">
                              +{file.additions}
                            </span>{" "}
                            <span className="text-st-failed">
                              -{file.deletions}
                            </span>
                          </span>
                        </button>
                        {!isFileCollapsed && (
                          <pre
                            className="overflow-auto bg-bg-1 p-2 font-mono text-fg-3 select-text"
                            style={{
                              fontSize: "10.5px",
                              lineHeight: "1.5",
                              maxHeight: "400px",
                            }}
                          >
                            {renderDiff(file.body)}
                          </pre>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
              {!loading && files.length === 0 && (
                <div className="text-fg-4" style={{ fontSize: "11px" }}>
                  No changes
                </div>
              )}
            </>
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
