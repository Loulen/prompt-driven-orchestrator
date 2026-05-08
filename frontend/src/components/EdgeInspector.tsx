import { useEditStore } from "../stores/editStore";
import type { EdgeEndpoint } from "../types";
import { SectionHead } from "./InspectorPrimitives";

export default function EdgeInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateEdge = useEditStore((s) => s.updateEdge);

  const tab = openTabs.find((t) => t.id === activeTabId);
  if (!tab || selection.kind !== "edge" || selection.id === null) return null;

  const edgeIndex = parseInt(selection.id, 10);
  const edge = tab.pipeline.edges[edgeIndex];
  if (!edge) return null;

  const isEndEdge = tab.pipeline.nodes.some(
    (n) => n.type === "end" && n.id === edge.target.node,
  );
  const nodeIds = tab.pipeline.nodes.map((n) => n.id);

  function handleSourceChange(updates: Partial<EdgeEndpoint>) {
    updateEdge(edgeIndex, { source: { ...edge.source, ...updates } });
  }

  function handleTargetNodeChange(node: string) {
    const targetNode = tab!.pipeline.nodes.find((n) => n.id === node);
    const port = targetNode?.inputs[0]?.name ?? "in";
    updateEdge(edgeIndex, { target: { node, port } });
  }

  function handleTargetPortChange(port: string) {
    updateEdge(edgeIndex, { target: { ...edge.target, port } });
  }

  function handleReasonChange(reason: string) {
    updateEdge(edgeIndex, { reason: reason || null });
  }

  const targetNodePorts =
    tab?.pipeline.nodes.find((n) => n.id === edge.target.node)?.inputs ?? [];

  const sourceNodePorts =
    tab?.pipeline.nodes.find((n) => n.id === edge.source.node)?.outputs ?? [];

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Edge Inspector
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        <SectionHead title="Source" />
        <div className="flex gap-2">
          <div className="flex-1">
            <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Node</label>
            <select
              value={edge.source.node}
              onChange={(e) => handleSourceChange({ node: e.target.value })}
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
            >
              {nodeIds.map((id) => <option key={id} value={id}>{id}</option>)}
            </select>
          </div>
          <div className="flex-1">
            <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Port</label>
            <select
              value={edge.source.port}
              onChange={(e) => handleSourceChange({ port: e.target.value })}
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
            >
              {sourceNodePorts.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
            </select>
          </div>
        </div>

        <SectionHead title="Target" />
        <div className="flex gap-2">
          <div className="flex-1">
            <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Node</label>
            <select
              value={edge.target.node}
              onChange={(e) => handleTargetNodeChange(e.target.value)}
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
            >
              {nodeIds.map((id) => <option key={id} value={id}>{id}</option>)}
            </select>
          </div>
          <div className="flex-1">
            <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Port</label>
            <select
              value={edge.target.port}
              onChange={(e) => handleTargetPortChange(e.target.value)}
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
            >
              {targetNodePorts.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
            </select>
          </div>
        </div>

        {isEndEdge && (
          <>
            <SectionHead title="Halt Reason" />
            <textarea
              value={edge.reason ?? ""}
              onChange={(e) => handleReasonChange(e.target.value)}
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 font-mono text-fg outline-none focus:border-acc"
              style={{ fontSize: "11px" }}
              rows={2}
              placeholder="Blocked after {iter} iterations..."
            />
          </>
        )}
      </div>
    </aside>
  );
}
