import { useMemo } from "react";
import { contributionElementId, type Surface } from "../../lib/appearance";
import type { UiLocation } from "../generated/UiLocation";
import { elementsAt } from "./catalogue";
import { useExtensions, type LoadedExtension } from "./store";

/** An extension's contribution a switch of Settings › Appearance hides. */
export interface HideableContribution {
  /** Its element id, `<surface>.ext.<extension id>:<contribution id>`. */
  id: string;
  /** « Extension name · label ». */
  label: string;
  location: UiLocation;
}

/**
 * The contributions the loaded extensions draw at `locations`, one Élément
 * masquable of `surface` each, in the catalogue's order. Their `when` is not
 * evaluated: a switch exists whether or not the contribution marks a given entity.
 */
export function hideableContributions(
  extensions: LoadedExtension[],
  surface: Surface,
  locations: readonly UiLocation[],
): HideableContribution[] {
  return locations.flatMap((location) =>
    elementsAt(extensions, location)
      .filter((element) => element.Component !== null)
      .map((element) => ({
        id: contributionElementId(surface, element.key),
        label: `${element.extension.name} · ${element.contribution.label}`,
        location,
      })),
  );
}

/** {@link hideableContributions} of the loaded extensions. */
export function useHideableContributions(surface: Surface, locations: readonly UiLocation[]): HideableContribution[] {
  const extensions = useExtensions();
  return useMemo(() => hideableContributions(extensions, surface, locations), [extensions, surface, locations]);
}
