import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { MarkCompleteVerdict, ReleaseCompletionVerdict } from "./verdicts";

/** The statuses a node still completes from: the verdicts of its gestures hold. */
const COMPLETABLE: ReadonlySet<NodeStatus> = new Set(["awaiting_user", "running", "failed", "stale", "interrupted"]);

/**
 * The verdicts of the completion gestures (Mark ready, Approve / Mark
 * complete), each a banner line at the top of the panel, for the iteration
 * on screen, while the node may still complete.
 */
export default function NodeVerdicts({
  status,
  selectedIter,
  completionReleased,
  releaseVerdict,
  markVerdict,
}: {
  status: NodeStatus;
  selectedIter: number;
  completionReleased: boolean;
  releaseVerdict: ({ iter: number } & ReleaseVerdict) | null;
  markVerdict: ({ iter: number } & MarkVerdict) | null;
}) {
  if (!COMPLETABLE.has(status)) return null;
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
