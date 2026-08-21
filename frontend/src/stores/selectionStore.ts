import { create } from "zustand";

/**
 * Multi-select for the three left-panel lists (#577). Each tab (Runs, Triggers,
 * Library) owns an INDEPENDENT selection set: the three entity types have
 * different valid bulk actions, so a selection never spans tabs. The sets
 * PERSIST across a tab switch — that is what leaves the small count badge on the
 * tab you left, so an in-flight selection is never silently lost (chosen design
 * "D"). Only the active tab surfaces the floating action bar.
 *
 * State is kept as ordered `string[]` (not `Set`) so a mutation yields a fresh
 * array reference zustand can diff — components derive a `Set` for O(1) row
 * lookups. The store is deliberately view-agnostic: range-select and
 * select-visible take the caller's *currently-visible order*, so the store never
 * needs to know how a list is grouped, filtered or sorted.
 */
export type SelectTab = "runs" | "triggers" | "library";

interface SelectionState {
  runs: string[];
  triggers: string[];
  library: string[];
  /** Per-tab range anchor: the last id a plain click set, from which a
   *  shift-click extends a contiguous range. Null until the first click. */
  anchor: Record<SelectTab, string | null>;

  /** Plain click on a row's select control: flip membership, set the anchor. */
  toggle: (tab: SelectTab, id: string) => void;
  /** Shift-click: union the contiguous range [anchor..id] within `ordered`
   *  (the visible order). No/absent anchor ⇒ behaves like `toggle`-select. */
  selectRange: (tab: SelectTab, id: string, ordered: string[]) => void;
  /** Ctrl/Cmd-A: union every currently-visible id (select-all-visible). */
  selectVisible: (tab: SelectTab, ids: string[]) => void;
  /** Group-header "select all in this repo": toggle the whole group — union
   *  when any is unselected, clear them when all are already selected. */
  selectGroup: (tab: SelectTab, ids: string[]) => void;
  /** Drop specific ids (e.g. the ones a bulk action just consumed). */
  deselect: (tab: SelectTab, ids: string[]) => void;
  /** Esc / the bar's Clear: empty one tab's selection. */
  clear: (tab: SelectTab) => void;
  clearAll: () => void;
}

function withToggled(list: string[], id: string): string[] {
  return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
}

function withUnion(list: string[], ids: string[]): string[] {
  const have = new Set(list);
  const add = ids.filter((id) => !have.has(id));
  return add.length === 0 ? list : [...list, ...add];
}

function withoutIds(list: string[], ids: string[]): string[] {
  const drop = new Set(ids);
  const next = list.filter((id) => !drop.has(id));
  return next.length === list.length ? list : next;
}

export const useSelectionStore = create<SelectionState>((set) => ({
  runs: [],
  triggers: [],
  library: [],
  anchor: { runs: null, triggers: null, library: null },

  toggle: (tab, id) =>
    set((s) => ({
      [tab]: withToggled(s[tab], id),
      anchor: { ...s.anchor, [tab]: id },
    })),

  selectRange: (tab, id, ordered) =>
    set((s) => {
      const anchor = s.anchor[tab];
      const from = anchor === null ? -1 : ordered.indexOf(anchor);
      const to = ordered.indexOf(id);
      // No usable anchor (first click, or anchor scrolled out of the visible
      // order) ⇒ fall back to a plain select and seed the anchor here.
      if (from < 0 || to < 0) {
        return { [tab]: withUnion(s[tab], [id]), anchor: { ...s.anchor, [tab]: id } };
      }
      const [lo, hi] = from <= to ? [from, to] : [to, from];
      const range = ordered.slice(lo, hi + 1);
      // Anchor stays put — repeated shift-clicks extend from the same origin.
      return { [tab]: withUnion(s[tab], range) };
    }),

  selectVisible: (tab, ids) =>
    set((s) => ({
      [tab]: withUnion(s[tab], ids),
      anchor: { ...s.anchor, [tab]: ids.length > 0 ? ids[ids.length - 1] : s.anchor[tab] },
    })),

  selectGroup: (tab, ids) =>
    set((s) => {
      if (ids.length === 0) return {};
      const have = new Set(s[tab]);
      const allSelected = ids.every((id) => have.has(id));
      return { [tab]: allSelected ? withoutIds(s[tab], ids) : withUnion(s[tab], ids) };
    }),

  deselect: (tab, ids) => set((s) => ({ [tab]: withoutIds(s[tab], ids) })),

  clear: (tab) => set((s) => ({ [tab]: [], anchor: { ...s.anchor, [tab]: null } })),

  clearAll: () =>
    set({ runs: [], triggers: [], library: [], anchor: { runs: null, triggers: null, library: null } }),
}));
