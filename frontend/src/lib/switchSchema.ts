import type { PipelineDef, FrontmatterFieldDecl } from "../types";

export function resolveUpstreamSchema(
  pipeline: PipelineDef,
  switchNodeId: string,
): Record<string, FrontmatterFieldDecl> | null {
  const edge = pipeline.edges.find(
    (e) => e.target.node === switchNodeId && e.target.port === "in",
  );
  if (!edge) return null;
  const sourceNode = pipeline.nodes.find((n) => n.id === edge.source.node);
  if (!sourceNode) return null;
  const sourcePort = sourceNode.outputs.find((p) => p.name === edge.source.port);
  if (!sourcePort?.frontmatter) return null;
  return sourcePort.frontmatter;
}
