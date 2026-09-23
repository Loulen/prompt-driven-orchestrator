// The carried-outputs side of an edge (ADR-0073 / #843).
//
// An edge carries one or more output ports of the SAME source node and stays a
// single edge for the runtime: it fires once, at the source node's completion,
// and drops one emergent input per carried port on the target.
//
// `EdgeSource` has two YAML shapes on purpose — `{node, port}` for one carried
// port, `{node, ports}` for several — because the single-port shape is the one
// every pre-#843 pipeline is written in and it has to come back out of a save
// byte for byte. This module is the single place that knows about the pair; the
// rest of the app talks in terms of "the ports this edge carries".

import type { EdgeDef, EdgeSource, NodeDef, PipelineDef } from "../types";

/**
 * The ports this edge carries, in authored order. Never empty for a parsed
 * edge; an edge whose source names neither key (hand-broken JSON) reads as
 * carrying nothing, which every caller treats as "nothing to show".
 */
export function carriedPorts(source: EdgeSource): string[] {
  if (source.ports && source.ports.length > 0) return source.ports;
  if (source.port) return [source.port];
  return [];
}

/**
 * The edge's *primary* port: the first carried one. This is the legacy
 * single-port reading — the xyflow source handle the arrow binds to, the port an
 * `EdgeInfo` names, the artifact a structural `body`/`done` edge means.
 */
export function primaryPort(source: EdgeSource): string {
  return carriedPorts(source)[0] ?? "";
}

/** Whether this edge carries `port`. */
export function carries(source: EdgeSource, port: string): boolean {
  return carriedPorts(source).includes(port);
}

/**
 * Rebuilds a source around `ports`, picking the shape that matches: one port
 * re-emits `{node, port}` (so an edge that never gained a second output is
 * indistinguishable from its pre-#843 self in the saved file), several emit
 * `{node, ports}`. An empty list is refused by returning the source unchanged —
 * an edge always carries at least one output, and silently writing an empty
 * `ports: []` would make the daemon reject the whole pipeline on reload.
 */
export function withCarriedPorts(source: EdgeSource, ports: string[]): EdgeSource {
  if (ports.length === 0) return source;
  return ports.length === 1
    ? { node: source.node, port: ports[0] }
    : { node: source.node, ports: [...ports] };
}

/**
 * The output the condition editor points a fresh row at: `out` when the edge
 * carries it (the conventional single output of a work node), otherwise the
 * first carried port. Matches what the daemon reads when a clause is written
 * unqualified.
 */
export function defaultConditionPort(ports: string[]): string {
  return ports.includes("out") ? "out" : (ports[0] ?? "");
}

/**
 * The inputs this edge drops on its target: one per carried port (ADR-0073).
 * Each entry is `{ port, input }` — the source output and the name it lands
 * under on the target.
 *
 * A single-port edge keeps `target.port` as the input name: that is what a
 * declared handle (a `merge` input, End's `result`) and every pre-#843 file
 * mean. From two ports on there is no single name left to honour, so each input
 * takes its own port's name — the emergent rule ("connecting
 * `debugger.repro_steps` creates a `repro_steps` input", CONTEXT.md).
 *
 * Mirrors `EdgeDef::emergent_inputs` in `crates/pdo-daemon/src/pipeline.rs`.
 */
export function emergentInputs(edge: EdgeDef): { port: string; input: string }[] {
  const ports = carriedPorts(edge.source);
  if (ports.length <= 1) {
    return [{ port: ports[0] ?? "", input: edge.target.port }];
  }
  return ports.map((p) => ({ port: p, input: p }));
}

/** The output ports the source node of `edge` declares, in declaration order. */
export function declaredOutputs(pipeline: PipelineDef, edge: EdgeDef): string[] {
  const source: NodeDef | undefined = pipeline.nodes.find((n) => n.id === edge.source.node);
  return source?.outputs.map((p) => p.name) ?? [];
}

/**
 * Reorders `ports` into the source node's declaration order, so ticking the
 * second output of a node never leaves the edge carrying `[b, a]`. Ports the
 * node no longer declares (a since-renamed output still named in the file) keep
 * their relative order at the end rather than being dropped — the panel shows
 * what the YAML says, it does not quietly rewrite it.
 */
export function inDeclarationOrder(ports: string[], declared: string[]): string[] {
  const known = declared.filter((p) => ports.includes(p));
  const unknown = ports.filter((p) => !declared.includes(p));
  return [...known, ...unknown];
}

/**
 * Sorts the `ports` list of every edge source in a serialized pipeline object,
 * IN PLACE, so the semantic diff compares the carried ports as a SET (ADR-0073
 * §2): two pipelines differing only by `[a, b]` vs `[b, a]` are the same
 * pipeline. Mirrors `SourceProjection` in
 * `crates/pdo-daemon/src/pipeline_semantics.rs`, which sorts the same list for
 * the library content hash — the two must agree or the star and the diff
 * contradict each other on the same edit.
 *
 * Feed it the throwaway output of `pipelineToYamlObject`; it mutates.
 */
export function canonicalizeCarriedPorts(
  obj: Record<string, unknown>,
): Record<string, unknown> {
  const edges = obj.edges;
  if (!Array.isArray(edges)) return obj;
  for (const edge of edges as Record<string, unknown>[]) {
    const source = edge.source as Record<string, unknown> | undefined;
    if (source && Array.isArray(source.ports)) {
      edge.source = { ...source, ports: [...(source.ports as string[])].sort() };
    }
  }
  return obj;
}
