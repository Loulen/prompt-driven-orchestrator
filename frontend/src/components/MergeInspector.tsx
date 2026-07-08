import { useEditStore } from "../stores/editStore";
import { SectionHead, Field } from "./InspectorPrimitives";
import ModelPicker from "./ModelPicker";

export default function MergeInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateNode = useEditStore((s) => s.updateNode);

  const tab = openTabs.find((t) => t.id === activeTabId);
  if (!tab || selection.kind !== "node" || !selection.id) return null;

  const node = tab.pipeline.nodes.find((n) => n.id === selection.id);
  if (!node || node.type !== "merge") return null;

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Merge Inspector
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        <SectionHead title="Identity" />
        <Field label="ID">
          <span className="font-mono text-fg-3">{node.id}</span>
        </Field>
        <Field label="Name">
          <input
            value={node.name ?? ""}
            onChange={(e) => updateNode(node.id, { name: e.target.value || null })}
            className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
          />
        </Field>
        {/* Model (#296/#324): a merge node spawns an agent, so its model is
            settable here too. Dropdown + Custom… escape hatch (see ModelPicker). */}
        <Field label="Model">
          <ModelPicker
            value={node.model ?? null}
            onChange={(v) => updateNode(node.id, { model: v })}
            testid="merge-model"
          />
        </Field>

        <SectionHead title="Ports" />
        <Field label="Input">
          <span className="font-mono text-fg-3">branches (repeated)</span>
        </Field>
        <Field label="Output">
          <span className="font-mono text-fg-3">merged</span>
        </Field>

        <div
          className="mt-2 rounded border border-acc/30 bg-acc/5 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          Merge nodes wait for all upstream branches to complete, then merge
          their worktrees. If conflicts arise, a resolver session is spawned.
        </div>
      </div>
    </aside>
  );
}
