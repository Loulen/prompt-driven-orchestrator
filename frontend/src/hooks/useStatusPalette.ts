/**
 * The live Palette daltonienne (CONTEXT.md), read from the Apparence of the
 * Actor (`useAppearance`): the Settings row and every tab of this browser read
 * the same value, painted on `<html>` by the Apparence store.
 */
import { updateAppearance, useAppearance } from "./useAppearance";

export function setColourBlindStatus(on: boolean): void {
  updateAppearance((appearance) => ({ ...appearance, colourBlindStatus: on }));
}

export function useStatusPalette(): { colourBlindStatus: boolean; setColourBlindStatus: (on: boolean) => void } {
  const { colourBlindStatus } = useAppearance();
  return { colourBlindStatus, setColourBlindStatus };
}
