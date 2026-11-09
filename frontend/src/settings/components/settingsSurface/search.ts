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

/**
 * The Appearance block a row is in, when a search left its header out: the row says it in
 * its own name, or « Cost » on the Runs list and « Cost » on the Run canvas read the same
 * (CORE-232).
 */
export const HeadlessBlockContext = createContext<string | null>(null);

export function searching(query: SettingsQuery | null): query is SettingsQuery {
  return !!query && (query.needle !== "" || query.modifiedOnly);
}

export function matches(needle: string, text: string): boolean {
  return needle === "" || text.toLowerCase().includes(needle);
}

/** Whether a row, named and described by `text`, shows under `query` in a section found by its name or not. */
export function rowVisible(query: SettingsQuery | null, sectionMatch: boolean, text: string, dirty: boolean): boolean {
  if (!searching(query)) return true;
  if (query.modifiedOnly && !dirty) return false;
  return sectionMatch || matches(query.needle, text);
}

/** Whether a row, named and described by `text`, shows under the search. */
export function useRowVisible(text: string, dirty: boolean): boolean {
  const query = useContext(SearchContext);
  const sectionMatch = useContext(SectionMatchContext);
  return rowVisible(query, sectionMatch, text, dirty);
}

/**
 * The attribute a search finds on a row that is not a setting row (a table's line: a
 * tour): the search line counts it, and its section shows for it (`useFound`).
 */
export const SEARCH_HIT = "data-search-hit";
