import type { ReactNode, Ref } from "react";
import { CheckCircle, LoaderCircle, LockOpen } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import { Tooltip } from "../../../shared/ui/tooltip";
import { STATUS_BG, STATUS_BORDER, STATUS_DOT, STATUS_TEXT } from "../../../nodeStyles";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import { STATUS_LABELS } from "./labels";
import { MarkCompleteVerdict, ReleaseCompletionVerdict } from "./verdicts";

const RELEASE_LABEL = "Mark ready for completion";
const RELEASED_LABEL = "Completion released";
const MARK_COMPLETE_LABEL = "Mark complete · take the artifacts as they are";

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
    <span className="flex items-center gap-1.5" data-testid="term-identity">
      <span
        className={`h-1.5 w-1.5 shrink-0 rounded-full ${STATUS_DOT[node.status]}${
          node.status === "running" ? " animate-pulse" : ""
        }`}
      />
      <span
        className="text-fg"
        style={{ fontSize: "12px", fontWeight: 500 }}
        data-testid="term-identity-name"
      >
        {nodeName ?? node.node_id}
      </span>
      <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
        iter {iter}
      </span>
      <span
        className={`rounded border px-1.5 py-px transition-colors duration-[250ms] ${STATUS_BORDER[node.status]} ${STATUS_BG[node.status]} ${STATUS_TEXT[node.status]}`}
        style={{ fontSize: "10px", fontWeight: 500 }}
        data-testid="term-identity-status"
        data-status={node.status}
      >
        {STATUS_LABELS[node.status] ?? node.status}
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
  const releaseLabel = completion.released ? RELEASED_LABEL : RELEASE_LABEL;
  return (
    <>
      {completion.canRelease && (
        <Tooltip content={releaseLabel}>
          <button
            onClick={() => {
              onGesture("release");
              completion.release();
            }}
            disabled={completion.isReleasing}
            aria-label={releaseLabel}
            className="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-st-await transition-colors hover:bg-st-await-bg disabled:cursor-wait"
            data-testid="term-release-completion"
          >
            {completion.isReleasing ? (
              <LoaderCircle size={12} className="animate-spin" />
            ) : completion.released ? (
              <CheckCircle size={12} />
            ) : (
              <LockOpen size={12} />
            )}
          </button>
        </Tooltip>
      )}
      <Tooltip content={MARK_COMPLETE_LABEL}>
        <button
          onClick={() => {
            onGesture("mark");
            completion.markComplete();
          }}
          aria-label={MARK_COMPLETE_LABEL}
          className="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-st-done transition-colors hover:bg-st-done-bg"
          data-testid="term-mark-complete"
        >
          <CheckCircle size={12} />
        </button>
      </Tooltip>
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
      className="absolute right-2 top-9 z-10 w-[330px] rounded-lg border border-line-strong bg-bg-2 p-1.5 shadow-lg"
      data-testid="term-gesture-popover"
    >
      {verdict}
    </div>
  );
}
