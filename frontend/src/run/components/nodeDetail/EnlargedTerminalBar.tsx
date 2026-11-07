import type { ReactNode, Ref } from "react";
import type { NodeState } from "../../../shared/generated/NodeState";
import { StatusIcon } from "../../../shared/ui/kit";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { MarkCompleteVerdict, ReleaseCompletionVerdict } from "./verdicts";
import IterMark from "../IterMark";

/** Which gesture of the enlarged bar last spoke, so its verdict shows under it. */
export type GesturePopover = "release" | "mark" | null;

/**
 * The completion gesture of the node in the enlarged terminal: the panel's
 * Approve ▾, in the normal size, by the same path and on the same rules (#44,
 * CORE-212).
 */
export interface TerminalCompletion {
  /** Approve ▾ shows (latest iteration, the daemon would complete the node). */
  show: boolean;
  /** Approve ▾ of the enlarged bar; each of its gestures says which spoke, so
   *  its verdict opens under the bar. */
  renderApprove: (onGesture: (gesture: Exclude<GesturePopover, null>) => void) => ReactNode;
  released: boolean;
  releaseVerdict: ({ iter: number } & ReleaseVerdict) | null;
  markVerdict: ({ iter: number } & MarkVerdict) | null;
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
