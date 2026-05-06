import { useEditStore } from "../stores/editStore";
import type { VariableDef } from "../types";
import { SectionHead, Field } from "./InspectorPrimitives";

const VAR_TYPES = ["int", "float", "string", "bool", "list"] as const;

export default function PipelineInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateMeta = useEditStore((s) => s.updatePipelineMeta);

  const tab = openTabs.find((t) => t.id === activeTabId);
  if (!tab || selection.kind !== "none") return null;

  const pipeline = tab.pipeline;
  const variables = Object.entries(pipeline.variables);

  function handleAddVariable() {
    let name = "new_var";
    let counter = 1;
    while (pipeline.variables[name]) {
      name = `new_var_${++counter}`;
    }
    updateMeta({
      variables: {
        ...pipeline.variables,
        [name]: { type: "int", default: 0 },
      },
    });
  }

  function handleUpdateVariable(oldName: string, newName: string, updates: Partial<VariableDef>) {
    const newVars = { ...pipeline.variables };
    if (oldName !== newName) {
      delete newVars[oldName];
    }
    newVars[newName] = { ...(pipeline.variables[oldName] ?? { type: "int", default: 0 }), ...updates };
    updateMeta({ variables: newVars });
  }

  function handleDeleteVariable(name: string) {
    const newVars = { ...pipeline.variables };
    delete newVars[name];
    updateMeta({ variables: newVars });
  }

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Pipeline Inspector
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        {/* Identity */}
        <SectionHead title="Identity" />
        <Field label="Name">
          <input
            value={pipeline.name}
            onChange={(e) => updateMeta({ name: e.target.value })}
            className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
          />
        </Field>
        <Field label="Version">
          <input
            value={pipeline.version ?? ""}
            onChange={(e) => updateMeta({ version: e.target.value || null })}
            className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
            placeholder="1.0"
          />
        </Field>

        {/* Variables */}
        <SectionHead title="Variables" count={variables.length} onAdd={handleAddVariable} />

        {variables.map(([name, def]) => (
          <VariableRow
            key={name}
            name={name}
            def={def}
            onUpdate={(newName, updates) => handleUpdateVariable(name, newName, updates)}
            onDelete={() => handleDeleteVariable(name)}
          />
        ))}

        {/* Stats */}
        <SectionHead title="Stats" />
        <div className="flex gap-4 text-fg-4" style={{ fontSize: "10px" }}>
          <span>{pipeline.nodes.length} nodes</span>
          <span>{pipeline.edges.length} edges</span>
        </div>
      </div>
    </aside>
  );
}

function VariableRow({
  name,
  def,
  onUpdate,
  onDelete,
}: {
  name: string;
  def: VariableDef;
  onUpdate: (newName: string, updates: Partial<VariableDef>) => void;
  onDelete: () => void;
}) {
  const defaultStr = Array.isArray(def.default)
    ? `[${(def.default as unknown[]).join(", ")}]`
    : String(def.default ?? "");

  function handleDefaultChange(val: string) {
    let parsed: unknown = val;
    if (def.type === "int") parsed = parseInt(val, 10) || 0;
    else if (def.type === "float") parsed = parseFloat(val) || 0;
    else if (def.type === "bool") parsed = val === "true";
    else if (def.type === "list") {
      if (val.startsWith("[") && val.endsWith("]")) {
        parsed = val.slice(1, -1).split(",").map((s) => s.trim()).filter(Boolean);
      } else {
        parsed = val.split(",").map((s) => s.trim()).filter(Boolean);
      }
    }
    onUpdate(name, { default: parsed });
  }

  return (
    <div className="flex items-center gap-1 rounded border border-line-soft bg-bg-3 px-2 py-1">
      <input
        value={name}
        onChange={(e) => onUpdate(e.target.value, {})}
        className="w-20 min-w-0 bg-transparent text-fg outline-none"
        style={{ fontSize: "11px" }}
      />
      <select
        value={def.type}
        onChange={(e) => onUpdate(name, { type: e.target.value })}
        className="rounded border border-line-strong bg-bg-4 px-1 py-0.5 text-fg-3 outline-none"
        style={{ fontSize: "10px" }}
      >
        {VAR_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
      </select>
      <input
        value={defaultStr}
        onChange={(e) => handleDefaultChange(e.target.value)}
        className="min-w-0 flex-1 bg-transparent text-fg outline-none"
        style={{ fontSize: "11px" }}
        placeholder="default"
      />
      <button
        onClick={onDelete}
        className="text-fg-4 hover:text-st-failed"
      >
        ×
      </button>
    </div>
  );
}
