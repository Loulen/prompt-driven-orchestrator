/**
 * The search line of the Settings surface: what it looks for (a few words, or only the
 * changed settings), and whether a row or a section shows under it. Outside a search,
 * everything shows. And what a category page tells its sections.
 */

import { createContext, useContext } from "react";

export interface SettingsQuery {
  /** Lower-cased, trimmed; `""` when only the modified filter is on. */
  needle: string;
  modifiedOnly: boolean;
}

/** `null`: no search, each category shows its page. */
export const SearchContext = createContext<SettingsQuery | null>(null);

/** What a category page tells its sections: which are dirty, and its own name (search). */
export const PageContext = createContext<{ dirty: ReadonlySet<string>; category: string }>({
  dirty: new Set(),
  category: "",
});

/** The section a row is in found the needle by its own name: its rows all show. */
export const SectionMatchContext = createContext(false);

export function searching(query: SettingsQuery | null): query is SettingsQuery {
  return !!query && (query.needle !== "" || query.modifiedOnly);
}

export function matches(needle: string, text: string): boolean {
  return needle === "" || text.toLowerCase().includes(needle);
}

/** Whether a row, named and described by `text`, shows under the search. */
export function useRowVisible(text: string, dirty: boolean): boolean {
  const query = useContext(SearchContext);
  const sectionMatch = useContext(SectionMatchContext);
  if (!searching(query)) return true;
  if (query.modifiedOnly && !dirty) return false;
  return sectionMatch || matches(query.needle, text);
}
