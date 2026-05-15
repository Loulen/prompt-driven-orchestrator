import { useCallback, useEffect, useState } from "react";
import {
  CheckCircle,
  AlertCircle,
  ChevronDown,
  ChevronRight,
  Square,
  RotateCcw,
  Play,
} from "lucide-react";
import type { IterationInfo, NodeState, NodeStatus } from "../types";
import {
  markNodeDone,
  killNode,
  restartNode,
  stopNode,
  retryNode,
  retryNodePreview,
  fetchPrompt,
  fetchNodeIO,
  artifactUrl,
} from "../api";
import type { PortIO, FileInfo, MarkNodeDoneResult } from "../api";
import type { PortType } from "../types";
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from "./ui/resizable";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "./ui/dropdown-menu";
import MarkdownArtifactModal from "./MarkdownArtifactModal";
import TmuxTerminal from "./TmuxTerminal";

const STATUS_LABELS: Record<NodeStatus, string> = {
  pending: "Pending",
  running: "Running",
  awaiting_user: "Awaiting User",
  completed: "Completed",
  failed: "Failed",
  stopped: "Stopped",
  stale: "Stale",
};

function pollInterval(status: NodeStatus): number | null {
  switch (status) {
    case "running":
    case "awaiting_user":
    case "stale":
      return 1000;
    case "completed":
    case "failed":
    case "stopped":
      return 5000;
    case "pending":
      return null;
  }
}

interface Props {
  node: NodeState;
  runId: string;
  isArchived?: boolean;
  nodeName?: string | null;
  initialTerminalExpanded?: boolean;
}

interface ModalState {
  portName: string;
  files: FileInfo[];
  portKind: "input" | "output";
  portType: PortType;
}

export default function NodeDetailPanel({
  node,
  runId,
  isArchived,
  nodeName,
  initialTerminalExpanded,
}: Props) {
  const [promptText, setPromptText] = useState<string | null>(null);
  const [inputs, setInputs] = useState<PortIO[]>([]);
  const [outputs, setOutputs] = useState<PortIO[]>([]);
  const [modal, setModal] = useState<ModalState | null>(null);
  const [missingOutputs, setMissingOutputs] = useState<string[] | null>(null);
  const [terminalExpanded, setTerminalExpanded] = useState(
    initialTerminalExpanded ?? false,
  );
  const [userSelectedIter, setUserSelectedIter] = useState<{
    nodeId: string;
    iter: number;
  } | null>(null);
  const [retryConfirm, setRetryConfirm] = useState<{
    affectedCount: number;
  } | null>(null);

  const selectedIter =
    userSelectedIter?.nodeId === node.node_id
      ? userSelectedIter.iter
      : node.iter;

  const setSelectedIter = useCallback(
    (iter: number) => {
      setUserSelectedIter({ nodeId: node.node_id, iter });
    },
    [node.node_id],
  );

  const sessionName = `maestro-${runId}-${node.node_id}-iter-${selectedIter}`;
  const interval = pollInterval(node.status);
  const isStaleIter = selectedIter !== node.iter;
  const hasMultipleIters = (node.iterations?.length ?? 0) > 1;
  const showTerminal = node.status !== "pending";

  const shouldFetchPrompt = node.status !== "pending" || isStaleIter;

  useEffect(() => {
    if (!shouldFetchPrompt) return;

    let cancelled = false;

    fetchPrompt(runId, node.node_id, selectedIter)
      .then((text) => {
        if (!cancelled) setPromptText(text);
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [runId, node.node_id, selectedIter, shouldFetchPrompt]);

  useEffect(() => {
    const oneShot = isStaleIter || (interval === null && node.status === "pending");

    if (oneShot) {
      let cancelled = false;
      fetchNodeIO(runId, node.node_id, selectedIter)
        .then((io) => {
          if (!cancelled) {
            setInputs(io.inputs);
            setOutputs(io.outputs);
          }
        })
        .catch(() => {});
      return () => {
        cancelled = true;
      };
    }

    if (interval === null) return;

    let cancelled = false;

    async function pollIO() {
      try {
        const io = await fetchNodeIO(runId, node.node_id, selectedIter);
        if (!cancelled) {
          setInputs(io.inputs);
          setOutputs(io.outputs);
        }
      } catch {
        // ignore
      }
    }

    pollIO();
    const timer = setInterval(pollIO, interval);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [interval, node.node_id, selectedIter, runId, isStaleIter, node.status]);

  const handleStop = useCallback(async () => {
    try {
      await stopNode(runId, node.node_id);
    } catch {
      // best-effort
    }
  }, [runId, node.node_id]);

  const handleRetry = useCallback(async () => {
    try {
      const preview = await retryNodePreview(runId, node.node_id);
      if (preview.affected_count > 0) {
        setRetryConfirm({ affectedCount: preview.affected_count });
        return;
      }
      await retryNode(runId, node.node_id);
    } catch {
      // best-effort
    }
  }, [runId, node.node_id]);

  const handleRetryConfirmed = useCallback(async () => {
    setRetryConfirm(null);
    try {
      await retryNode(runId, node.node_id);
    } catch {
      // best-effort
    }
  }, [runId, node.node_id]);

  const handleMarkComplete = useCallback(async () => {
    setMissingOutputs(null);
    try {
      const result: MarkNodeDoneResult = await markNodeDone(runId, node.node_id, selectedIter);
      if (!result.ok && result.missingOutputs) {
        setMissingOutputs(result.missingOutputs.missing);
      }
    } catch (e) {
      console.error("Failed to mark node done:", e);
    }
  }, [runId, node.node_id, selectedIter]);

  return (
    <aside className="flex h-full flex-col bg-bg-2">
      {/* Header */}
      <div className="border-b border-line px-3 py-2">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg" style={{ fontSize: "12.5px" }}>
            {nodeName ?? node.node_id}
          </span>
          <span
            className="rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3"
            style={{ fontSize: "10px", fontWeight: 500 }}
          >
            {STATUS_LABELS[node.status] ?? node.status}
          </span>
        </div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
          {node.node_id}
        </div>
        <div
          className="mt-0.5 flex items-center gap-1 font-mono text-fg-4"
          style={{ fontSize: "10px" }}
        >
          {hasMultipleIters ? (
            <IterSelector
              iterations={node.iterations}
              selectedIter={selectedIter}
              onSelect={setSelectedIter}
            />
          ) : (
            <span>iter {node.iter}</span>
          )}
          {node.started_at && (
            <span> · started {formatTime(node.started_at)}</span>
          )}
          {node.completed_at && (
            <span> · ended {formatTime(node.completed_at)}</span>
          )}
        </div>
      </div>

      {!isArchived && node.status !== "pending" && (
        <div
          className="flex items-center gap-1.5 border-b border-line px-3 py-1.5"
          data-testid="node-controls"
        >
          <button
            data-testid="stop-btn"
            disabled={node.status !== "running"}
            onClick={node.status === "running" ? handleStop : undefined}
            className={
              node.status === "running"
                ? "flex cursor-pointer items-center gap-1 rounded border border-st-failed/40 bg-st-failed/10 px-2 py-0.5 text-st-failed transition-colors hover:bg-st-failed/20"
                : "flex items-center gap-1 rounded border border-line bg-bg-3 px-2 py-0.5 text-fg-4 opacity-50"
            }
            style={{ fontSize: "10.5px", fontWeight: 500 }}
          >
            <Square size={10} />
            Stop
          </button>
          <RetryPlayButton status={node.status} onClick={handleRetry} />
        </div>
      )}

      {/* Awaiting user banner */}
      {node.status === "awaiting_user" && (
        <div className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2">
          <AlertCircle size={14} className="shrink-0 text-st-await" />
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Awaiting user — interact in the terminal below, then mark complete
          </span>
        </div>
      )}

      {/* Stale banner */}
      {node.status === "stale" && (
        <div
          className="flex items-center gap-2 border-b border-st-stale/30 bg-st-stale-bg px-3 py-2"
          data-testid="stale-banner"
        >
          <AlertCircle size={14} className="shrink-0 text-st-stale" />
          <span
            className="flex-1 text-st-stale"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Agent idle for &gt;2 min — outputs incomplete
          </span>
          {!isArchived && (
            <div className="flex items-center gap-1">
              <button
                data-testid="stale-stop-btn"
                onClick={async () => {
                  try { await killNode(runId, node.node_id, selectedIter); } catch { /* best-effort */ }
                }}
                className="flex cursor-pointer items-center gap-1 rounded border border-st-stale/40 bg-st-stale/10 px-1.5 py-0.5 text-st-stale transition-colors hover:bg-st-stale/20"
                style={{ fontSize: "10.5px", fontWeight: 500 }}
              >
                <Square size={10} />
                Stop
              </button>
              <button
                data-testid="stale-retry-btn"
                onClick={async () => {
                  try { await restartNode(runId, node.node_id, selectedIter); } catch { /* best-effort */ }
                }}
                className="flex cursor-pointer items-center gap-1 rounded border border-st-stale/40 bg-st-stale/10 px-1.5 py-0.5 text-st-stale transition-colors hover:bg-st-stale/20"
                style={{ fontSize: "10.5px", fontWeight: 500 }}
              >
                <RotateCcw size={10} />
                Retry
              </button>
            </div>
          )}
        </div>
      )}

      {/* Stopped banner */}
      {node.status === "stopped" && (
        <div className="flex items-center gap-2 border-b border-st-stopped/30 bg-st-stopped-bg px-3 py-2">
          <AlertCircle size={14} className="shrink-0 text-st-stopped" />
          <span
            className="text-st-stopped"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Stopped{node.failure_reason ? ` — ${node.failure_reason}` : ""}
          </span>
        </div>
      )}

      {/* Frontmatter retry pending banner (amber) */}
      {node.status === "running" && (node.frontmatter_retries ?? 0) > 0 && (
        <div
          className="flex items-center gap-2 border-b border-st-await/30 bg-st-await-bg px-3 py-2"
          data-testid="frontmatter-retry-banner"
        >
          <AlertCircle size={14} className="shrink-0 text-st-await" />
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Frontmatter mismatch — corrective message sent, awaiting retry
          </span>
        </div>
      )}

      {/* Failed banner — validation exhausted variant */}
      {node.status === "failed" && node.failure_reason === "output validation failed" && (
        <div
          className="flex flex-col gap-1 border-b border-st-failed/30 bg-st-failed-bg px-3 py-2"
          data-testid="frontmatter-exhausted-banner"
        >
          <div className="flex items-center gap-2">
            <AlertCircle size={14} className="shrink-0 text-st-failed" />
            <span
              className="text-st-failed"
              style={{ fontSize: "11.5px", fontWeight: 500 }}
            >
              Failed — output validation failed after retry
            </span>
          </div>
          {node.frontmatter_violations && node.frontmatter_violations.length > 0 && (
            <ul
              className="mt-0.5 flex flex-col gap-0.5 pl-5 font-mono text-st-failed"
              style={{ fontSize: "10px" }}
              data-testid="frontmatter-violation-list"
            >
              {node.frontmatter_violations.map((v, i) => (
                <li key={i}>
                  {v.port}.{v.field}: {v.reason}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {/* Failed banner — generic */}
      {node.status === "failed" && node.failure_reason !== "output validation failed" && (
        <div className="flex items-center gap-2 border-b border-st-failed/30 bg-st-failed-bg px-3 py-2">
          <AlertCircle size={14} className="shrink-0 text-st-failed" />
          <span
            className="text-st-failed"
            style={{ fontSize: "11.5px", fontWeight: 500 }}
          >
            Failed{node.failure_reason ? ` — ${node.failure_reason}` : ""}
          </span>
        </div>
      )}

      {(() => {
        const terminalPane = (
          <div
            className="flex h-full flex-col overflow-hidden"
            data-testid="terminal-pane-wrapper"
          >
            {showTerminal ? (
              <TmuxTerminal
                session={sessionName}
                expanded={terminalExpanded}
                onExpand={() => setTerminalExpanded((v) => !v)}
                status={node.status}
              />
            ) : (
              <div className="flex h-full flex-col" data-testid="pending-placeholder">
                <div
                  className="flex items-center gap-1.5 border-b border-line px-3 py-1.5 text-fg-3"
                  style={{ fontSize: "11px" }}
                >
                  <span className="h-1.5 w-1.5 rounded-full bg-fg-5" />
                  Terminal
                </div>
                <div className="flex flex-1 items-center justify-center bg-bg-0">
                  <span className="text-fg-4" style={{ fontSize: "11px" }}>
                    {terminalPlaceholder(node)}
                  </span>
                </div>
              </div>
            )}
          </div>
        );

        const detailsPane = (
          <div
            className="flex h-full flex-col overflow-auto"
            data-testid="details-pane"
          >
            {/* Actions */}
            <div className="flex flex-col gap-1.5 px-3 py-2">
              {(node.status === "awaiting_user" || node.status === "running" || node.status === "failed" || node.status === "stale") && !isArchived && (
                <>
                  <button
                    onClick={handleMarkComplete}
                    className="flex w-full cursor-pointer items-center justify-center gap-1.5 rounded-md border border-st-done/40 bg-st-done-bg px-3 py-1.5 text-st-done transition-colors hover:border-st-done/60 hover:bg-st-done/20"
                    style={{ fontSize: "11.5px", fontWeight: 500 }}
                  >
                    <CheckCircle size={12} />
                    Mark complete
                  </button>

                  {missingOutputs && missingOutputs.length > 0 && (
                    <div
                      className="flex items-start gap-1.5 rounded-md border border-st-failed/30 bg-st-failed-bg px-2.5 py-1.5 font-mono text-st-failed"
                      style={{ fontSize: "10.5px" }}
                    >
                      <AlertCircle size={12} className="mt-px shrink-0" />
                      <span>
                        Missing outputs: {missingOutputs.join(", ")}
                      </span>
                    </div>
                  )}
                </>
              )}
            </div>

            {/* Inputs section */}
            {inputs.length > 0 && (
              <IOSection
                title="Inputs"
                ports={inputs}
                runId={runId}
                onOpenFile={(portName, files, portType) =>
                  setModal({ portName, files, portKind: "input", portType })
                }
              />
            )}

            {/* Outputs section */}
            {outputs.length > 0 && (
              <IOSection
                title="Outputs"
                ports={outputs}
                runId={runId}
                showFrontmatter
                onOpenFile={(portName, files, portType) =>
                  setModal({ portName, files, portKind: "output", portType })
                }
              />
            )}

            {/* Initial Prompt */}
            <PromptSection promptText={promptText} status={node.status} />
          </div>
        );

        // Keep `TmuxTerminal` mounted across the fullscreen toggle: render
        // the same `<ResizablePanelGroup>` parent in both modes and only
        // conditionally render the details panel + handle. React's reconciler
        // matches the terminal panel at position 0 in both renders, so the
        // WebSocket and xterm instance survive the toggle. Conditional panels
        // with stable `id` + `order` props are the documented pattern for
        // react-resizable-panels.
        return (
          <ResizablePanelGroup
            orientation="vertical"
            className="min-h-0 flex-1"
            data-testid={terminalExpanded ? "terminal-fullsize" : undefined}
          >
            <ResizablePanel
              id="terminal"
              defaultSize={terminalExpanded ? 100 : 45}
              minSize="100px"
            >
              {terminalPane}
            </ResizablePanel>
            {!terminalExpanded && (
              <>
                <ResizableHandle />
                <ResizablePanel
                  id="details"
                  defaultSize={55}
                  minSize="100px"
                >
                  {detailsPane}
                </ResizablePanel>
              </>
            )}
          </ResizablePanelGroup>
        );
      })()}

      {modal && (
        <MarkdownArtifactModal
          runId={runId}
          portName={modal.portName}
          portType={modal.portType}
          source={
            node.iterations && node.iterations.length > 1
              ? {
                  kind: "iter-nav",
                  nodeId: node.node_id,
                  portKind: modal.portKind,
                  iterations: node.iterations,
                  initialIter: selectedIter,
                }
              : { kind: "static", files: modal.files }
          }
          onClose={() => setModal(null)}
        />
      )}

      {retryConfirm && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
          data-testid="retry-confirm-backdrop"
          onClick={() => setRetryConfirm(null)}
        >
          <div
            className="w-[360px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
            style={{ fontSize: "12px" }}
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
              Retry this node?
            </h3>
            <p className="mt-2 text-fg-3" style={{ fontSize: "11.5px" }}>
              This will reset {retryConfirm.affectedCount} downstream{" "}
              {retryConfirm.affectedCount === 1 ? "node" : "nodes"} with
              artifacts. Continue?
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                data-testid="retry-confirm-cancel"
                onClick={() => setRetryConfirm(null)}
                className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
              >
                Cancel
              </button>
              <button
                data-testid="retry-confirm-ok"
                onClick={handleRetryConfirmed}
                className="rounded-md bg-accent px-3 py-1.5 text-white transition-colors hover:bg-accent/80"
                style={{ fontSize: "11.5px" }}
              >
                Retry
              </button>
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}

const RETRY_BUTTON_CLASS =
  "flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 transition-colors hover:bg-bg-4";
const RETRY_BUTTON_STYLE = { fontSize: "10.5px", fontWeight: 500 } as const;

function RetryPlayButton({
  status,
  onClick,
}: {
  status: NodeStatus;
  onClick: () => void;
}) {
  if (status === "running") {
    return (
      <button
        data-testid="retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <RotateCcw size={10} />
        Retry
      </button>
    );
  }

  if (status === "completed") {
    return (
      <button
        data-testid="play-retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <RotateCcw size={10} />
        Retry
      </button>
    );
  }

  if (status === "failed" || status === "stopped" || status === "stale") {
    return (
      <button
        data-testid="play-retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <Play size={10} />
        Play
      </button>
    );
  }

  return null;
}

function PromptSection({
  promptText,
  status,
}: {
  promptText: string | null;
  status: NodeStatus;
}) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="border-t border-line">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full cursor-pointer items-center gap-1.5 px-3 py-1.5 text-fg-3 transition-colors hover:text-fg-2"
        style={{ fontSize: "11px" }}
        data-testid="prompt-toggle"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        Initial Prompt
      </button>
      {expanded && (
        <pre
          className="prompt-block overflow-auto bg-bg-0 p-2 font-mono text-fg-3"
          style={{ fontSize: "10px", lineHeight: "1.5" }}
        >
          {promptText ?? (
            <span className="text-fg-4">
              {status === "pending"
                ? "Prompt available after node starts."
                : "Loading prompt..."}
            </span>
          )}
        </pre>
      )}
    </div>
  );
}

// --- Iter Selector ---

const STATUS_DOTS: Record<NodeStatus, string> = {
  pending: "bg-st-pending",
  running: "bg-st-running",
  awaiting_user: "bg-st-await",
  completed: "bg-st-done",
  failed: "bg-st-failed",
  stopped: "bg-st-stopped",
  stale: "bg-st-stale",
};

function IterSelector({
  iterations,
  selectedIter,
  onSelect,
}: {
  iterations: IterationInfo[];
  selectedIter: number;
  onSelect: (iter: number) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className="flex cursor-pointer items-center gap-0.5 rounded px-1 py-0.5 font-mono text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg-2"
        style={{ fontSize: "10px" }}
      >
        iter {selectedIter}
        <ChevronDown size={10} className="text-fg-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        className="min-w-[180px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
        side="bottom"
        align="start"
      >
        {iterations.map((it) => (
          <DropdownMenuItem
            key={it.iter}
            className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4 ${
              it.iter === selectedIter ? "bg-bg-4" : ""
            }`}
            style={{ fontSize: "11px" }}
            onClick={() => onSelect(it.iter)}
            data-testid={`iter-option-${it.iter}`}
          >
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${STATUS_DOTS[it.status]}`}
            />
            <span className="font-mono">iter {it.iter}</span>
            <span
              className="ml-auto font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              {it.started_at ? formatTime(it.started_at) : ""}
              {it.completed_at ? ` – ${formatTime(it.completed_at)}` : ""}
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

// --- IO Section ---

function IOSection({
  title,
  ports,
  runId,
  showFrontmatter,
  onOpenFile,
}: {
  title: string;
  ports: PortIO[];
  runId: string;
  showFrontmatter?: boolean;
  onOpenFile: (portName: string, files: FileInfo[], portType: PortType) => void;
}) {
  return (
    <div className="border-t border-line">
      <div
        className="flex items-center gap-1.5 px-3 py-1.5 text-fg-3"
        style={{ fontSize: "11px" }}
      >
        {title}
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {ports.length}
        </span>
      </div>
      <div className="flex flex-col gap-1 px-3 pb-2">
        {ports.map((port) => (
          <PortRow
            key={port.port}
            port={port}
            runId={runId}
            showFrontmatter={showFrontmatter}
            onOpen={() => onOpenFile(port.port, port.files, port.port_type ?? "markdown")}
          />
        ))}
      </div>
    </div>
  );
}

// --- Port Row ---

const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "webp", "gif"]);

function isImageFile(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTENSIONS.has(ext);
}

function PortRow({
  port,
  runId,
  showFrontmatter,
  onOpen,
}: {
  port: PortIO;
  runId: string;
  showFrontmatter?: boolean;
  onOpen: () => void;
}) {
  const firstFile = port.files[0];
  const anyExists = port.files.some((f) => f.exists);
  const portType = port.port_type ?? "markdown";
  const isImage = portType === "image" || portType === "image_list";

  let dotClass = "bg-fg-5";
  if (anyExists && port.repeated && port.files.length > 1) {
    dotClass = "bg-st-running";
  } else if (anyExists) {
    dotClass = "bg-st-done";
  }

  let displayPath = firstFile?.path ?? "";
  if (port.files.length > 1 && (port.repeated || isImage)) {
    displayPath = `${port.files.length} files`;
  }

  const totalSize = port.files.reduce((sum, f) => sum + (f.size ?? 0), 0);

  const frontmatter =
    showFrontmatter && !isImage && firstFile?.frontmatter
      ? firstFile.frontmatter
      : null;

  const imageFiles = isImage
    ? port.files.filter((f) => f.exists && isImageFile(f.path))
    : [];

  const gridStyle = {
    gridTemplateColumns: "8px 1fr auto",
    fontSize: "11.5px",
  };

  const children = (
    <>
      {/* Status dot */}
      <div className={`h-2 w-2 rounded-full ${dotClass}`} />

      {/* Name + path */}
      <div className="min-w-0 text-left">
        <div className="flex items-center gap-1.5">
          <span className="font-mono text-fg" style={{ fontSize: "11.5px" }}>
            {port.port}
          </span>
          {port.repeated && (
            <span
              className="rounded border border-line-strong bg-bg-4 px-1 py-px font-mono text-fg-4"
              style={{ fontSize: "9px" }}
            >
              repeated
            </span>
          )}
          {isImage && (
            <span
              className="rounded border border-line-strong bg-bg-4 px-1 py-px font-mono text-fg-4"
              style={{ fontSize: "9px" }}
              data-testid="port-type-badge"
            >
              {portType}
            </span>
          )}
        </div>
        <div
          className="mt-0.5 truncate font-mono text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          {displayPath}
        </div>
      </div>

      {/* Meta + arrow icon */}
      <div className="flex items-center gap-2">
        {anyExists && totalSize > 0 && (
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {formatSize(totalSize)}
          </span>
        )}
        {anyExists && (
          <span
            className="font-mono text-fg-3"
            style={{ fontSize: "10.5px" }}
            aria-hidden="true"
          >
            ↗
          </span>
        )}
      </div>

      {/* Image thumbnails */}
      {imageFiles.length > 0 && (
        <div
          className="col-span-3 mt-1 flex gap-1 overflow-x-auto"
          data-testid="image-thumbnails"
        >
          {imageFiles.slice(0, 4).map((f) => (
            <img
              key={f.path}
              src={artifactUrl(runId, f.path)}
              alt={f.path.split("/").pop() ?? ""}
              className="h-12 w-12 rounded border border-line object-cover"
            />
          ))}
          {imageFiles.length > 4 && (
            <span
              className="flex h-12 w-12 items-center justify-center rounded border border-line bg-bg-0 font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              +{imageFiles.length - 4}
            </span>
          )}
        </div>
      )}

      {/* Frontmatter card (spans full width below) */}
      {frontmatter && Object.keys(frontmatter).length > 0 && (
        <div
          className="col-span-3 mt-1 grid rounded border border-line bg-bg-0 p-1.5 font-mono"
          style={{
            fontSize: "10px",
            gridTemplateColumns: "auto 1fr",
            gap: "2px 8px",
          }}
        >
          {Object.entries(frontmatter).map(([k, v]) => (
            <FrontmatterKV key={k} field={k} value={v} />
          ))}
        </div>
      )}
    </>
  );

  if (anyExists) {
    return (
      <button
        type="button"
        onClick={onOpen}
        className="port-row grid w-full cursor-pointer items-center gap-2 rounded-md border border-line bg-bg-3 px-2.5 py-2 transition-colors hover:bg-bg-4"
        style={gridStyle}
      >
        {children}
      </button>
    );
  }

  return (
    <div
      className="port-row grid items-center gap-2 rounded-md border border-line bg-bg-3 px-2.5 py-2 opacity-60"
      style={gridStyle}
    >
      {children}
    </div>
  );
}

function FrontmatterKV({ field, value }: { field: string; value: unknown }) {
  const display =
    typeof value === "object" ? JSON.stringify(value) : String(value);
  return (
    <>
      <span className="text-fg-3">{field}</span>
      <span className="text-fg">{display}</span>
    </>
  );
}

function terminalPlaceholder(node: NodeState): string {
  switch (node.status) {
    case "pending":
      return "en attente d’activation";
    case "completed":
      return "Session ended.";
    case "failed":
      return `Failed: ${node.failure_reason ?? "unknown reason"}`;
    case "stopped":
      return `Stopped: ${node.failure_reason ?? "user stopped"}`;
    case "stale":
      return "Agent idle — outputs incomplete";
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

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
