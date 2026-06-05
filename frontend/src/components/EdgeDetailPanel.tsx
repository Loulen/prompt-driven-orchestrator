import { useCallback } from "react";
import { ArrowRight, Plus, X, Activity } from "lucide-react";
import { useEditStore } from "../stores/editStore";
import type { EdgeDef, EdgeTriggerStatus } from "../types";
import {
  OPERATORS,
  whenToRows,
  rowsToWhen,
  type ConditionRow,
  type Operator,
} from "../lib/whenClause";
import { edgeConditionFields, isBoolField, type EdgeConditionField } from "../lib/edgeFields";
import { SectionHead } from "./InspectorPrimitives";

interface Props {
  /**
   * Runtime trigger status for the selected edge, when available. Panel-only —
   * the canvas never renders this (design screen 02, ADR-0011).
   */
  trigger?: EdgeTriggerStatus | null;
}

const OP_SYMBOLS: Record<Operator, string> = {
  eq: "=",
  neq: "≠",
  lt: "<",
  lte: "≤",
  gt: ">",
  gte: "≥",
  in: "in",
  not_in: "not in",
};

export default function EdgeDetailPanel({ trigger = null }: Props) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateEdge = useEditStore((s) => s.updateEdge);

  const tab = openTabs.find((t) => t.id === activeTabId);
  const edgeIndex = selection.kind === "edge" ? selection.edgeIndex ?? null : null;
  const edge: EdgeDef | null =
    tab && edgeIndex != null ? tab.pipeline.edges[edgeIndex] ?? null : null;

  const fields = edge ? edgeConditionFields(tab!.pipeline, edge) : [];
  const sourceNode = edge
    ? tab!.pipeline.nodes.find((n) => n.id === edge.source.node)
    : null;
  const targetNode = edge
    ? tab!.pipeline.nodes.find((n) => n.id === edge.target.node)
    : null;

  const rows = whenToRows(edge?.when);

  const commitRows = useCallback(
    (next: ConditionRow[]) => {
      if (edgeIndex == null) return;
      const when = rowsToWhen(next);
      // `when:` and `else:` are mutually exclusive (ADR-0011). Authoring a
      // condition on a fallback edge converts it to a guarded edge.
      updateEdge(edgeIndex, when ? { when, else: false } : { when });
    },
    [edgeIndex, updateEdge],
  );

  if (!tab || !edge) return null;

  const handleAddCondition = () => {
    const defaultField = fields[0]?.name ?? "iter";
    commitRows([
      ...rows,
      withTypeHint(
        { field: defaultField, op: "eq", value: defaultValueFor(defaultField, fields) },
        fields,
      ),
    ]);
  };

  const handleUpdateRow = (i: number, updates: Partial<ConditionRow>) => {
    const next = rows.map((r, idx) => {
      if (idx !== i) return r;
      // A field change resets the value and recomputes the bool type hint.
      const merged = { ...r, ...updates };
      return withTypeHint(updates.field !== undefined ? { ...merged, value: defaultValueFor(merged.field, fields) } : merged, fields);
    });
    commitRows(next);
  };

  const handleDeleteRow = (i: number) => {
    commitRows(rows.filter((_, idx) => idx !== i));
  };

  const fromName = sourceNode?.name ?? edge.source.node;
  const toName = targetNode?.name ?? edge.target.node;

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto" data-testid="edge-detail-panel">
      {/* Header — route */}
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <ArrowRight size={14} className="shrink-0 text-acc" />
        <div className="min-w-0">
          <div className="flex items-center gap-1 font-medium text-fg" style={{ fontSize: "12.5px" }}>
            <span className="truncate">{fromName}</span>
            <span className="font-mono text-fg-4" style={{ fontSize: "10.5px" }}>.{edge.source.port}</span>
            <ArrowRight size={11} className="shrink-0 text-fg-4" />
            <span className="truncate">{toName}</span>
          </div>
          <div className="mt-0.5 text-fg-4" style={{ fontSize: "10px" }}>
            edge · conditional route
          </div>
        </div>
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        {/* When */}
        <SectionHead title="When" count={rows.length} />
        <div className="flex flex-col gap-2" data-testid="when-editor">
          {rows.map((row, i) => (
            <ConditionRowEditor
              key={i}
              row={row}
              fields={fields}
              onUpdate={(updates) => handleUpdateRow(i, updates)}
              onDelete={() => handleDeleteRow(i)}
            />
          ))}
          <button
            onClick={handleAddCondition}
            className="flex items-center gap-1 self-start rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-3 hover:border-acc hover:text-acc"
            style={{ fontSize: "10.5px" }}
            data-testid="add-condition"
          >
            <Plus size={12} />
            Add condition
          </button>
          {rows.length === 0 && (
            <div className="text-fg-4" style={{ fontSize: "10px" }}>
              No condition — this edge always fires.
            </div>
          )}
        </div>

        {/* Available fields */}
        <SectionHead title="Available fields" />
        <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.6 }}>
          Output schema of{" "}
          <span className="font-mono text-fg-3">
            {fromName}.{edge.source.port}
          </span>
          , plus <span className="font-mono text-acc">iter</span> — the counter of
          the enclosing region.
        </div>

        {/* Runtime trigger status — panel-only (never on canvas) */}
        <SectionHead title="Runtime" />
        <TriggerStatusView trigger={trigger} />
      </div>
    </aside>
  );
}

function ConditionRowEditor({
  row,
  fields,
  onUpdate,
  onDelete,
}: {
  row: ConditionRow;
  fields: EdgeConditionField[];
  onUpdate: (updates: Partial<ConditionRow>) => void;
  onDelete: () => void;
}) {
  const selectedField = fields.find((f) => f.name === row.field);
  const isEnum = selectedField?.decl?.type === "enum" && selectedField.decl.allowed;
  const isBool = isBoolField(fields, row.field);
  const isList = row.op === "in" || row.op === "not_in";

  return (
    <div className="flex items-center gap-1" data-testid="condition-row">
      <select
        value={row.field}
        onChange={(e) => onUpdate({ field: e.target.value })}
        className="min-w-0 flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
        style={{ fontSize: "10.5px" }}
        data-testid="field-dropdown"
      >
        {!fields.some((f) => f.name === row.field) && (
          <option value={row.field}>{row.field}</option>
        )}
        {fields.map((f) => (
          <option key={f.name} value={f.name}>
            {f.name}
            {f.isIter ? " (iter)" : ""}
          </option>
        ))}
      </select>

      <select
        value={row.op}
        onChange={(e) => onUpdate({ op: e.target.value as Operator })}
        className="rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
        style={{ fontSize: "10.5px" }}
        data-testid="op-dropdown"
      >
        {OPERATORS.map((op) => (
          <option key={op} value={op}>
            {OP_SYMBOLS[op]}
          </option>
        ))}
      </select>

      {isBool && !isList ? (
        <div className="flex overflow-hidden rounded border border-line-strong" data-testid="bool-toggle">
          <button
            onClick={() => onUpdate({ value: "true", valueType: "bool" })}
            className={`px-2 py-1 ${row.value === "true" ? "bg-acc text-bg-1" : "bg-bg-3 text-fg-3"}`}
            style={{ fontSize: "10.5px" }}
            data-testid="bool-true"
          >
            true
          </button>
          <button
            onClick={() => onUpdate({ value: "false", valueType: "bool" })}
            className={`px-2 py-1 ${row.value === "false" ? "bg-acc text-bg-1" : "bg-bg-3 text-fg-3"}`}
            style={{ fontSize: "10.5px" }}
            data-testid="bool-false"
          >
            false
          </button>
        </div>
      ) : isEnum && !isList ? (
        <select
          value={row.value}
          onChange={(e) => onUpdate({ value: e.target.value })}
          className="min-w-0 flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
          style={{ fontSize: "10.5px" }}
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
          className="min-w-0 flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
          style={{ fontSize: "10.5px" }}
          placeholder={isList ? "a, b, c" : "value"}
          data-testid="value-input"
        />
      )}

      <button
        onClick={onDelete}
        className="shrink-0 rounded p-1 text-fg-4 hover:text-st-failed"
        data-testid="delete-condition"
        title="Delete condition"
      >
        <X size={12} />
      </button>
    </div>
  );
}

function TriggerStatusView({ trigger }: { trigger: EdgeTriggerStatus | null }) {
  if (!trigger) {
    return (
      <div className="text-fg-4" style={{ fontSize: "10px" }} data-testid="trigger-status-empty">
        No run yet — trigger status appears here while a run evaluates this edge.
        This status is shown only in this panel, never on the canvas.
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1.5 rounded border border-line bg-bg-3 p-2" data-testid="trigger-status">
      <div className="flex items-center gap-1.5 text-fg-3" style={{ fontSize: "10px" }}>
        <Activity size={12} />
        trigger status · this run
      </div>
      <div className="flex items-center gap-2" style={{ fontSize: "11px" }}>
        <span
          className={`h-1.5 w-1.5 rounded-full ${trigger.fired ? "bg-st-done" : "bg-fg-5"}`}
        />
        <span className="text-fg-2">{trigger.fired ? "fired" : "not fired"}</span>
      </div>
      {trigger.last_value != null && (
        <div className="flex justify-between text-fg-3" style={{ fontSize: "10.5px" }}>
          last value
          <span className="font-mono text-fg-2">{trigger.last_value}</span>
        </div>
      )}
      {(trigger.iter != null || trigger.evaluated_at != null) && (
        <div className="flex justify-between text-fg-3" style={{ fontSize: "10.5px" }}>
          evaluated
          <span className="font-mono text-fg-2">
            {trigger.iter != null ? `iter ${trigger.iter}` : ""}
            {trigger.iter != null && trigger.evaluated_at ? " · " : ""}
            {trigger.evaluated_at ? formatTime(trigger.evaluated_at) : ""}
          </span>
        </div>
      )}
    </div>
  );
}

function withTypeHint(row: ConditionRow, fields: EdgeConditionField[]): ConditionRow {
  return isBoolField(fields, row.field) ? { ...row, valueType: "bool" } : { ...row, valueType: undefined };
}

function defaultValueFor(field: string, fields: EdgeConditionField[]): string {
  return isBoolField(fields, field) ? "true" : "";
}

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return iso;
  }
}
