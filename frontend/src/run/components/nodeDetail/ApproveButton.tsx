import { forwardRef } from "react";
import { Check, LockOpen } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { RunStatus } from "../../../shared/generated/RunStatus";
import type { PortIO } from "../../api/nodes";
import { SplitButton } from "../../../shared/ui/kit";
import type { MenuEntry } from "../../../shared/ui/kit";
import { isMarkCompletable } from "../../model/nodeStatus";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import type { NodeGestureRefusals } from "./types";

export const AUTO_APPROVE_LABEL = "Auto-approve when ready";
export const AUTO_APPROVE_ARMED = "Auto-approve armed";

/** The outputs of the iteration on screen that are not on the blackboard yet. */
function missingOutputs(outputs: readonly PortIO[]): string[] {
  return outputs.filter((p) => p.missing).map((p) => p.port);
}

export interface ApproveButtonProps {
  node: NodeState;
  /** The iteration on screen: Approve shows on the latest one only. */
  selectedIter: number;
  /** The Run's status: a paused or archived Run takes no completion. */
  runStatus: RunStatus;
  /** The outputs of the iteration on screen, `missing` per port; `null`
   *  until they are read for it (Approve waits rather than guess). */
  outputs: readonly PortIO[] | null;
  /** CORE-205: the gestures the caller may not make, and why. */
  refused: Pick<NodeGestureRefusals, "markComplete" | "release">;
  markVerdict: ({ iter: number } & MarkVerdict) | null;
  releaseVerdict: ({ iter: number } & ReleaseVerdict) | null;
  markComplete: () => void;
  release: () => void;
  size?: "sm" | "md";
  /** « Show Approve » of the waiting banner: an amber ring around it. */
  ringed?: boolean;
}

/**
 * Approve ▾, the one completion gesture of a node: Approve completes it now
 * (`mark_node_done`), inert in a lighter ruby while an output is missing; the
 * chevron, on an interactive iteration whose session lives, offers « Auto-approve
 * when ready » (`release_node_completion`), checked and inert once armed, a
 * blue dot on the chevron then. Hidden on an older iteration, on an archived
 * or paused Run, and once the node no longer completes. The ref reaches the
 * main button.
 */
const ApproveButton = forwardRef<HTMLButtonElement, ApproveButtonProps>(function ApproveButton(
  {
    node,
    selectedIter,
    runStatus,
    outputs,
    refused,
    markVerdict,
    releaseVerdict,
    markComplete,
    release,
    size = "sm",
    ringed = false,
  },
  ref,
) {
  if (selectedIter !== node.iter || !isMarkCompletable(node.status, runStatus)) return null;
  const iteration = node.iterations?.find((i) => i.iter === selectedIter);
  const iterStatus = iteration?.status ?? node.status;
  const canRelease =
    iteration?.interactive === true && (iterStatus === "running" || iterStatus === "awaiting_user");
  const released = iteration?.completion_released === true;
  const releasing = releaseVerdict?.iter === selectedIter && releaseVerdict.kind === "pending";
  const approving = markVerdict?.iter === selectedIter && markVerdict.kind === "pending";
  const missing = outputs ? missingOutputs(outputs) : [];

  const entries: MenuEntry[] = canRelease
    ? [
        {
          label: releasing ? "Releasing…" : AUTO_APPROVE_LABEL,
          icon: LockOpen,
          checked: released,
          disabled: released || releasing,
          disabledReason: released ? undefined : refused.release,
          onSelect: release,
        },
      ]
    : [];

  return (
    <span className="ri-approve" data-ring={ringed || undefined} data-testid="approve">
      <SplitButton
        ref={ref}
        icon={Check}
        tone="primary"
        size={size}
        data-testid="approve-btn"
        disabledReason={refused.markComplete}
        pendingReason={
          outputs === null
            ? "Reading the outputs…"
            : missing.length > 0
              ? `Missing outputs: ${missing.join(", ")}`
              : approving
                ? "Approving…"
                : undefined
        }
        onClick={markComplete}
        entries={entries}
        menuLabel="Approve options"
        chevronTip={released ? AUTO_APPROVE_ARMED : undefined}
        indicator={released}
      >
        Approve
      </SplitButton>
    </span>
  );
});

export default ApproveButton;
