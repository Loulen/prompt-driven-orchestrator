import { useCallback, useMemo } from "react";
import { Trash2 } from "lucide-react";
import { useEditStore } from "../stores/editStore";
import type { PortDef, PortSide, PipelineDef, FrontmatterFieldDecl } from "../types";
import { SectionHead, Field } from "./InspectorPrimitives";
import SidePicker from "./SidePicker";
import { resolveUpstreamSchema } from "../lib/switchSchema";

const OPERATORS = ["eq", "neq", "lt", "lte", "gt", "gte", "in", "not_in"] as const;
type Operator = (typeof OPERATORS)[number];

interface ConditionRow {
  field: string;
  op: Operator;
  value: string;
}

function whenToRows(when: Record<string, unknown> | null | undefined): ConditionRow[] {
  if (!when) return [];
  const rows: ConditionRow[] = [];
  for (const [field, predicate] of Object.entries(when)) {
    if (typeof predicate === "object" && predicate !== null && !Array.isArray(predicate)) {
      for (const [op, val] of Object.entries(predicate as Record<string, unknown>)) {
        rows.push({
          field,
          op: op as Operator,
          value: Array.isArray(val) ? JSON.stringify(val) : String(val ?? ""),
        });
      }
    }
  }
  return rows;
}

function rowsToWhen(rows: ConditionRow[]): Record<string, unknown> | null {
  if (rows.length === 0) return null;
  const when: Record<string, Record<string, unknown>> = {};
  for (const row of rows) {
    if (!when[row.field]) when[row.field] = {};
    let parsed: unknown = row.value;
    if (row.op === "in" || row.op === "not_in") {
      try {
        const arr = JSON.parse(row.value);
        if (Array.isArray(arr)) parsed = arr;
      } catch {
        parsed = row.value.split(",").map((s) => s.trim()).filter(Boolean);
      }
    } else {
      const num = Number(row.value);
      if (!isNaN(num) && row.value.trim() !== "") parsed = num;
    }
    when[row.field][row.op] = parsed;
  }
  return when;
}

interface FieldSource {
  name: string;
  decl: FrontmatterFieldDecl | null;
}

function useAvailableFields(pipeline: PipelineDef, switchNodeId: string): FieldSource[] {
  return useMemo(() => {
    const fields: FieldSource[] = [];
    const schema = resolveUpstreamSchema(pipeline, switchNodeId);
    if (schema) {
      for (const [name, decl] of Object.entries(schema)) {
        fields.push({ name, decl });
      }
    }
    for (const varName of Object.keys(pipeline.variables)) {
      fields.push({ name: `$${varName}`, decl: null });
    }
    return fields;
  }, [pipeline, switchNodeId]);
}

export default function SwitchInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateNode = useEditStore((s) => s.updateNode);

  const tab = openTabs.find((t) => t.id === activeTabId);
  const node =
    tab && selection.kind === "node" && selection.id
      ? tab.pipeline.nodes.find((n) => n.id === selection.id) ?? null
      : null;

  const availableFields = useAvailableFields(
    tab?.pipeline ?? { name: "", variables: {}, nodes: [], edges: [] },
    node?.id ?? "",
  );

  if (!tab || !node || node.type !== "switch") return null;

  const branches = node.outputs;
  const hasSource = availableFields.length > 0;

  function updateBranches(newOutputs: PortDef[]) {
    updateNode(node!.id, { outputs: newOutputs });
  }

  function handleAddBranch() {
    const existing = branches.map((b) => b.name);
    let name = "branch";
    let counter = 1;
    while (existing.includes(name)) {
      name = `branch-${++counter}`;
    }
    const defaultIdx = branches.findIndex((b) => b.name === "default");
    const newBranch: PortDef = { name, repeated: false, side: "right" };
    const newOutputs = [...branches];
    if (defaultIdx >= 0) {
      newOutputs.splice(defaultIdx, 0, newBranch);
    } else {
      newOutputs.push(newBranch);
    }
    updateBranches(newOutputs);
  }

  function handleDeleteBranch(index: number) {
    if (branches[index].name === "default") return;
    updateBranches(branches.filter((_, i) => i !== index));
  }

  function handleUpdateBranch(index: number, updates: Partial<PortDef>) {
    const newOutputs = branches.map((b, i) => (i === index ? { ...b, ...updates } : b));
    updateBranches(newOutputs);
  }

  function handleMoveBranch(fromIndex: number, toIndex: number) {
    if (branches[fromIndex].name === "default" || branches[toIndex].name === "default") return;
    const newOutputs = [...branches];
    const [moved] = newOutputs.splice(fromIndex, 1);
    newOutputs.splice(toIndex, 0, moved);
    updateBranches(newOutputs);
  }

  function handleUpdateConditions(branchIndex: number, rows: ConditionRow[]) {
    const when = rowsToWhen(rows);
    handleUpdateBranch(branchIndex, { when });
  }

  function handleUpdateInputSide(side: PortSide) {
    const inputs = node!.inputs.map((p, i) =>
      i === 0 ? { ...p, side } : p,
    );
    updateNode(node!.id, { inputs });
  }

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Switch Inspector
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
        <Field label="Name">
          <NameInput
            key={node.id}
            value={node.name ?? ""}
            placeholder={node.id}
            onCommit={(v) => updateNode(node.id, { name: v || null })}
          />
        </Field>

        {/* Input port side */}
        <Field label="Input port side">
          <SidePicker
            value={node.inputs[0]?.side ?? "left"}
            onChange={handleUpdateInputSide}
          />
        </Field>

        {/* Branches */}
        <SectionHead title="Branches" count={branches.length} onAdd={handleAddBranch} />
        <p className="text-fg-4" style={{ fontSize: "10px" }}>
          All conditions in a branch are AND'd. For OR, use <code className="text-fg-3">in [...]</code> or add another branch with the same target.
        </p>

        {branches.map((branch, i) => (
          <BranchRow
            key={`${branch.name}-${i}`}
            branch={branch}
            isDefault={branch.name === "default"}
            availableFields={availableFields}
            hasSource={hasSource}
            onUpdateName={(name) => handleUpdateBranch(i, { name })}
            onUpdateSide={(side) => handleUpdateBranch(i, { side })}
            onDelete={() => handleDeleteBranch(i)}
            onMoveUp={i > 0 ? () => handleMoveBranch(i, i - 1) : undefined}
            onMoveDown={
              i < branches.length - 1 && branches[i + 1]?.name !== "default"
                ? () => handleMoveBranch(i, i + 1)
                : undefined
            }
            onUpdateConditions={(rows) => handleUpdateConditions(i, rows)}
          />
        ))}
      </div>
    </aside>
  );
}

function BranchRow({
  branch,
  isDefault,
  availableFields,
  hasSource,
  onUpdateName,
  onUpdateSide,
  onDelete,
  onMoveUp,
  onMoveDown,
  onUpdateConditions,
}: {
  branch: PortDef;
  isDefault: boolean;
  availableFields: FieldSource[];
  hasSource: boolean;
  onUpdateName: (name: string) => void;
  onUpdateSide: (side: PortSide) => void;
  onDelete: () => void;
  onMoveUp?: () => void;
  onMoveDown?: () => void;
  onUpdateConditions: (rows: ConditionRow[]) => void;
}) {
  const rows = whenToRows(branch.when);

  const handleAddCondition = useCallback(() => {
    if (!hasSource) return;
    const defaultField = availableFields[0]?.name ?? "";
    onUpdateConditions([...rows, { field: defaultField, op: "eq", value: "" }]);
  }, [rows, onUpdateConditions, availableFields, hasSource]);

  const handleUpdateRow = useCallback(
    (rowIndex: number, updates: Partial<ConditionRow>) => {
      const newRows = rows.map((r, i) => (i === rowIndex ? { ...r, ...updates } : r));
      onUpdateConditions(newRows);
    },
    [rows, onUpdateConditions],
  );

  const handleDeleteRow = useCallback(
    (rowIndex: number) => {
      onUpdateConditions(rows.filter((_, i) => i !== rowIndex));
    },
    [rows, onUpdateConditions],
  );

  return (
    <div
      data-testid={`branch-editor-${branch.name}`}
      className="rounded border border-line-soft bg-bg-3 p-2"
    >
      <div className="flex items-center gap-1.5">
        {!isDefault && (
          <div className="flex flex-col">
            {onMoveUp && (
              <button
                onClick={onMoveUp}
                className="cursor-pointer text-fg-4 hover:text-fg-3"
                style={{ fontSize: "8px", lineHeight: 1 }}
                title="Move up"
              >
                ▲
              </button>
            )}
            {onMoveDown && (
              <button
                onClick={onMoveDown}
                className="cursor-pointer text-fg-4 hover:text-fg-3"
                style={{ fontSize: "8px", lineHeight: 1 }}
                title="Move down"
              >
                ▼
              </button>
            )}
          </div>
        )}
        {isDefault ? (
          <div className="flex min-w-0 flex-1 items-center gap-1.5">
            <span className="font-medium text-fg-3" style={{ fontSize: "11px" }}>
              default
            </span>
            <span
              className="rounded bg-fg-4/20 px-1 text-fg-4"
              style={{ fontSize: "8px" }}
            >
              else
            </span>
          </div>
        ) : (
          <input
            value={branch.name}
            onChange={(e) => onUpdateName(e.target.value)}
            className="min-w-0 flex-1 rounded border border-line-strong bg-bg-4 px-2 py-0.5 text-fg outline-none focus:border-acc"
            style={{ fontSize: "11px" }}
            placeholder="branch name"
          />
        )}
        <SidePicker value={branch.side ?? "right"} onChange={onUpdateSide} />
        {!isDefault && (
          <button
            onClick={onDelete}
            className="cursor-pointer text-fg-4 hover:text-st-failed"
            title="Delete branch"
          >
            <Trash2 size={12} />
          </button>
        )}
      </div>

      {!isDefault && (
        <div className="mt-1.5 flex flex-col gap-1">
          {rows.map((row, ri) => (
            <ConditionRowEditor
              key={ri}
              row={row}
              availableFields={availableFields}
              onUpdate={(updates) => handleUpdateRow(ri, updates)}
              onDelete={() => handleDeleteRow(ri)}
            />
          ))}
          <button
            onClick={handleAddCondition}
            disabled={!hasSource}
            className="cursor-pointer self-start text-fg-4 hover:text-acc disabled:cursor-not-allowed disabled:opacity-40"
            style={{ fontSize: "10px" }}
            data-testid="add-condition"
          >
            + Add condition
          </button>
        </div>
      )}
    </div>
  );
}

function ConditionRowEditor({
  row,
  availableFields,
  onUpdate,
  onDelete,
}: {
  row: ConditionRow;
  availableFields: FieldSource[];
  onUpdate: (updates: Partial<ConditionRow>) => void;
  onDelete: () => void;
}) {
  const selectedField = availableFields.find((f) => f.name === row.field);
  const isEnum = selectedField?.decl?.type === "enum" && selectedField.decl.allowed;

  return (
    <div className="flex items-center gap-1" data-testid="condition-row">
      <select
        value={row.field}
        onChange={(e) => onUpdate({ field: e.target.value, value: "" })}
        className="min-w-0 flex-1 rounded border border-line-strong bg-bg-4 px-1.5 py-0.5 text-fg outline-none focus:border-acc"
        style={{ fontSize: "10px" }}
        data-testid="field-dropdown"
      >
        {!availableFields.some((f) => f.name === row.field) && (
          <option value={row.field}>{row.field}</option>
        )}
        {availableFields.map((f) => (
          <option key={f.name} value={f.name}>
            {f.name}
          </option>
        ))}
      </select>
      <select
        value={row.op}
        onChange={(e) => onUpdate({ op: e.target.value as Operator })}
        className="rounded border border-line-strong bg-bg-4 px-1 py-0.5 text-fg-3 outline-none"
        style={{ fontSize: "10px" }}
        data-testid="op-dropdown"
      >
        {OPERATORS.map((op) => (
          <option key={op} value={op}>
            {op}
          </option>
        ))}
      </select>
      {isEnum ? (
        <select
          value={row.value}
          onChange={(e) => onUpdate({ value: e.target.value })}
          className="min-w-0 flex-1 rounded border border-line-strong bg-bg-4 px-1.5 py-0.5 text-fg outline-none focus:border-acc"
          style={{ fontSize: "10px" }}
          data-testid="value-dropdown"
        >
          <option value="">—</option>
          {selectedField!.decl!.allowed!.map((v) => (
            <option key={v} value={v}>
              {v}
            </option>
          ))}
        </select>
      ) : (
        <input
          value={row.value}
          onChange={(e) => onUpdate({ value: e.target.value })}
          className="min-w-0 flex-1 rounded border border-line-strong bg-bg-4 px-1.5 py-0.5 text-fg outline-none focus:border-acc"
          style={{ fontSize: "10px" }}
          placeholder="value"
        />
      )}
      <button
        onClick={onDelete}
        className="cursor-pointer text-fg-4 hover:text-st-failed"
        style={{ fontSize: "10px" }}
        data-testid="delete-condition"
      >
        ×
      </button>
    </div>
  );
}

function NameInput({
  value,
  placeholder,
  onCommit,
}: {
  value: string;
  placeholder: string;
  onCommit: (v: string) => void;
}) {
  return (
    <input
      defaultValue={value}
      onBlur={(e) => onCommit(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onCommit((e.target as HTMLInputElement).value);
          (e.target as HTMLInputElement).blur();
        }
      }}
      className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
      placeholder={placeholder}
    />
  );
}
