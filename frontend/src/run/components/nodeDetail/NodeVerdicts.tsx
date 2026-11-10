import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { MarkCompleteVerdict, ReleaseCompletionVerdict } from "./verdicts";

/** The statuses a node still completes from: the verdicts of its gestures hold. */
const COMPLETABLE: ReadonlySet<NodeStatus> = new Set(["awaiting_user", "running", "failed", "stale", "interrupted"]);

/**
 * The verdicts of the completion gestures (Approve, « Auto-approve when
 * ready ») that did not go through, each a banner line at the top of the
 * panel, for the iteration on screen, while the node may still complete. A
 * gesture that went through says nothing here: the blue dot on Approve ▾ and
 * the node's status do.
 */
export default function NodeVerdicts({
  status,
  selectedIter,
  releaseVerdict,
  markVerdict,
}: {
  status: NodeStatus;
  selectedIter: number;
  releaseVerdict: ({ iter: number } & ReleaseVerdict) | null;
  markVerdict: ({ iter: number } & MarkVerdict) | null;
}) {
  if (!COMPLETABLE.has(status)) return null;
  return (
    <>
      {releaseVerdict && releaseVerdict.iter === selectedIter && releaseVerdict.kind !== "pending" && (
        <ReleaseCompletionVerdict verdict={releaseVerdict} />
      )}
      {/* #490: the refusal of the click, at the gesture. */}
      {markVerdict && markVerdict.iter === selectedIter && markVerdict.kind !== "pending" && markVerdict.kind !== "completed" && (
        <MarkCompleteVerdict verdict={markVerdict} />
      )}
    </>
  );
}
