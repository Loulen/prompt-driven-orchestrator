import { useMemo } from "react";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import ExtensionDecorations from "../../shared/extensions/ExtensionDecorations";
import type { DecorationContext } from "../../shared/extensions/contract";

/** The `pipeline/item` decorations of one row (ADR-0081, #31). */
export default function PipelineItemDecorations({ pipeline }: { pipeline: PipelineListEntry }) {
  const extra = useMemo(
    () => ({
      entity: {
        kind: "pipeline",
        id: pipeline.id,
        name: pipeline.name,
        attributes: pipeline.attributes ?? {},
      },
    }),
    [pipeline.id, pipeline.name, pipeline.attributes],
  );
  return (
    <ExtensionDecorations<DecorationContext>
      location="pipeline/item"
      extra={extra}
      nowrap
    />
  );
}
