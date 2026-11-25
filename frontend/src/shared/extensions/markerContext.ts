/**
 * The Start and End markers of a Run (Nœuds structurels, ADR-0081 amendment
 * CORE-322): what `marker/item` and `marker/detail` hand over, and which
 * markers of a Run's canvas carry a decoration row.
 *
 * The row is reserved as soon as one contribution applies, before anything is
 * rendered: the canvas lays the decorated marker out at its fixed height
 * (`pipeline/model/markerCanvas.ts`) from {@link decoratedMarkers}, and the
 * marker draws the row from the same contributions (`ExtensionDecorations`).
 */

import { useMemo } from "react";
import { useAppearance } from "../../hooks/useAppearance";
import { contributionElementId, isVisible, type Appearance } from "../../lib/appearance";
import { elementsAt } from "./catalogue";
import { useBaseContext, type BaseContext } from "./context";
import type { MarkerContext, MarkerEntity } from "./contract";
import { useExtensions, type LoadedExtension } from "./store";
import { evaluateWhen } from "./when";

export type MarkerType = "start" | "end";

/** A marker of a Run, as the canvas or its inspector knows it. */
export interface MarkerOfRun {
  run: string;
  node: string;
  name: string | null;
  type: MarkerType;
  /** Start: always in a Run. End: the Run has reached it (`markerReached`). */
  reached: boolean;
}

/** What `marker/item` and `marker/detail` add to the base context. */
export type MarkerExtra = Omit<MarkerContext, keyof BaseContext | "location">;

export function markerExtra({ run, node, name, type, reached }: MarkerOfRun): MarkerExtra {
  const entity: MarkerEntity = { kind: "node", id: node, type, name, attributes: {} };
  return { entity, marker: { run, node, type, reached } };
}

/** {@link markerExtra}, memoised on its fields. */
export function useMarkerExtra({ run, node, name, type, reached }: MarkerOfRun): MarkerExtra {
  return useMemo(() => markerExtra({ run, node, name, type, reached }), [run, node, name, type, reached]);
}

/**
 * Whether the marker carries a `marker/item` row: one contribution whose
 * `when` holds on its context and that the Apparence shows (Run canvas). The
 * component may still render nothing: the row stays reserved.
 */
export function isMarkerDecorated(
  extensions: LoadedExtension[],
  base: BaseContext,
  appearance: Appearance,
  marker: MarkerOfRun,
): boolean {
  const context = { ...base, location: "marker/item", ...markerExtra(marker) };
  return elementsAt(extensions, "marker/item").some(
    (element) =>
      element.Component !== null &&
      evaluateWhen(element.contribution.when, context) &&
      isVisible(appearance, contributionElementId("run.canvas", element.key)),
  );
}

/**
 * The ids of the markers that carry a `marker/item` row, as one string (ids
 * joined), so a new list with the same markers changes nothing downstream.
 */
export function useDecoratedMarkers(markers: readonly MarkerOfRun[]): string {
  const extensions = useExtensions();
  const base = useBaseContext();
  const appearance = useAppearance();
  return useMemo(
    () =>
      markers
        .filter((marker) => isMarkerDecorated(extensions, base, appearance, marker))
        .map((marker) => marker.node)
        .join("\n"),
    [extensions, base, appearance, markers],
  );
}
