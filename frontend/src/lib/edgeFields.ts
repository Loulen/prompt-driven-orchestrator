import { OPERATORS, isPortlessField, type ConditionRow, type Operator } from "./whenClause";
import { primaryPort } from "./edgePorts";
import type { EdgeDef } from "../shared/generated/EdgeDef";
import type { FrontmatterFieldDecl } from "../shared/generated/FrontmatterFieldDecl";
import type { EditorPipelineDef } from "../pipeline/model/pipeline";

/**
 * A field that can appear in an edge's `when:` clause. Either a frontmatter
 * field declared on the edge's source output port, the enclosing region's
 * `iter` counter (ADR-0011), or a pipeline variable (`$name`).
 */
export interface EdgeConditionField {
  name: string;
  decl: FrontmatterFieldDecl | null;
  /** True for the `iter` region counter (re-authorised in `when:` by ADR-0011). */
  isIter?: boolean;
  /** Where the value comes from, the header of its group in the field dropdown:
   *  « Output · <port> », « Loop » or « Pipeline variables ». */
  group: string;
  /** Its type and the values it admits, beside it in the dropdown. */
  hint: string;
}

/** The group of the loop's iteration counter in the field dropdown. */
export const LOOP_GROUP = "Loop";
/** The group of the pipeline's variables in the field dropdown. */
export const VARIABLES_GROUP = "Pipeline variables";

/** The type of a declared frontmatter field, then the values it allows. */
function declHint(decl: FrontmatterFieldDecl): string {
  return decl.allowed?.length ? `${decl.type} · ${decl.allowed.join(" | ")}` : decl.type;
}

/** The type of a pipeline variable, then its default when it has one. */
function variableHint(variable: { type: string; default?: unknown }): string {
  const fallback = variable.default;
  if (fallback == null || fallback === "") return variable.type;
  return `${variable.type} · default ${typeof fallback === "string" ? fallback : JSON.stringify(fallback)}`;
}

/**
 * Resolves the fields selectable in the `when:` editor for a given edge:
 * the source output port's frontmatter, plus `iter`, plus pipeline variables.
 * Conditions reference only these (ADR-0002): no free expressions, no LLM.
 *
 * `port` names WHICH carried output the row reads (#843 / ADR-0073 §3) — an edge
 * can carry several, and each condition row picks one. Omitted, it falls back to
 * the edge's primary port, which is the whole story for a single-port edge.
 */
export function edgeConditionFields(
  pipeline: EditorPipelineDef,
  edge: EdgeDef,
  port?: string,
): EdgeConditionField[] {
  const fields: EdgeConditionField[] = [];

  const readPort = port ?? primaryPort(edge.source);
  const sourceNode = pipeline.nodes.find((n) => n.id === edge.source.node);
  const sourcePort = sourceNode?.outputs.find((p) => p.name === readPort);
  if (sourcePort?.frontmatter) {
    for (const [name, decl] of Object.entries(sourcePort.frontmatter)) {
      fields.push({ name, decl, group: `Output · ${readPort}`, hint: declHint(decl) });
    }
  }

  // The enclosing region's iteration counter. Always offered so an exhaust-exit
  // such as `iter >= max` can be authored even before a region is materialised.
  fields.push({ name: "iter", decl: null, isIter: true, group: LOOP_GROUP, hint: "int · loop iteration" });

  for (const [varName, variable] of Object.entries(pipeline.variables)) {
    fields.push({ name: `$${varName}`, decl: null, group: VARIABLES_GROUP, hint: variableHint(variable) });
  }

  return fields;
}

/** Whether the named field is declared as a boolean (drives the true/false toggle). */
export function isBoolField(fields: EdgeConditionField[], name: string): boolean {
  return fields.find((f) => f.name === name)?.decl?.type === "bool";
}

/**
 * The type a condition's value is written with, from the field's declaration:
 * a `bool` writes a boolean, a `string` or an `enum` a string (an allowed value
 * `"2"` must not be saved as the number `2`, which the frontmatter's string
 * never equals). Numbers, and a field nobody declares, are inferred from the
 * text (`undefined`).
 */
export function declaredValueType(fields: EdgeConditionField[], name: string): ConditionRow["valueType"] {
  const type = fields.find((f) => f.name === name)?.decl?.type;
  if (type === "bool") return "bool";
  if (type === "string" || type === "enum") return "string";
  return undefined;
}

/** Equality only — a boolean has no ordering the predicate grammar can use (#456). */
const BOOL_OPERATORS: readonly Operator[] = ["eq", "neq"];

/**
 * The operators admissible on a given field (#456). A `bool` takes equality
 * only: `approved >= true` was authorable and savable before this, and ADR-0002
 * gives the engine no ordering to apply to it.
 *
 * Every other field keeps the full set — including a field this resolver does
 * not know (a stale `when:` referencing a since-renamed port). Narrowing an
 * unknown field would be guessing, and the panel's job is to stop nonsense, not
 * to hide what the YAML already says.
 */
export function operatorsForField(
  fields: EdgeConditionField[],
  name: string,
): readonly Operator[] {
  return isBoolField(fields, name) ? BOOL_OPERATORS : OPERATORS;
}

/**
 * Coerces `op` into what `name` admits, defaulting to `eq`. Called when the
 * field of a condition row changes: without it, switching an `iter >= 3` row
 * over to a bool field would leave `gte` in place and write the very clause
 * this restriction exists to prevent.
 */
export function clampOperator(
  fields: EdgeConditionField[],
  name: string,
  op: Operator,
): Operator {
  const allowed = operatorsForField(fields, name);
  return allowed.includes(op) ? op : "eq";
}

/**
 * Whether a row of `rows` already holds the pair `field`/`op` of `port`: the
 * clause keeps one value per pair. `iter` and `$variables` are spelled without
 * their output, so any row holds them.
 */
export function isConditionHeld(
  rows: ConditionRow[],
  cond: { port: string; field: string; op: Operator },
): boolean {
  return rows.some(
    (r) =>
      r.field === cond.field &&
      r.op === cond.op &&
      (r.port === cond.port || isPortlessField(cond.field)),
  );
}

/**
 * The field and operator a new condition row of `port` starts on: the first
 * pair no row already holds. A `when:` clause is a map per field and per
 * operator, so a new row on a held pair would overwrite that row's value
 * instead of adding one. `null` when every pair is held.
 */
export function nextFreeCondition(
  rows: ConditionRow[],
  port: string,
  fields: EdgeConditionField[],
): { field: string; op: Operator } | null {
  const held = (field: string, op: Operator) => isConditionHeld(rows, { port, field, op });
  const names = fields.length > 0 ? fields.map((f) => f.name) : ["iter"];
  for (const op of OPERATORS) {
    for (const field of names) {
      if (operatorsForField(fields, field).includes(op) && !held(field, op)) return { field, op };
    }
  }
  return null;
}
