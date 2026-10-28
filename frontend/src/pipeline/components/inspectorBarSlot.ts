import { createContext, useContext } from "react";

/**
 * Where the Run pane draws the node's controls: the right end of the tab bar,
 * while the Run tab is the one on screen. `null` out of the tabs: the pane
 * draws them in its own header.
 */
export interface InspectorBarSlot {
  element: HTMLElement | null;
  runActive: boolean;
}

export const BarSlot = createContext<InspectorBarSlot | null>(null);

/** The tab bar's slot for the Run pane's controls, when the pane is under the tabs. */
export function useInspectorBarSlot(): InspectorBarSlot | null {
  return useContext(BarSlot);
}

