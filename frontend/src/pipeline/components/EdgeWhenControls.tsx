// The controls of the edge detail panel's Conditions and Display sections: the
// `else` toggle, the panel's switch, and one condition row of a `when:` clause.
import { X } from "lucide-react";
import { IconButton, Segmented, Select, TextInput } from "../../shared/ui/kit";
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
 * The panel's switch row, shared by the Conditions section's `else` toggle and the
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

/**
 * The header of the condition rows: one small label over each column of the
 * grid the rows share, so every Output, Field, Op and Value sits at the same
 * width on every row.
 */
export function ConditionGridHead() {
  return (
    <div className="ei-cond ei-cond--head" aria-hidden data-testid="condition-grid-head">
      <span>Output</span>
      <span>Field</span>
      <span>Op</span>
      <span>Value</span>
      <span />
    </div>
  );
}

/**
 * One condition of a `when:` clause: the output it reads, the field, the
 * operator and the value, each a control of the kit at the same height, on
 * the grid {@link ConditionGridHead} labels.
 */
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
          added. A port named by a hand-written clause but no longer carried
          stays selectable: dropping it would leave the dropdown DISPLAYING
          another port while the row still reads the old one (the #454 trap). */}
      <span className="ei-cond__cell" data-testid="condition-port-dropdown" title={`Reads the frontmatter of ${row.port}`}>
        <Select<string>
          label="Output"
          mono
          value={row.port}
          onChange={(port) => onUpdate({ port })}
          options={[
            ...(ports.includes(row.port) ? [] : [{ value: row.port, label: row.port, hint: "not carried" }]),
            ...ports.map((p) => ({ value: p, label: p })),
          ]}
        />
      </span>

      {/* Grouped by where the value comes from, each with its type and the
          values it admits. A field the schema no longer declares stays shown. */}
      <span className="ei-cond__cell" data-testid="field-dropdown">
        <Select<string>
          label="Field"
          mono
          value={row.field}
          onChange={(field) => onUpdate({ field })}
          options={[
            ...(fields.some((f) => f.name === row.field)
              ? []
              : [{ value: row.field, label: row.field, hint: "not declared" }]),
            ...fields.map((f) => ({ value: f.name, label: f.name, group: f.group, hint: f.hint })),
          ]}
        />
      </span>

      {/* An operator the field no longer admits can still sit in a hand-written
          or pre-#456 YAML. Keep it selectable, exactly as the field dropdown
          keeps an unknown field: dropping it would leave the dropdown
          DISPLAYING `=` while the row still holds `gte` — the
          shows-one-sends-another trap of #454. */}
      <span className="ei-cond__cell" data-testid="op-dropdown">
        <Select<Operator>
          label="Operator"
          mono
          value={row.op}
          onChange={(op) => onUpdate({ op })}
          options={[
            ...(allowedOps.includes(row.op) ? [] : [{ value: row.op, label: OP_SYMBOLS[row.op] }]),
            ...allowedOps.map((op) => ({ value: op, label: OP_SYMBOLS[op] })),
          ]}
        />
      </span>

      <span className="ei-cond__cell">
        {isBool && !isList ? (
          <span className="ei-cond__fill" data-testid="bool-toggle">
            <Segmented<string>
              label="Value"
              fill
              value={row.value}
              onChange={(value) => onUpdate({ value, valueType: "bool" })}
              items={[
                { value: "true", label: "true" },
                { value: "false", label: "false" },
              ]}
            />
          </span>
        ) : isEnum && !isList ? (
          <span className="ei-cond__fill" data-testid="value-dropdown">
            <Select<string>
              label="Value"
              mono
              value={row.value}
              onChange={(value) => onUpdate({ value })}
              options={[{ value: "", label: "—" }, ...selectedField!.decl!.allowed!.map((v) => ({ value: v, label: v }))]}
            />
          </span>
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
      </span>

      <IconButton icon={X} size="sm" label="Delete condition" onClick={onDelete} data-testid="delete-condition" />
    </div>
  );
}
