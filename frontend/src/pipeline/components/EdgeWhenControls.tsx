// The controls of the edge detail panel's When and Display sections: the
// `else` toggle, the panel's switch, and one condition row of a `when:` clause.
import { X, CornerDownRight } from "lucide-react";
import { type ConditionRow, type Operator } from "../../lib/whenClause";
import { isBoolField, operatorsForField, type EdgeConditionField } from "../../lib/edgeFields";

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

/**
 * Toggle that marks the selected edge as the default (`else`) branch. An `else`
 * edge fires iff no sibling edge (same source output port) matched (ADR-0011,
 * CONTEXT.md → *Edges conditionnelles*). It is mutually exclusive with a `when:`
 * predicate, so the panel suppresses the predicate editor while it is on.
 */
export function ElseToggle({
  isElse,
  onToggle,
}: {
  isElse: boolean;
  onToggle: (next: boolean) => void;
}) {
  return (
    <Switch
      icon={<CornerDownRight size={12} className="shrink-0" />}
      label="Default (else)"
      on={isElse}
      onToggle={onToggle}
      testId="else-toggle"
      title="A default edge fires only when no sibling edge matched"
    />
  );
}

/**
 * The panel's switch chrome, shared by the When section's `else` toggle and the
 * Display section's two (#845) — one control, so a layout switch is never read
 * as a different KIND of thing from the semantic one above it.
 *
 * `note` is the faint suffix the Output labels switch uses to say its value is
 * still derived ("· default").
 */
export function Switch({
  icon,
  label,
  note,
  on,
  onToggle,
  testId,
  title,
}: {
  icon: React.ReactNode;
  label: string;
  note?: string;
  on: boolean;
  onToggle: (next: boolean) => void;
  testId: string;
  title: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={() => onToggle(!on)}
      className={`flex items-center justify-between gap-2 rounded border px-2 py-1.5 ${
        on
          ? "border-acc bg-bg-3 text-acc"
          : "border-line-strong bg-bg-3 text-fg-3 hover:border-acc hover:text-acc"
      }`}
      data-testid={testId}
      title={title}
    >
      <span className="flex items-center gap-1.5" style={{ fontSize: "11px" }}>
        {icon}
        {label}
        {note && (
          <span className="text-fg-4" style={{ fontSize: "9.5px" }} data-testid={`${testId}-note`}>
            {note}
          </span>
        )}
      </span>
      <span
        className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
          on ? "bg-acc" : "bg-fg-5"
        }`}
      >
        <span
          className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
            on ? "left-3" : "left-0.5"
          }`}
        />
      </span>
    </button>
  );
}

export function ConditionRowEditor({
  row,
  ports,
  fields,
  onUpdate,
  onDelete,
}: {
  row: ConditionRow;
  /** The outputs the edge carries — what the leading dropdown offers (#843). */
  ports: string[];
  fields: EdgeConditionField[];
  onUpdate: (updates: Partial<ConditionRow>) => void;
  onDelete: () => void;
}) {
  const selectedField = fields.find((f) => f.name === row.field);
  const isEnum = selectedField?.decl?.type === "enum" && selectedField.decl.allowed;
  const isBool = isBoolField(fields, row.field);
  const isList = row.op === "in" || row.op === "not_in";
  const allowedOps = operatorsForField(fields, row.field);

  return (
    <div className="flex items-center gap-1" data-testid="condition-row">
      {/* The output is the row's FIRST control (#843): a clause reads one port's
          frontmatter (ADR-0073 §3), and reading that next to the property it
          qualifies beats a separate chooser elsewhere in the panel. Shown even
          on a single-port edge, so rows do not change shape when a port is
          added. */}
      <select
        value={row.port}
        onChange={(e) => onUpdate({ port: e.target.value })}
        className="min-w-0 shrink-0 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg-2"
        style={{ fontSize: "10.5px", maxWidth: "88px" }}
        data-testid="condition-port-dropdown"
        title="The carried output whose frontmatter this condition reads"
      >
        {/* A port named by a hand-written clause but no longer carried stays
            selectable: dropping it would leave the `<select>` DISPLAYING another
            port while the row still reads the old one (the #454 trap). */}
        {!ports.includes(row.port) && <option value={row.port}>{row.port}</option>}
        {ports.map((p) => (
          <option key={p} value={p}>
            {p}
          </option>
        ))}
      </select>

      <select
        value={row.field}
        onChange={(e) => onUpdate({ field: e.target.value })}
        className="flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
        // A floor, not `min-w-0`: the property name is the point of the row, so
        // it is the last control allowed to shrink.
        style={{ fontSize: "10.5px", minWidth: "124px" }}
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
        {/* An operator the field no longer admits can still sit in a hand-written
            or pre-#456 YAML. Keep it selectable, exactly as the field dropdown
            above keeps an unknown field: dropping it would leave the `<select>`
            DISPLAYING `=` while the row still holds `gte` — the
            shows-one-sends-another trap of #454. */}
        {!allowedOps.includes(row.op) && (
          <option value={row.op}>{OP_SYMBOLS[row.op]}</option>
        )}
        {allowedOps.map((op) => (
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
