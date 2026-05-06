import { useEditStore } from "../stores/editStore";
import type { NodeDef, NodeType, PortDef } from "../types";
import { SectionHead, Field } from "./InspectorPrimitives";

export default function NodeInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateNode = useEditStore((s) => s.updateNode);
  const updatePrompt = useEditStore((s) => s.updatePrompt);

  const tab = openTabs.find((t) => t.id === activeTabId);
  if (!tab || selection.kind !== "node" || !selection.id) return null;

  const node = tab.pipeline.nodes.find((n) => n.id === selection.id);
  if (!node) return null;

  const promptContent = tab.prompts[node.id] ?? "";

  function handleField(field: keyof NodeDef, value: unknown) {
    updateNode(node!.id, { [field]: value } as Partial<NodeDef>);
  }

  function handleAddPort(side: "inputs" | "outputs") {
    const ports = [...node![side]];
    let name = side === "inputs" ? "in" : "out";
    let counter = 1;
    while (ports.some((p) => p.name === name)) {
      name = `${side === "inputs" ? "in" : "out"}-${++counter}`;
    }
    ports.push({ name, repeated: false });
    updateNode(node!.id, { [side]: ports });
  }

  function handleUpdatePort(side: "inputs" | "outputs", index: number, updates: Partial<PortDef>) {
    const ports = node![side].map((p, i) => (i === index ? { ...p, ...updates } : p));
    updateNode(node!.id, { [side]: ports });
  }

  function handleRemovePort(side: "inputs" | "outputs", index: number) {
    const ports = node![side].filter((_, i) => i !== index);
    updateNode(node!.id, { [side]: ports });
  }

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Node Inspector
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        {/* Identity */}
        <SectionHead title="Identity" />
        <Field label="ID">
          <input
            value={node.id}
            onChange={(e) => handleField("id", e.target.value)}
            className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
          />
        </Field>

        {/* Type */}
        <SectionHead title="Type" />
        <div className="flex gap-1">
          {(["code-mutating", "doc-only"] as NodeType[]).map((t) => (
            <button
              key={t}
              onClick={() => handleField("type", t)}
              className={`flex-1 rounded border px-2 py-1 font-medium transition-colors ${
                node.type === t
                  ? t === "code-mutating"
                    ? "border-acc bg-acc-bg text-acc"
                    : "border-fg-4 bg-bg-3 text-fg"
                  : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
              }`}
              style={{ fontSize: "10px" }}
            >
              {t}
            </button>
          ))}
        </div>

        {/* Behavior */}
        <SectionHead title="Behavior" />
        <div className="flex items-center justify-between">
          <span className="text-fg-3">Interactive</span>
          <button
            onClick={() => handleField("interactive", !node.interactive)}
            className={`relative h-5 w-9 rounded-full transition-colors ${
              node.interactive ? "bg-acc" : "bg-bg-5"
            }`}
          >
            <span
              className={`absolute top-0.5 h-4 w-4 rounded-full bg-fg transition-transform ${
                node.interactive ? "translate-x-4" : "translate-x-0.5"
              }`}
            />
          </button>
        </div>

        {/* Prompt */}
        <SectionHead title="Prompt" />
        <div className="text-fg-4" style={{ fontSize: "10px" }}>
          {node.prompt_file ?? "no prompt file"}
        </div>
        <textarea
          value={promptContent}
          onChange={(e) => updatePrompt(node.id, e.target.value)}
          className="min-h-[120px] w-full resize-y rounded border border-line-strong bg-bg-3 px-2 py-1.5 font-mono text-fg outline-none focus:border-acc"
          style={{ fontSize: "11px", lineHeight: "1.5" }}
          placeholder="Enter the node's role prompt..."
        />

        {/* Inputs */}
        <SectionHead title="Inputs" count={node.inputs.length} onAdd={() => handleAddPort("inputs")} />
        {node.inputs.map((port, i) => (
          <PortRow
            key={i}
            port={port}
            onUpdate={(updates) => handleUpdatePort("inputs", i, updates)}
            onRemove={() => handleRemovePort("inputs", i)}
          />
        ))}

        {/* Outputs */}
        <SectionHead title="Outputs" count={node.outputs.length} onAdd={() => handleAddPort("outputs")} />
        {node.outputs.map((port, i) => (
          <PortRow
            key={i}
            port={port}
            onUpdate={(updates) => handleUpdatePort("outputs", i, updates)}
            onRemove={() => handleRemovePort("outputs", i)}
          />
        ))}
      </div>
    </aside>
  );
}

function PortRow({
  port,
  onUpdate,
  onRemove,
}: {
  port: PortDef;
  onUpdate: (updates: Partial<PortDef>) => void;
  onRemove: () => void;
}) {
  return (
    <div className="flex items-center gap-1.5 rounded border border-line-soft bg-bg-3 px-2 py-1">
      <span className="h-2 w-2 shrink-0 rounded-full bg-fg-4" />
      <input
        value={port.name}
        onChange={(e) => onUpdate({ name: e.target.value })}
        className="min-w-0 flex-1 bg-transparent text-fg outline-none"
        style={{ fontSize: "11px" }}
      />
      <button
        onClick={() => onUpdate({ repeated: !port.repeated })}
        className={`rounded px-1 py-px transition-colors ${
          port.repeated
            ? "bg-st-await-bg text-st-await"
            : "text-fg-4 hover:text-fg-3"
        }`}
        style={{ fontSize: "9px" }}
        title="Toggle repeated"
      >
        repeated
      </button>
      <button
        onClick={onRemove}
        className="text-fg-4 hover:text-st-failed"
        style={{ fontSize: "10px" }}
      >
        ×
      </button>
    </div>
  );
}
