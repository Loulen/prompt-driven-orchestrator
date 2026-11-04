import { useMemo, useState } from "react";
import { AppearancePreviewContext } from "../../../hooks/useElementMarks";
import { InfoTab } from "../../../pipeline/components/InfoTab";
import { SAMPLE_PIPELINE, sampleRunState } from "../../model/runCanvasPreview";
import type { RecentRun } from "../../store/useRecentRun";

const nothing = () => {};

/**
 * The preview of the Run info tab block: the tab itself (`InfoTab`) on the most recent
 * Run the user sees, or on a sample Run without extension sections when they see none,
 * at the panel's width (360 px). Inert: nothing in it reacts. `pointed` is the element
 * whose switch the pointer or the focus is on.
 */
export default function RunInfoPreview({
  recent,
  none,
  pointed,
}: {
  recent: RecentRun | null;
  /** The user sees no Run. */
  none: boolean;
  pointed: string | null;
}) {
  const sample = recent == null;
  // The sample starts 14 minutes before the preview opens.
  const [sampleRun] = useState(() => sampleRunState(Date.now()));
  const run = recent?.run ?? sampleRun;
  const pipeline = sample ? SAMPLE_PIPELINE : recent.pipeline;
  const context = useMemo(() => ({ pointed, sample }), [pointed, sample]);
  const caption = recent ? `Your most recent run · ${recent.run.pipeline_name}` : none ? "Sample run · you see no run yet" : "Sample run";
  return (
    <AppearancePreviewContext.Provider value={context}>
      <figure className="set-preview" data-testid="appearance-preview-run-info" data-sample={sample || undefined}>
        <div className="set-preview__info" inert>
          <InfoTab run={run} pipeline={pipeline} pipelineName={run.pipeline_name} pipelineId={null} onOpenDiff={nothing} />
        </div>
        <figcaption className="set-preview__caption">{caption}</figcaption>
      </figure>
    </AppearancePreviewContext.Provider>
  );
}
