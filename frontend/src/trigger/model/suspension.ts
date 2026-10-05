import type { TriggerSuspension } from "../../shared/generated/TriggerSuspension";

/** What a suspended Trigger's tooltip and detail say (API 0.8): the
 *  extension's reason, and which extension suspends it. */
export function suspensionTooltip(suspension: TriggerSuspension): string {
  return `${suspension.reason}\nSuspended by the extension ${suspension.extension}.`;
}
