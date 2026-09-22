import { OPERATORS, type Operator } from "./whenClause";
import { primaryPort } from "./edgePorts";
import type { PipelineDef, EdgeDef, FrontmatterFieldDecl } from "../types";

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
  pipeline: PipelineDef,
  edge: EdgeDef,
  port?: string,
): EdgeConditionField[] {
  const fields: EdgeConditionField[] = [];

  const readPort = port ?? primaryPort(edge.source);
  const sourceNode = pipeline.nodes.find((n) => n.id === edge.source.node);
  const sourcePort = sourceNode?.outputs.find((p) => p.name === readPort);
  if (sourcePort?.frontmatter) {
    for (const [name, decl] of Object.entries(sourcePort.frontmatter)) {
      fields.push({ name, decl });
    }
  }

  // The enclosing region's iteration counter. Always offered so an exhaust-exit
  // such as `iter >= max` can be authored even before a region is materialised.
  fields.push({ name: "iter", decl: null, isIter: true });

  for (const varName of Object.keys(pipeline.variables)) {
    fields.push({ name: `$${varName}`, decl: null });
  }

  return fields;
}

/** Whether the named field is declared as a boolean (drives the true/false toggle). */
export function isBoolField(fields: EdgeConditionField[], name: string): boolean {
  return fields.find((f) => f.name === name)?.decl?.type === "bool";
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
