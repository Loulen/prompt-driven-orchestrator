import type { ReactNode, Ref } from "react";
import { Check, CircleCheck, LockOpen } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import { IconButton, StatusIcon } from "../../../shared/ui/kit";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { MarkCompleteVerdict, ReleaseCompletionVerdict } from "./verdicts";
import IterMark from "../IterMark";

const RELEASE_LABEL = "Mark ready for completion";
const RELEASED_LABEL = "Completion released";
const MARK_COMPLETE_LABEL = "Approve";

/** Which gesture of the enlarged bar last spoke, so its verdict shows under it. */
export type GesturePopover = "release" | "mark" | null;

/**
 * The completion gestures of the node, as the panel's « Mark ready for
 * completion » / « Mark complete » buttons hold them: the enlarged terminal
 * offers the same two, by the same path (#44).
 */
export interface TerminalCompletion {
  /** `awaiting_user` on a live run: the gestures belong in the enlarged bar. */
  showGestures: boolean;
  canRelease: boolean;
  /** CORE-42: « Mark complete » is allowed to the caller (else hidden). */
  canMarkComplete: boolean;
  released: boolean;
  isReleasing: boolean;
  releaseVerdict: ({ iter: number } & ReleaseVerdict) | null;
  markVerdict: ({ iter: number } & MarkVerdict) | null;
  release: () => void;
  markComplete: () => void;
}

/** #44: the node's identity at the head of the enlarged terminal's toolbar. */
export function EnlargedTerminalIdentity({
  node,
  nodeName,
  iter,
}: {
  node: NodeState;
  nodeName?: string | null;
  iter: number;
}) {
  return (
    <span className="flex min-w-0 items-center gap-1.5" data-testid="term-identity">
      <span data-testid="term-identity-status" data-status={node.status} className="inline-flex">
        <StatusIcon kind="node" status={node.status} />
      </span>
      <span className="truncate text-sm font-medium text-text" data-testid="term-identity-name">
        {nodeName ?? node.node_id}
      </span>
      <span className="shrink-0 text-text-3">
        <IterMark iter={iter} />
      </span>
    </span>
  );
}

/** #44: « Mark ready for completion » and « Mark complete », compact. */
export function EnlargedCompletionGestures({
  completion,
  onGesture,
}: {
  completion: TerminalCompletion;
  onGesture: (gesture: Exclude<GesturePopover, null>) => void;
}) {
  const releaseLabel = completion.isReleasing ? "Releasing…" : completion.released ? RELEASED_LABEL : RELEASE_LABEL;
  return (
    <>
      {completion.canRelease && (
        <IconButton
          icon={completion.released ? CircleCheck : LockOpen}
          size="sm"
          label={releaseLabel}
          pressed={completion.released}
          disabled={completion.isReleasing}
          data-testid="term-release-completion"
          onClick={() => {
            onGesture("release");
            completion.release();
          }}
        />
      )}
      {completion.canMarkComplete && (
        <IconButton
          icon={Check}
          size="sm"
          label={MARK_COMPLETE_LABEL}
          data-testid="term-mark-complete"
          onClick={() => {
            onGesture("mark");
            completion.markComplete();
          }}
        />
      )}
    </>
  );
}

/**
 * The verdict of the last gesture made from the enlarged bar, in a popover under
 * it: the panel's verdict regions are hidden beneath the overlay.
 */
export function GestureVerdictPopover({
  gesture,
  completion,
  iter,
  popoverRef,
}: {
  gesture: GesturePopover;
  completion: TerminalCompletion;
  iter: number;
  popoverRef: Ref<HTMLDivElement>;
}) {
  const { released, releaseVerdict, markVerdict } = completion;
  let verdict: ReactNode = null;
  if (gesture === "release") {
    if (released) verdict = <ReleaseCompletionVerdict verdict={{ kind: "released" }} />;
    else if (releaseVerdict && releaseVerdict.iter === iter) {
      verdict = <ReleaseCompletionVerdict verdict={releaseVerdict} />;
    }
  } else if (gesture === "mark" && markVerdict && markVerdict.iter === iter) {
    verdict = <MarkCompleteVerdict verdict={markVerdict} />;
  }
  if (verdict === null) return null;
  return (
    <div
      ref={popoverRef}
      className="absolute right-2 top-7 z-10 w-[330px] overflow-hidden rounded border border-line bg-surface-1 shadow-lg"
      data-testid="term-gesture-popover"
    >
      {verdict}
    </div>
  );
}
