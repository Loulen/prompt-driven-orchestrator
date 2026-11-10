/**
 * The live « Compact nodes » (CONTEXT.md « Nodes compactes »), read from the
 * Apparence of the Actor (`useAppearance`): the Settings row, the command
 * field and every open Run canvas read the same value, and a change made in
 * another tab redraws this one too.
 */
import { resetAppearance, updateAppearance, useAppearance } from "./useAppearance";

export function setCompactNodes(on: boolean): void {
  updateAppearance((appearance) => ({ ...appearance, compactNodes: on }));
}

export function toggleCompactNodes(): void {
  updateAppearance((appearance) => ({ ...appearance, compactNodes: !appearance.compactNodes }));
}

/** Drops the cached Apparence, so the next read goes back to the local copy (tests). */
export function resetCompactNodes(): void {
  resetAppearance();
}

export function useCompactNodes(): { compactNodes: boolean; setCompactNodes: (on: boolean) => void; toggleCompactNodes: () => void } {
  const { compactNodes } = useAppearance();
  return { compactNodes, setCompactNodes, toggleCompactNodes };
}
