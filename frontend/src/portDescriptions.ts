import type { NodeType } from "./types";

const FIRST_CLASS_DESCRIPTIONS: Partial<
  Record<NodeType, Record<string, string>>
> = {
  "for-each": {
    "input:in": "Artifact containing the items list",
    "input:break": "Stops launching new iterations",
    "output:body": "Fires once per item, in parallel",
    "output:done": "Fires when all iterations complete",
  },
  merge: {
    "input:branches": "Accumulates all incoming branches",
    "output:merged": "Result artifact after merge",
  },
};

export function getPortDescription(
  nodeType: NodeType,
  kind: "input" | "output",
  portName: string,
  yamlDescription?: string | null,
): string {
  const key = `${kind}:${portName}`;
  const hardcoded = FIRST_CLASS_DESCRIPTIONS[nodeType]?.[key];
  if (hardcoded) return hardcoded;
  if (yamlDescription) return yamlDescription;
  return portName;
}
