// Encoding helpers for `when:` clauses (ADR-0002 mechanical predicate grammar,
// ADR-0011 conditions-on-edges). A clause is `{ field: { op: value } }`. The UI
// edits it as a flat list of rows (field / operator / value); these functions
// convert both ways. Shared by the edge detail panel (#147).

export const OPERATORS = ["eq", "neq", "lt", "lte", "gt", "gte", "in", "not_in"] as const;
export type Operator = (typeof OPERATORS)[number];

export interface ConditionRow {
  field: string;
  op: Operator;
  value: string;
  /**
   * Type hint for `value` coercion. `"bool"` forces the written value to a
   * canonical YAML boolean (`true`/`false`) rather than a string, resolving the
   * true / 1 / True ambiguity (#147). Absent means "infer" (numeric → number,
   * otherwise string).
   */
  valueType?: "bool";
}

export function whenToRows(when: Record<string, unknown> | null | undefined): ConditionRow[] {
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

export function rowsToWhen(rows: ConditionRow[]): Record<string, unknown> | null {
  if (rows.length === 0) return null;
  const when: Record<string, Record<string, unknown>> = {};
  for (const row of rows) {
    if (!when[row.field]) when[row.field] = {};
    when[row.field][row.op] = parseValue(row);
  }
  return when;
}

function parseValue(row: ConditionRow): unknown {
  if (row.op === "in" || row.op === "not_in") {
    try {
      const arr = JSON.parse(row.value);
      if (Array.isArray(arr)) return arr;
    } catch {
      // fall through to comma-splitting
    }
    return row.value.split(",").map((s) => s.trim()).filter(Boolean);
  }
  if (row.valueType === "bool") {
    // Canonical boolean: the toggle only ever produces "true"/"false".
    return row.value === "true";
  }
  const num = Number(row.value);
  if (!isNaN(num) && row.value.trim() !== "") return num;
  return row.value;
}
