/**
 * The extension sections of the canvas inspectors (ADR-0081, #31): `node/config`
 * after a node's Outputs, `pipeline/config` after a template's Stats in the Info
 * tab of Pipeline info (#948). Each is headed like the host's own sections;
 * nothing without a contribution.
 * The inspectors save as they edit: these sections save the way their
 * extension decides.
 */

import { useMemo } from "react";
import ExtensionSections, { type RenderedSection } from "../../shared/extensions/ExtensionSections";
import type { NodeConfigContext, PipelineConfigContext } from "../../shared/extensions/contract";
import type { Attributes } from "../../shared/extensions/attributes";
import { SectionHead } from "./InspectorPrimitives";

function inspectorSection({ key, label, body }: RenderedSection) {
  return (
    <div className="flex flex-col gap-2" data-testid={`extension-section-${key}`}>
      <SectionHead title={label} />
      {body}
    </div>
  );
}

export function NodeConfigSections({
  nodeId,
  nodeName,
  pipelineId,
  pipelineName,
  readOnly,
}: {
  nodeId: string;
  nodeName: string | null;
  pipelineId: string;
  pipelineName: string;
  readOnly: boolean;
}) {
  const extra = useMemo(
    () => ({
      entity: { kind: "node", id: nodeId, attributes: {}, ...(nodeName ? { name: nodeName } : {}) },
      pipeline: { id: pipelineId, name: pipelineName },
      readOnly,
    }),
    [nodeId, nodeName, pipelineId, pipelineName, readOnly],
  );
  return (
    <ExtensionSections<NodeConfigContext> location="node/config" extra={extra} wrap={inspectorSection} />
  );
}

export function PipelineConfigSections({
  pipelineId,
  pipelineName,
  attributes,
}: {
  pipelineId: string;
  pipelineName: string;
  attributes: Attributes | undefined;
}) {
  const extra = useMemo(
    () => ({
      entity: { kind: "pipeline", id: pipelineId, name: pipelineName, attributes: attributes ?? {} },
    }),
    [pipelineId, pipelineName, attributes],
  );
  return (
    <ExtensionSections<PipelineConfigContext>
      location="pipeline/config"
      extra={extra}
      wrap={inspectorSection}
    />
  );
}
