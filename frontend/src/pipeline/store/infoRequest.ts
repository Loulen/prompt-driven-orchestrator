import { create } from "zustand";
import type { TabId } from "../../lib/infoPanelReconcile";

/**
 * A surface that is not the canvas toolbar asks for Pipeline info on a given
 * tab (the line under the tab bar opens Versions). The App's info panel takes
 * the request and clears it.
 */
interface InfoRequestState {
  request: { tab: TabId; seq: number } | null;
  open: (tab: TabId) => void;
  clear: () => void;
}

export const useInfoRequest = create<InfoRequestState>((set, get) => ({
  request: null,
  open: (tab) => set({ request: { tab, seq: (get().request?.seq ?? 0) + 1 } }),
  clear: () => set({ request: null }),
}));

/** Opens Pipeline info on `tab`. */
export function requestInfoTab(tab: TabId): void {
  useInfoRequest.getState().open(tab);
}
