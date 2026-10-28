import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { MarkCompleteVerdict, ReleaseCompletionVerdict } from "./verdicts";

/**
 * The verdicts of the completion gestures (Mark ready, Approve / Mark
 * complete), each a banner line at the top of the panel, for the iteration
 * on screen.
 */
export default function NodeVerdicts({
  selectedIter,
  completionReleased,
  releaseVerdict,
  markVerdict,
}: {
  selectedIter: number;
  completionReleased: boolean;
  releaseVerdict: ({ iter: number } & ReleaseVerdict) | null;
  markVerdict: ({ iter: number } & MarkVerdict) | null;
}) {
  return (
    <>
      {completionReleased ? (
        <ReleaseCompletionVerdict verdict={{ kind: "released" }} />
      ) : (
        releaseVerdict &&
        releaseVerdict.iter === selectedIter && <ReleaseCompletionVerdict verdict={releaseVerdict} />
      )}
      {/* #490: the verdict of the click, AT the gesture. Rendered for every
          outcome including `pending`, so nothing ever blinks out without a
          replacement. */}
      {markVerdict && markVerdict.iter === selectedIter && <MarkCompleteVerdict verdict={markVerdict} />}
    </>
  );
}
