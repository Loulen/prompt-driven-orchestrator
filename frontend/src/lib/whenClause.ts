// Encoding helpers for `when:` clauses (ADR-0002 mechanical predicate grammar,
// ADR-0011 conditions-on-edges). A clause is `{ field: { op: value } }`. The UI
// edits it as a flat list of rows (output / field / operator / value); these
// functions convert both ways. Shared by the edge detail panel (#147).
//
// #843 — which output a row reads. An edge can carry several outputs of its
// source node (ADR-0073), and a condition reads ONE of them. The port is encoded
// in the field key itself:
//
//     when: { has_design_work: { eq: true } }        # one carried port, implicit
//     when: { design.has_design_work: { eq: true } } # several, port named
//
// So a single-port edge — every pre-#843 edge — keeps the exact clause it
// already had, and the canvas pill (`formatWhenPill`, which renders the key
// verbatim) reads `has_design_work = true` there and `design.has_design_work =
// true` on a multi-port edge, which is the spelling the design asked for.
//
// `iter` and `$variables` are never qualified: they belong to the region and the
// pipeline, not to any output's frontmatter.

import { defaultConditionPort } from "./edgePorts";

export const OPERATORS = ["eq", "neq", "lt", "lte", "gt", "gte", "in", "not_in"] as const;
export type Operator = (typeof OPERATORS)[number];

export interface ConditionRow {
  /**
   * The carried output whose frontmatter this row reads (#843). Always set on a
   * row the editor produced; `""` only for a row decoded from an edge that
   * carries nothing.
   */
  port: string;
  field: string;
  op: Operator;
  value: string;
  /**
   * Type hint for `value` coercion. `"bool"` forces the written value to a
   * canonical YAML boolean (`true`/`false`) rather than a string, resolving the
   * true / 1 / True ambiguity (#147). `"string"` keeps a value that reads as a
   * number (`"42"`) a string. Absent means "infer" (numeric → number, otherwise
   * string). `whenToRows` sets it from the clause, so re-committing a row the
   * user did not touch writes back the type it was read with.
   */
  valueType?: "bool" | "string";
}

/**
 * Keys that name no output's frontmatter and so carry no qualifier: `iter` (the
 * enclosing region's counter), `$variables` (pipeline-scoped), and `any` (the
 * grammar's disjunction, ADR-0002 — its value is a list of clauses, not a
 * predicate on a field).
 */
export function isPortlessField(field: string): boolean {
  return field === "iter" || field === "any" || field.startsWith("$");
}

/**
 * Splits a clause key into `{port, field}`. A key is read as qualified only when
 * its prefix before the first `.` is one of the ports the edge actually carries
 * — a frontmatter field legitimately named `a.b` on an edge that carries no port
 * `a` stays one field name, which is the only reading that cannot corrupt an
 * existing file.
 */
export function splitFieldKey(
  key: string,
  ports: string[],
): { port: string; field: string } {
  const dot = key.indexOf(".");
  if (dot > 0) {
    const prefix = key.slice(0, dot);
    if (ports.includes(prefix)) {
      return { port: prefix, field: key.slice(dot + 1) };
    }
  }
  return { port: defaultConditionPort(ports), field: key };
}

/**
 * The clause key a row writes: qualified with its output as soon as the edge
 * carries more than one, bare while it carries one (where the port is implicit
 * and re-qualifying it would rewrite every existing pipeline).
 */
export function fieldKey(row: ConditionRow, ports: string[]): string {
  if (ports.length <= 1 || isPortlessField(row.field) || !row.port) return row.field;
  return `${row.port}.${row.field}`;
}

export function whenToRows(
  when: Record<string, unknown> | null | undefined,
  ports: string[] = [],
): ConditionRow[] {
  if (!when) return [];
  const rows: ConditionRow[] = [];
  for (const [key, predicate] of Object.entries(when)) {
    if (typeof predicate === "object" && predicate !== null && !Array.isArray(predicate)) {
      const { port, field } = splitFieldKey(key, ports);
      for (const [op, val] of Object.entries(predicate as Record<string, unknown>)) {
        const value = Array.isArray(val) ? JSON.stringify(val) : String(val ?? "");
        const valueType = typeHintFor(val);
        // No `valueType` key at all when inference already gives the type back.
        rows.push(
          valueType
            ? { port, field, op: op as Operator, value, valueType }
            : { port, field, op: op as Operator, value },
        );
      }
    }
  }
  return rows;
}

export function rowsToWhen(
  rows: ConditionRow[],
  ports: string[] = [],
): Record<string, unknown> | null {
  if (rows.length === 0) return null;
  const when: Record<string, Record<string, unknown>> = {};
  for (const row of rows) {
    const key = fieldKey(row, ports);
    if (!when[key]) when[key] = {};
    when[key][row.op] = parseValue(row);
  }
  return when;
}

/**
 * The grammar's map of field → predicate. A `when` the YAML holds as anything
 * else (a bare string) is the author's: passed through as is, never walked
 * key by key — a string would come back as `{"0": "i", "1": "t", …}`.
 */
function isClauseMap(when: unknown): when is Record<string, unknown> {
  return when != null && typeof when === "object" && !Array.isArray(when);
}

/**
 * Re-encodes a clause for a new set of carried ports (#843). Called when the
 * Outputs section ticks or unticks a port, and when a port rename reaches the
 * edges: the qualification changes shape (bare ⇄ `port.field`), and a key
 * reading a port that is no longer carried is re-pointed at one that still is
 * rather than left dangling (ADR-0073 §3).
 *
 * Rewrites KEYS ONLY, never round-trips through `ConditionRow`s. The row model
 * is lossy by construction — it drops what the editor cannot show, `any:` first
 * of all — and a port tick must not be able to delete a clause the panel simply
 * doesn't render.
 */
export function requalifyWhen(
  when: unknown,
  fromPorts: string[],
  toPorts: string[],
): Record<string, unknown> | null {
  // Not a map: returned as the edge holds it (typed as the grammar's map, like `EdgeDef.when`).
  if (!isClauseMap(when)) return (when ?? null) as Record<string, unknown> | null;
  if (Object.keys(when).length === 0) return null;
  const fallback = defaultConditionPort(toPorts);
  const out: Record<string, unknown> = {};
  for (const [key, predicate] of Object.entries(when)) {
    const { port, field } = splitFieldKey(key, fromPorts);
    const nextPort = toPorts.includes(port) ? port : fallback;
    out[fieldKey({ port: nextPort, field, op: "eq", value: "" }, toPorts)] = predicate;
  }
  return out;
}

/**
 * Renames the outputs a clause's keys point at (#843), keeping the same number
 * of carried ports. Called when a source node's output port is renamed: without
 * it the qualifier would still name the old port, `requalifyWhen` would find it
 * uncarried, and the clause would silently jump to another output.
 *
 * Keys only, like `requalifyWhen` — the row model is lossy.
 */
export function renameWhenPorts(
  when: unknown,
  ports: string[],
  rename: Map<string, string>,
): Record<string, unknown> | null {
  // Not a map: returned as the edge holds it (typed as the grammar's map, like `EdgeDef.when`).
  if (!isClauseMap(when)) return (when ?? null) as Record<string, unknown> | null;
  if (Object.keys(when).length === 0) return null;
  if (rename.size === 0) return when;
  const renamed = ports.map((p) => rename.get(p) ?? p);
  const out: Record<string, unknown> = {};
  for (const [key, predicate] of Object.entries(when)) {
    const { port, field } = splitFieldKey(key, ports);
    const nextPort = rename.get(port) ?? port;
    out[fieldKey({ port: nextPort, field, op: "eq", value: "" }, renamed)] = predicate;
  }
  return out;
}

/** What tells one row from another: no two rows share a port, a field and an operator. */
export function conditionKey(row: ConditionRow): string {
  return `${row.port}\u0000${row.field}\u0000${row.op}`;
}

/**
 * The rows in the order the editor last showed them. A clause groups its rows
 * by field (`{ verdict: { eq, neq } }`), so a condition added on a field
 * already used would come back next to it, not last: `order` (the keys of the
 * rows as shown) puts each where it was. A row `order` does not know (an undo,
 * an outside write) comes after, in the clause's order.
 */
export function inShownOrder(rows: ConditionRow[], order: readonly string[]): ConditionRow[] {
  const rank = new Map(order.map((key, i) => [key, i]));
  const known = rows.filter((r) => rank.has(conditionKey(r)));
  known.sort((a, b) => rank.get(conditionKey(a))! - rank.get(conditionKey(b))!);
  return [...known, ...rows.filter((r) => !rank.has(conditionKey(r)))];
}

/**
 * The type hint a clause value needs to come back unchanged through
 * `parseValue`: a bool would be read back as a string, a string that reads as a
 * number as a number. Numbers and lists infer back to themselves.
 */
function typeHintFor(val: unknown): ConditionRow["valueType"] {
  if (typeof val === "boolean") return "bool";
  if (typeof val === "string" && readsAsNumber(val)) return "string";
  return undefined;
}

function readsAsNumber(value: string): boolean {
  return value.trim() !== "" && !isNaN(Number(value));
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
  if (row.valueType === "string") return row.value;
  return readsAsNumber(row.value) ? Number(row.value) : row.value;
}
