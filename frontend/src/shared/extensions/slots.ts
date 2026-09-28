/**
 * The UI slots (Emplacements d'UI, CONTEXT.md § Extensions): what the loaded
 * extensions put in one slot, each element with the component its bundle
 * exports. An element whose component the bundle does not export is left out.
 */

import { useMemo, type ComponentType } from "react";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { ExtensionSlot } from "../generated/ExtensionSlot";
import type { SlotKind } from "../generated/SlotKind";
import type { Attributes } from "./attributes";
import { useExtensions } from "./store";

/** What every extension component receives. */
export interface SlotProps {
  extension: { id: string; name: string };
}

/** A list filter's component. */
export interface ListFilterProps extends SlotProps {
  resource: string;
  /** The attribute filter in force: prefixed key → value. */
  filter: Attributes;
  setFilter: (next: Attributes) => void;
}

/** A detail tab's component: the entity it details. */
export interface DetailTabProps extends SlotProps {
  entity: { kind: string; id: string; attributes: Attributes };
}

export interface SlotElement<P> {
  /** Unique across extensions: `<extension id>:<slot id>`. */
  key: string;
  extension: ExtensionEntry;
  slot: ExtensionSlot;
  Component: ComponentType<P>;
}

export function useSlot<P extends SlotProps = SlotProps>(
  kind: SlotKind,
  resource?: string,
): SlotElement<P>[] {
  const extensions = useExtensions();
  return useMemo(
    () =>
      extensions.flatMap(({ entry, status, module }) => {
        if (status !== "ready" || !module) return [];
        return (entry.ui?.slots ?? []).flatMap((slot) => {
          if (slot.slot !== kind || (resource !== undefined && slot.resource !== resource)) {
            return [];
          }
          const Component = module[slot.component];
          if (typeof Component !== "function") return [];
          return [
            {
              key: `${entry.id}:${slot.id}`,
              extension: entry,
              slot,
              Component: Component as ComponentType<P>,
            },
          ];
        });
      }),
    [extensions, kind, resource],
  );
}

/** The settings-panel slot's elements; none without a contribution. */
export function useSettingsPanels(): SlotElement<SlotProps>[] {
  return useSlot("settings_panel");
}

/** The detail-tab slot's elements for the detail of `resource`. */
export function useDetailTabs(resource: string): SlotElement<DetailTabProps>[] {
  return useSlot<DetailTabProps>("detail_tab", resource);
}
