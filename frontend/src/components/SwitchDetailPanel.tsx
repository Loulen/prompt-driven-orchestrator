import { useEffect, useState } from "react";
import { GitBranch } from "lucide-react";
import type { NodeState, SwitchStateInfo } from "../types";
import { fetchNodeIO } from "../api";
import type { PortIO, FileInfo } from "../api";
import MarkdownArtifactModal from "./MarkdownArtifactModal";

interface Props {
  node: NodeState;
  runId: string;
  switchState: SwitchStateInfo | null;
  nodeName?: string | null;
}

export default function SwitchDetailPanel({
  node,
  runId,
  switchState,
  nodeName,
}: Props) {
  const [inputs, setInputs] = useState<PortIO[]>([]);
  const [outputs, setOutputs] = useState<PortIO[]>([]);
  const [modal, setModal] = useState<{
    portName: string;
    files: FileInfo[];
    portKind: "input" | "output";
  } | null>(null);

  useEffect(() => {
    if (node.status === "pending") return;
    let cancelled = false;
    fetchNodeIO(runId, node.node_id, node.iter)
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
  }, [runId, node.node_id, node.iter, node.status]);

  return (
    <aside className="flex h-full flex-col bg-bg-2" data-testid="switch-detail-panel">
      {/* Header */}
      <div className="border-b border-line px-3 py-2">
        <div className="flex items-center gap-2">
          <GitBranch size={14} className="text-[var(--color-switch-tint,#a78bfa)]" />
          <span className="font-medium text-fg" style={{ fontSize: "12.5px" }}>
            {nodeName ?? node.node_id}
          </span>
          <span
            className="rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3"
            style={{ fontSize: "10px", fontWeight: 500 }}
          >
            Switch
          </span>
        </div>
        <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "9px" }}>
          {node.node_id}
        </div>
      </div>

      {/* Chosen branch */}
      {switchState && (
        <div className="border-b border-line px-3 py-2" data-testid="switch-chosen-branch">
          <div className="text-fg-3" style={{ fontSize: "11px" }}>
            Routed branch
          </div>
          <div className="mt-1 flex items-center gap-2">
            <span
              className="rounded bg-acc-bg px-2 py-0.5 font-medium text-acc ring-1 ring-acc/40"
              style={{ fontSize: "12px" }}
            >
              {switchState.chosen_branch}
            </span>
          </div>
          <div className="mt-1 font-mono text-fg-4" style={{ fontSize: "9px" }}>
            evaluated {formatTime(switchState.evaluated_at)}
          </div>
        </div>
      )}

      {/* Pending state */}
      {!switchState && node.status === "pending" && (
        <div className="border-b border-line px-3 py-2 text-fg-4" style={{ fontSize: "11px" }}>
          Waiting for upstream nodes to complete before evaluating.
        </div>
      )}

      {/* I/O sections */}
      <div className="flex-1 overflow-auto">
        {inputs.length > 0 && (
          <div className="border-t border-line">
            <div
              className="flex items-center gap-1.5 px-3 py-1.5 text-fg-3"
              style={{ fontSize: "11px" }}
            >
              Input artifact
              <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
                {inputs.length}
              </span>
            </div>
            <div className="flex flex-col gap-1 px-3 pb-2">
              {inputs.map((port) => (
                <PortRow
                  key={port.port}
                  port={port}
                  onOpen={() => setModal({ portName: port.port, files: port.files, portKind: "input" })}
                />
              ))}
            </div>
          </div>
        )}

        {outputs.length > 0 && (
          <div className="border-t border-line">
            <div
              className="flex items-center gap-1.5 px-3 py-1.5 text-fg-3"
              style={{ fontSize: "11px" }}
            >
              Output (passthrough)
              <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
                {outputs.length}
              </span>
            </div>
            <div className="flex flex-col gap-1 px-3 pb-2">
              {outputs.map((port) => (
                <PortRow
                  key={port.port}
                  port={port}
                  onOpen={() => setModal({ portName: port.port, files: port.files, portKind: "output" })}
                />
              ))}
            </div>
          </div>
        )}

        <div className="border-t border-line px-3 py-2 text-fg-4" style={{ fontSize: "10px" }}>
          Switch nodes evaluate in-process — no agent session.
        </div>
      </div>

      {modal && (
        <MarkdownArtifactModal
          runId={runId}
          portName={modal.portName}
          source={{ kind: "static", files: modal.files }}
          onClose={() => setModal(null)}
        />
      )}
    </aside>
  );
}

function PortRow({ port, onOpen }: { port: PortIO; onOpen: () => void }) {
  const firstFile = port.files[0];
  const anyExists = port.files.some((f) => f.exists);

  return (
    <button
      onClick={anyExists ? onOpen : undefined}
      className={`flex items-center gap-1.5 rounded px-1.5 py-1 text-left transition-colors ${
        anyExists
          ? "cursor-pointer bg-bg-3 hover:bg-bg-4"
          : "cursor-default bg-bg-3 opacity-60"
      }`}
      style={{ fontSize: "10.5px" }}
      disabled={!anyExists}
    >
      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${anyExists ? "bg-st-done" : "bg-fg-5"}`} />
      <span className="font-medium text-fg-2">{port.port}</span>
      {firstFile && (
        <span className="ml-auto truncate font-mono text-fg-4" style={{ fontSize: "9px", maxWidth: 200 }}>
          {firstFile.path.split("/").slice(-2).join("/")}
        </span>
      )}
    </button>
  );
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
