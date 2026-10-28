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
          className="inline-flex min-w-0 max-w-full items-center gap-0.5 overflow-hidden rounded-full border border-st-await/50 bg-st-await/14 px-1.5 py-px text-st-await"
          style={{ fontSize: "9px" }}
          data-testid="trigger-suspended"
        >
          <Pause size={8} className="shrink-0" aria-hidden="true" />
          <span className="truncate">suspended</span>
        </span>
      </Tooltip>
    </>
  );
}
