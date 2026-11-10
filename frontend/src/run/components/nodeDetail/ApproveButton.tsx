import { forwardRef } from "react";
import { Check, LockOpen } from "lucide-react";
import type { NodeState } from "../../../shared/generated/NodeState";
import type { RunStatus } from "../../../shared/generated/RunStatus";
import type { PortIO } from "../../api/nodes";
import { SplitButton } from "../../../shared/ui/kit";
import type { MenuEntry } from "../../../shared/ui/kit";
import {
  APPROVE_LABEL,
  AUTO_APPROVE_ARMED,
  AUTO_APPROVE_ARMING,
  AUTO_APPROVE_LABEL,
  approveReleasable,
  approveShown,
  approveWaitReason,
  missingOutputs,
} from "../../model/approve";
import type { MarkVerdict, ReleaseVerdict } from "../../store/useNodeRun";
import type { NodeGestureRefusals } from "./types";


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
  /** The base of its `data-testid`s: the wrapper takes `<base>`, the button
   *  `<base>-btn`, the chevron `<base>-btn-menu`. The enlarged terminal
   *  renders a second one under another base. */
  testIdBase?: string;
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
    testIdBase = "approve",
  },
  ref,
) {
  if (!approveShown(node, selectedIter, runStatus)) return null;
  const iteration = node.iterations?.find((i) => i.iter === selectedIter);
  const canRelease = approveReleasable(iteration, node.status);
  const released = iteration?.completion_released === true;
  const releasing = releaseVerdict?.iter === selectedIter && releaseVerdict.kind === "pending";
  const approving = markVerdict?.iter === selectedIter && markVerdict.kind === "pending";

  const entries: MenuEntry[] = canRelease
    ? [
        {
          label: releasing ? AUTO_APPROVE_ARMING : AUTO_APPROVE_LABEL,
          icon: LockOpen,
          checked: released,
          disabled: released || releasing,
          disabledReason: released ? undefined : refused.release,
          onSelect: release,
        },
      ]
    : [];

  return (
    <span className="ri-approve" data-ring={ringed || undefined} data-testid={testIdBase}>
      <SplitButton
        ref={ref}
        icon={Check}
        tone="primary"
        size={size}
        data-testid={`${testIdBase}-btn`}
        disabledReason={refused.markComplete}
        pendingReason={approveWaitReason(outputs ? missingOutputs(outputs) : null) ?? (approving ? "Approving…" : undefined)}
        onClick={markComplete}
        entries={entries}
        menuLabel="Approve options"
        chevronTip={released ? AUTO_APPROVE_ARMED : undefined}
        indicator={released}
      >
        {APPROVE_LABEL}
      </SplitButton>
    </span>
  );
});

export default ApproveButton;
