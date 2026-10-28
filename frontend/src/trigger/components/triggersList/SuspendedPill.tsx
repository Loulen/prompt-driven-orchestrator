import { Pause } from "lucide-react";
import type { TriggerSuspension } from "../../../shared/generated/TriggerSuspension";
import { Tooltip } from "../../../shared/ui/tooltip";
import { suspensionTooltip } from "../../model/suspension";

/**
 * The amber « suspended » pill of a Trigger an extension suspends (API 0.8):
 * it fires neither on its schedule nor by Run now. The reason is in its
 * tooltip. Brings its own tooltip provider, like `InfoHint`. It shrinks with
 * its column (its label truncates) rather than spill under a row's hover
 * actions.
 */
export default function SuspendedPill({ suspension }: { suspension: TriggerSuspension }) {
  return (
    <>
      <Tooltip content={suspensionTooltip(suspension)}>
        <span
          role="note"
          tabIndex={0}
          aria-label={`Suspended: ${suspension.reason}`}
          className="d-chip tg-suspended-pill"
          data-testid="trigger-suspended"
        >
          <Pause size={10} strokeWidth={2} aria-hidden="true" />
          <span className="tg-suspended-pill__t">suspended</span>
        </span>
      </Tooltip>
    </>
  );
}
