import { useEffect, useState } from "react";
import type { StartNodeInfo } from "../types";
import { fetchArtifact } from "../api";
import type { FileInfo } from "../api";
import MarkdownArtifactModal from "./MarkdownArtifactModal";

interface Props {
  startNode: StartNodeInfo;
  runId: string;
  nodeId: string;
}

export default function StartInspector({ startNode, runId, nodeId }: Props) {
  const [inputText, setInputText] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;

    fetchArtifact(runId, startNode.input_path)
      .then((text) => {
        if (!cancelled) setInputText(text);
      })
      .catch(() => {
        if (!cancelled) setInputText(null);
      });

    return () => {
      cancelled = true;
    };
  }, [runId, startNode.input_path]);

  const modalFiles: FileInfo[] = [
    {
      path: startNode.input_path,
      exists: inputText != null,
      size: inputText?.length ?? null,
      frontmatter: null,
    },
  ];

  return (
    <aside className="start-inspector flex h-full flex-col bg-bg-2">
      <div className="border-b border-line px-3 py-2">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg" style={{ fontSize: "12.5px" }}>
            Run start
          </span>
          <span
            className="runtime-badge rounded border border-acc/40 bg-acc/10 px-1.5 py-0.5 text-acc"
            style={{ fontSize: "10px", fontWeight: 500 }}
          >
            runtime
          </span>
        </div>
        <div
          className="mt-0.5 font-mono text-fg-4"
          style={{ fontSize: "10px" }}
        >
          {nodeId}
        </div>
      </div>

      <div className="flex-1 overflow-auto p-3">
        <div
          className="mb-2 text-fg-3"
          style={{ fontSize: "11px", fontWeight: 500 }}
        >
          Input
        </div>
        <pre
          className="start-input-text overflow-auto rounded border border-line bg-bg-0 p-2 font-mono text-fg-2"
          style={{ fontSize: "10.5px", lineHeight: "1.5", whiteSpace: "pre-wrap" }}
        >
          {inputText ?? (
            <span className="text-fg-4">Loading input...</span>
          )}
        </pre>
      </div>

      <div className="border-t border-line px-3 py-2">
        <button
          onClick={() => setModalOpen(true)}
          className="view-markdown-link text-fg-3 transition-colors hover:text-acc"
          style={{ fontSize: "11px" }}
        >
          View as markdown &#x2197;
        </button>
      </div>

      {modalOpen && (
        <MarkdownArtifactModal
          runId={runId}
          portName="_input"
          source={{ kind: "static", files: modalFiles }}
          onClose={() => setModalOpen(false)}
        />
      )}
    </aside>
  );
}
