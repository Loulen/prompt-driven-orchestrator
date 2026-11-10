import { create } from "zustand";
import { loadTranscriptOrder, saveTranscriptOrder } from "../../lib/uiPrefs";
import type { TranscriptOrder } from "../../lib/uiPrefs";

/**
 * The Transcript order on screen (newest or oldest first): one global choice,
 * read once from this browser, saved at each toggle, so every node and every
 * Run opens in the order the user chose.
 */
interface TranscriptOrderState {
  order: TranscriptOrder;
  toggle: () => void;
}

export const useTranscriptOrder = create<TranscriptOrderState>((set, get) => ({
  order: loadTranscriptOrder(),
  toggle: () => {
    const next: TranscriptOrder = get().order === "newest-first" ? "oldest-first" : "newest-first";
    saveTranscriptOrder(next);
    set({ order: next });
  },
}));
