import { create } from "zustand";

/**
 * How many enlarged node terminals are open (#968).
 *
 * The « Terminal agrandi » is the terminal's own wrapper switched to
 * `position: fixed` — it stays a DOM descendant of the app's resizable panel
 * groups (a portal would remount the terminal). `react-resizable-panels` hit-tests
 * its separators against every pointer press inside a group, overlay or not, so
 * a drag across a hidden separator — a text selection in the terminal, a press
 * on the backdrop — resized the panels underneath. The groups that contain the
 * overlay read this and turn resizing off while it is up.
 *
 * A count, not a flag: the Run inspector keeps a second (hidden) panel mounted.
 */
interface TerminalOverlayState {
  openCount: number;
  opened: () => void;
  closed: () => void;
}

export const useTerminalOverlayStore = create<TerminalOverlayState>((set) => ({
  openCount: 0,
  opened: () => set((s) => ({ openCount: s.openCount + 1 })),
  closed: () => set((s) => ({ openCount: Math.max(0, s.openCount - 1) })),
}));

/** Is any enlarged terminal over the app right now? */
export function useTerminalOverlayOpen(): boolean {
  return useTerminalOverlayStore((s) => s.openCount > 0);
}
