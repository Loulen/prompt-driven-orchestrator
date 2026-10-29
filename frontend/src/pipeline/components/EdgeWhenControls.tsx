// The controls of the edge detail panel's When and Display sections: the
// `else` toggle, the panel's switch, and one condition row of a `when:` clause.
import { X } from "lucide-react";
import { IconButton, TextInput } from "../../shared/ui/kit";
import { FormRow, Toggle } from "./InspectorPrimitives";
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
      label="Else"
      on={isElse}
      onToggle={onToggle}
      testId="else-toggle"
      title="Default branch: fires only when no sibling edge matched"
    />
  );
}

/**
 * The panel's switch row, shared by the When section's `else` toggle and the
 * Display section's two (#845) — one control, so a layout switch is never read
 * as a different KIND of thing from the semantic one above it.
 *
 * `note` is the faint suffix the Output labels switch uses to say its value is
 * still derived ("· default").
 */
export function Switch({
  label,
  note,
  on,
  onToggle,
  testId,
  title,
}: {
  /** Kept for the callers that still pass one: the row has no icon. */
  icon?: React.ReactNode;
  label: string;
  note?: string;
  on: boolean;
  onToggle: (next: boolean) => void;
  testId: string;
  title: string;
}) {
  return (
    <FormRow label={label} help={title}>
      <span className="ei-line">
        <Toggle checked={on} onChange={onToggle} label={title} testid={testId} />
        {note && (
          <span className="ei-meta" data-testid={`${testId}-note`}>
            {note}
          </span>
        )}
      </span>
    </FormRow>
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
    <div className="ei-cond" data-testid="condition-row">
      {/* The output is the row's FIRST control (#843): a clause reads one port's
          frontmatter (ADR-0073 §3), and reading that next to the property it
          qualifies beats a separate chooser elsewhere in the panel. Shown even
          on a single-port edge, so rows do not change shape when a port is
          added. */}
      <select
        value={row.port}
        onChange={(e) => onUpdate({ port: e.target.value })}
        className="ei-select ei-select--mono"
        aria-label="Output"
        data-testid="condition-port-dropdown"
        title={`Reads the frontmatter of ${row.port}`}
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
        className="ei-select ei-select--mono ei-cond__field"
        aria-label="Field"
        title={row.field}
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
        className="ei-select ei-select--mono ei-cond__op"
        aria-label="Operator"
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
        <div className="d-seg d-seg--sm" role="radiogroup" aria-label="Value" data-testid="bool-toggle">
          {(["true", "false"] as const).map((v) => (
            <button
              key={v}
              type="button"
              role="radio"
              aria-checked={row.value === v}
              className="d-seg__btn"
              onClick={() => onUpdate({ value: v, valueType: "bool" })}
              data-testid={`bool-${v}`}
            >
              {v}
            </button>
          ))}
        </div>
      ) : isEnum && !isList ? (
        <select
          value={row.value}
          onChange={(e) => onUpdate({ value: e.target.value })}
          className="ei-select ei-select--mono ei-cond__value"
          aria-label="Value"
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
        <TextInput
          mono
          value={row.value}
          onChange={(e) => onUpdate({ value: e.target.value })}
          placeholder={isList ? "a, b, c" : "value"}
          aria-label="Value"
          data-testid="value-input"
        />
      )}

      <IconButton icon={X} size="sm" label="Delete condition" onClick={onDelete} data-testid="delete-condition" />
    </div>
  );
}
