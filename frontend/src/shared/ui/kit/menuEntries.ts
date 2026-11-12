/** What the Menu of the shared design system reads off its entries. */
import type { MenuEntry, MenuItem } from "./menu";

export const isItem = (e: MenuEntry): e is MenuItem => !e.kind || e.kind === "item";

/** The reasons of the greyed entries of `entries`, each once, in order. */
export function menuReasons(entries: readonly MenuEntry[]): string[] {
  const reasons: string[] = [];
  for (const e of entries) {
    if (isItem(e) && e.disabledReason && !reasons.includes(e.disabledReason)) reasons.push(e.disabledReason);
  }
  return reasons;
}
