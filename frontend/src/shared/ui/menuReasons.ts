/**
 * The reasons of a menu's greyed entries (CORE-205, option B): each sentence
 * once, on a line at the menu's foot (`RowMenu`, the kit's `Menu`).
 */
import { createContext } from "react";

/**
 * How the entries a menu does not render itself (the extensions' groups)
 * hand it the reasons of their greyed entries, so the menu says each once, at
 * its foot (CORE-205). Absent outside a `RowMenu`: those entries say theirs.
 */
export const MenuReasonsContext = createContext<((source: string, reasons: readonly string[]) => void) | null>(null);

/** The reasons of `refusals`, each once, in order: what a `RowMenu` says at its foot. */
export function reasonsOf(...refusals: (string | undefined | false | null)[]): string[] {
  return [...new Set(refusals.filter((r): r is string => typeof r === "string" && r.length > 0))];
}
