import { create } from "zustand";
import type { Point } from "../lib/orthogonalRouter";
import {
  loadWiringGridFeedback,
  saveWiringGridFeedback,
  type WiringGridFeedback,
} from "../lib/uiPrefs";

/**
 * Transient canvas state of the wiring gesture (#844) — deliberately NOT in
 * `editStore`.
 *
 * `editStore` owns the pipeline document and an undo history: everything written
 * there is a document edit a user can undo. None of this is. « The grid is
 * currently originated here », « a wire is being drawn right now » and « show the
 * lattice as dots » are properties of the canvas in front of the reader, not of
 * the workflow being authored, and dropping them into the history would make
 * Ctrl-Z step through the pointer's past.
 */
interface WiringState {
  /** True between `onConnectStart` and `onConnectEnd` — the wiring grid shows
   *  only while a wire is actually being drawn. */
  wiring: boolean;
  /**
   * The point the wiring lattice passes through. Seeded on the departure point of
   * each gesture and moved by a Shift release (ADR-0072: releasing Shift
   * re-origins the grid on the current tip), so the cells that follow line up
   * with the freehand run just drawn instead of with where the gesture started.
   */
  origin: Point;
  /** How the lattice shows itself while wiring. Mirrors the per-browser
   *  preference; written through so a change applies without a reload. */
  gridFeedback: WiringGridFeedback;
  setWiring: (wiring: boolean) => void;
  setOrigin: (origin: Point) => void;
  setGridFeedback: (gridFeedback: WiringGridFeedback) => void;
}

export const useWiringStore = create<WiringState>((set) => ({
  wiring: false,
  origin: { x: 0, y: 0 },
  gridFeedback: loadWiringGridFeedback(),
  setWiring: (wiring) => set({ wiring }),
  setOrigin: (origin) => set({ origin }),
  setGridFeedback: (gridFeedback) => {
    saveWiringGridFeedback(gridFeedback);
    set({ gridFeedback });
  },
}));
