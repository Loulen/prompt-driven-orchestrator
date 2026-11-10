import { createContext } from "react";
import type { Icon } from "./kit";

/** One tab of the left panel: Runs, Triggers, Pipelines. */
export interface LeftTab {
  id: string;
  label: string;
  icon: Icon;
  /** Items ticked in that tab: a dot on its icon while another tab shows. */
  selected: number;
}

interface LeftTabsValue {
  tabs: LeftTab[];
  active: string;
  onChange: (id: string) => void;
}

export const LeftTabsContext = createContext<LeftTabsValue | null>(null);

/** Hands the left panel's tabs to the header of the pane it shows. */
export const LeftTabsProvider = LeftTabsContext.Provider;
