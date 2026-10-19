import { create } from "zustand";
import { loadNodeDurationMode, saveNodeDurationMode } from "../../lib/uiPrefs";
import type { NodeDurationMode } from "../../lib/uiPrefs";

/**
 * The Mode de durée on screen (`total` or `active`): one global choice, read
 * once from this browser, saved at each toggle. Shared by the node panel's
 * meta line and every menu of iterations, so a click on the duration switches
 * them all at once.
 */
interface NodeDurationModeState {
  mode: NodeDurationMode;
  toggle: () => void;
}

export const useNodeDurationMode = create<NodeDurationModeState>((set, get) => ({
  mode: loadNodeDurationMode(),
  toggle: () => {
    const next: NodeDurationMode = get().mode === "total" ? "active" : "total";
    saveNodeDurationMode(next);
    set({ mode: next });
  },
}));
