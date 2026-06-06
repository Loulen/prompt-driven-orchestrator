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
 */
export function edgeConditionFields(
  pipeline: PipelineDef,
  edge: EdgeDef,
): EdgeConditionField[] {
  const fields: EdgeConditionField[] = [];

  const sourceNode = pipeline.nodes.find((n) => n.id === edge.source.node);
  const sourcePort = sourceNode?.outputs.find((p) => p.name === edge.source.port);
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
