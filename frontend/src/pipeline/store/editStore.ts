/**
 * The editor's store: one Zustand store, assembled from its slices.
 */

import { create } from "zustand";

import type { EditState } from "./editState";
import { graphSlice } from "./graphSlice";
import { runEditSlice } from "./runEditSlice";
import { saveSlice } from "./saveSlice";
import { tabsSlice } from "./tabsSlice";
import { versionsSlice } from "./versionsSlice";

export const useEditStore = create<EditState>(
  (set, get) =>
    ({
      ...tabsSlice(set, get),
      ...graphSlice(set, get),
      ...saveSlice(set, get),
      ...runEditSlice(set, get),
      ...versionsSlice(set, get),
    }) as EditState,
);
