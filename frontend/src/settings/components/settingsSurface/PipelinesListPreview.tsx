import { useMemo } from "react";
import { AppearancePreviewContext } from "../../../hooks/useElementMarks";
import LibraryRow from "../../../pipeline/components/LibraryRow";
import PipelineItemDecorations from "../../../pipeline/components/PipelineItemDecorations";
import ValidationIndicator from "../../../pipeline/components/ValidationIndicator";
import { validationOf } from "../../../pipeline/model/pipelineValidation";
import type { PipelineListEntry } from "../../../shared/generated/PipelineListEntry";
import type { Project } from "../../../workspace/model/project";
import { pipelinesListPreview } from "../../model/pipelinesListPreview";

const nothing = () => {};

/**
 * The preview of the Pipelines list block: the list's own rows (`LibraryRow`, with its
 * `ValidationIndicator` and who has its draft open) on the Pipelines the user sees
 * edited last, or on a sample Pipeline without extension marks, at the list's width.
 * Inert: nothing in it reacts. `pointed` is the element whose switch the pointer or
 * the focus is on.
 */
export default function PipelinesListPreview({
  pipelines,
  projects,
  pointed,
}: {
  pipelines: PipelineListEntry[];
  projects: Project[];
  pointed: string | null;
}) {
  const shown = useMemo(() => pipelinesListPreview(pipelines), [pipelines]);
  const context = useMemo(() => ({ pointed, sample: shown.sample }), [pointed, shown.sample]);
  return (
    <AppearancePreviewContext.Provider value={context}>
      <figure
        className="set-preview"
        data-testid="appearance-preview-pipelines-list"
        data-sample={shown.sample || undefined}
      >
        <div className="lp-pane set-preview__list" inert>
          <div className="lp-rows">
            {shown.pipelines.map((p) => (
              <LibraryRow
                key={p.id}
                testId={`preview-library-row-${p.id}`}
                name={p.name}
                invalid={p.invalid}
                showDuplicate={false}
                onOpen={nothing}
                onToggleSelect={nothing}
                onDelete={nothing}
                deleteTitle="Delete pipeline"
                modified={p.modified}
                draft={p.draft}
                version={p.version?.number}
                decorations={<PipelineItemDecorations pipeline={p} />}
                indicator={<ValidationIndicator validation={validationOf(p)} projects={projects} />}
              />
            ))}
          </div>
        </div>
        <figcaption className="set-preview__caption">{shown.caption}</figcaption>
      </figure>
    </AppearancePreviewContext.Provider>
  );
}
