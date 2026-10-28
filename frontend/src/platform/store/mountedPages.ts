/**
 * The mounted pages open in the center panel: one tab per page, beside the Run and
 * pipeline tabs of the tab bar, at most one of them shown at a time. A page tab shown
 * hides the canvas (kept mounted underneath) and hands the right pane to its reading
 * guide; selecting a Run or a pipeline tab gives the center back to the canvas.
 *
 * The guide's folded state is remembered on this browser.
 */

import { create } from "zustand";
import { moveItem } from "../../lib/moveItem";
import type { PageMount } from "../../shared/generated/PageMount";

export const GUIDE_OPEN_KEY = "pdo.pages.guide";

function readGuideOpen(): boolean {
  try {
    return localStorage.getItem(GUIDE_OPEN_KEY) !== "closed";
  } catch {
    return true;
  }
}

function writeGuideOpen(open: boolean): void {
  try {
    localStorage.setItem(GUIDE_OPEN_KEY, open ? "open" : "closed");
  } catch {
    // Private mode / quota: the fold still works for the session.
  }
}

interface MountedPagesState {
  /** The open page tabs, in the order they opened, by mount name. */
  tabs: PageMount[];
  /** The page tab shown in the center, or `null` for the canvas. */
  active: string | null;
  /** Whether the guide shows in the right pane, or folds into its rail. */
  guideOpen: boolean;
  /** The last place in a page a guide entry asked its frame to show (`seq` tells two
   *  clicks on the same entry apart). */
  frameRequest: { name: string; link: string; seq: number } | null;
  /** Ask the frame of the page `name` to show `link` (an anchor, a relative path). */
  goInPage: (name: string, link: string) => void;
  /** Open the tab of `mount` (or refresh it) and show it. */
  openPage: (mount: PageMount) => void;
  /** Show the tab named `name`, or the canvas for `null`. */
  showPage: (name: string | null) => void;
  /** Close the tab named `name`; the canvas comes back when it was shown. */
  closePage: (name: string) => void;
  /** Move the tab named `name` to `index` among the page tabs (a drag of the tab). */
  movePage: (name: string, index: number) => void;
  setGuideOpen: (open: boolean) => void;
  /** Forget every tab (tests). */
  reset: () => void;
}

export const useMountedPages = create<MountedPagesState>((set) => ({
  tabs: [],
  active: null,
  guideOpen: readGuideOpen(),
  frameRequest: null,
  goInPage: (name, link) =>
    set((state) => ({ frameRequest: { name, link, seq: (state.frameRequest?.seq ?? 0) + 1 } })),
  openPage: (mount) =>
    set((state) => {
      const at = state.tabs.findIndex((tab) => tab.name === mount.name);
      const tabs = at < 0 ? [...state.tabs, mount] : state.tabs.map((tab, i) => (i === at ? mount : tab));
      return { tabs, active: mount.name };
    }),
  showPage: (name) => set({ active: name }),
  closePage: (name) =>
    set((state) => ({
      tabs: state.tabs.filter((tab) => tab.name !== name),
      active: state.active === name ? null : state.active,
    })),
  movePage: (name, index) =>
    set((state) => ({ tabs: moveItem(state.tabs, state.tabs.findIndex((tab) => tab.name === name), index) })),
  setGuideOpen: (open) => {
    writeGuideOpen(open);
    set({ guideOpen: open });
  },
  reset: () => set({ tabs: [], active: null, guideOpen: readGuideOpen(), frameRequest: null }),
}));

/** The page tab shown in the center, if any. */
export function useActivePage(): PageMount | null {
  return useMountedPages((state) => state.tabs.find((tab) => tab.name === state.active) ?? null);
}
