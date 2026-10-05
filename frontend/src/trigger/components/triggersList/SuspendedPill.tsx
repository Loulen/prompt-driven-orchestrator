import { Pause } from "lucide-react";
import type { TriggerSuspension } from "../../../shared/generated/TriggerSuspension";
import { Tooltip, TooltipProvider } from "../../../shared/ui/tooltip";
import { suspensionTooltip } from "../../model/suspension";

/**
 * The amber « suspended » pill of a Trigger an extension suspends (API 0.8):
 * it fires neither on its schedule nor by Run now. The reason is in its
 * tooltip. Brings its own tooltip provider, like `InfoHint`.
 */
export default function SuspendedPill({ suspension }: { suspension: TriggerSuspension }) {
  return (
    <TooltipProvider>
      <Tooltip content={suspensionTooltip(suspension)}>
        <span
          role="note"
          tabIndex={0}
          aria-label={`Suspended: ${suspension.reason}`}
          className="inline-flex shrink-0 items-center gap-0.5 rounded-full border border-st-interrupted/50 bg-st-interrupted-bg px-1.5 py-px text-st-interrupted"
          style={{ fontSize: "9px" }}
          data-testid="trigger-suspended"
        >
          <Pause size={8} aria-hidden="true" />
          suspended
        </span>
      </Tooltip>
    </TooltipProvider>
  );
}
