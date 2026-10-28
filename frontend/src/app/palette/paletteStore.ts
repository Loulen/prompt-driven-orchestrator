import { create } from "zustand";

/**
 * Whether the command palette is open, and its query. The command field of the
 * top bar and Ctrl+K open it; Escape, a click outside or a chosen result close
 * it, and closing clears the query.
 */
interface PaletteState {
  open: boolean;
  query: string;
  show: () => void;
  hide: () => void;
  toggle: () => void;
  setQuery: (query: string) => void;
}

export const usePalette = create<PaletteState>((set, get) => ({
  open: false,
  query: "",
  show: () => set({ open: true }),
  hide: () => set({ open: false, query: "" }),
  toggle: () => (get().open ? get().hide() : get().show()),
  setQuery: (query) => set({ query }),
}));
