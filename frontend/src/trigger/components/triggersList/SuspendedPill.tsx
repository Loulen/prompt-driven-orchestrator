import { Pause } from "lucide-react";
import type { TriggerSuspension } from "../../../shared/generated/TriggerSuspension";
import { Tooltip } from "../../../shared/ui/kit";

/**
 * The amber « suspended » pill of a Trigger an extension suspends (API 0.8):
 * it fires neither on its schedule nor by Run now. The reason, and the
 * extension, are in its tooltip. It shrinks with its line (its label ends in
 * an ellipsis) rather than spill under a row's hover bar.
 */
export default function SuspendedPill({ suspension }: { suspension: TriggerSuspension }) {
  return (
    <Tooltip label={suspension.reason} description={`Suspended by the extension ${suspension.extension}.`}>
      <span
        role="note"
        tabIndex={0}
        aria-label={`Suspended: ${suspension.reason}`}
        className="lp-suspended"
        data-testid="trigger-suspended"
      >
        <Pause size={9} strokeWidth={2} aria-hidden />
        <span>suspended</span>
      </span>
    </Tooltip>
  );
}
