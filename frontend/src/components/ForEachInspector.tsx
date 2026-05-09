import { useEditStore } from "../stores/editStore";
import type { PortSide } from "../types";
import { SectionHead, Field } from "./InspectorPrimitives";
import SidePicker from "./SidePicker";

const FOREACH_PORTS = [
  { name: "in", dir: "input" as const },
  { name: "break", dir: "input" as const },
  { name: "body", dir: "output" as const },
  { name: "done", dir: "output" as const },
];

export default function ForEachInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateNode = useEditStore((s) => s.updateNode);

  const tab = openTabs.find((t) => t.id === activeTabId);
  const node =
    tab && selection.kind === "node" && selection.id
      ? tab.pipeline.nodes.find((n) => n.id === selection.id) ?? null
      : null;

  if (!tab || !node || node.type !== "for-each") return null;

  function handlePortSideChange(portName: string, dir: "input" | "output", side: PortSide) {
    const isInput = dir === "input";
    const ports = isInput ? node!.inputs : node!.outputs;
    const updated = ports.map((p) =>
      p.name === portName ? { ...p, side } : p,
    );
    updateNode(node!.id, isInput ? { inputs: updated } : { outputs: updated });
  }

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        ForEach Inspector
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        {/* Identity */}
        <SectionHead title="Identity" />
        <Field label="ID">
          <span
            className="block w-full cursor-pointer select-all rounded border border-line bg-bg-3 px-2 py-1 font-mono text-fg-3"
            style={{ fontSize: "10px" }}
            title="Click to copy"
            onClick={() => navigator.clipboard.writeText(node.id)}
          >
            {node.id}
          </span>
        </Field>
        <Field label="Display name">
          <input
            key={node.id}
            defaultValue={node.name ?? ""}
            placeholder="ForEach"
            onBlur={(e) => updateNode(node.id, { name: e.target.value || null })}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                const val = (e.target as HTMLInputElement).value;
                updateNode(node.id, { name: val || null });
                (e.target as HTMLInputElement).blur();
              }
            }}
            className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
          />
        </Field>

        {/* Ports */}
        <SectionHead title="Ports" count={4} />
        <div className="flex flex-col gap-2">
          {FOREACH_PORTS.map((portDef) => {
            const portList = portDef.dir === "input" ? node.inputs : node.outputs;
            const port = portList.find((p) => p.name === portDef.name);
            const side = port?.side ?? (portDef.dir === "input" ? "left" : "right");

            return (
              <div
                key={portDef.name}
                data-testid={`port-row-${portDef.name}`}
                className="flex items-center gap-2 rounded border border-line-soft bg-bg-3 px-2 py-1.5"
              >
                <span
                  className={`h-1.5 w-1.5 rounded-full ${
                    portDef.dir === "output" ? "bg-acc" : "bg-fg-4"
                  }`}
                />
                <span className="font-mono text-fg-2" style={{ fontSize: "11.5px" }}>
                  {portDef.name}
                </span>
                <span
                  className="rounded bg-bg-4 px-1 text-fg-4"
                  style={{ fontSize: "9.5px" }}
                >
                  {portDef.dir}
                </span>
                <span className="flex-1" />
                <SidePicker
                  value={side}
                  onChange={(s) => handlePortSideChange(portDef.name, portDef.dir, s)}
                />
              </div>
            );
          })}
        </div>
        <p className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.5 }}>
          Port names are fixed: <code className="text-fg-3">in</code>,{" "}
          <code className="text-fg-3">break</code> (inputs);{" "}
          <code className="text-fg-3">body</code>,{" "}
          <code className="text-fg-3">done</code> (outputs). The upstream
          artifact must include an <code className="text-fg-3">items</code>{" "}
          frontmatter field (YAML sequence). Each body iteration receives one
          item.
        </p>
      </div>
    </aside>
  );
}
