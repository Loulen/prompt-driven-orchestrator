import { create } from "zustand";

/** A gesture the palette asks to confirm once it closed (Retry all, a Retry that resets nodes). */
export interface PaletteConfirm {
  title: string;
  consequence?: string;
  target?: string;
  actionLabel: string;
  danger?: boolean;
  run: () => void;
}

/**
 * Whether the command palette is open, and its query. The command field of the
 * top bar and Ctrl+K open it; Escape, a click outside or a chosen result close
 * it, and closing clears the query. A gesture picked in it that must be
 * confirmed waits in `confirm`.
 */
interface PaletteState {
  open: boolean;
  query: string;
  confirm: PaletteConfirm | null;
  show: () => void;
  hide: () => void;
  toggle: () => void;
  setQuery: (query: string) => void;
  ask: (confirm: PaletteConfirm) => void;
  settle: () => void;
}

export const usePalette = create<PaletteState>((set, get) => ({
  open: false,
  query: "",
  confirm: null,
  show: () => set({ open: true }),
  hide: () => set({ open: false, query: "" }),
  toggle: () => (get().open ? get().hide() : get().show()),
  setQuery: (query) => set({ query }),
  ask: (confirm) => set({ confirm }),
  settle: () => set({ confirm: null }),
}));
