/**
 * What `marker/item` and `marker/detail` hand over (API 0.32, ADR-0081
 * amendment CORE-322): the Start or End marker of a Run, a Nœud structurel.
 * Re-exported by `contract.ts`.
 */

import type { DecorationProps, EntityContext, LocationContext } from "./contract";

/**
 * A Start or End marker of a Run (a Nœud structurel), as `marker/item` and
 * `marker/detail` hand it over (API 0.32): `kind: "node"`, its id, its type.
 */
export interface MarkerEntity extends EntityContext {
  kind: "node";
  type: "start" | "end";
  name: string | null;
}

/**
 * `marker/item` (a decoration under the marker's title on a Run's canvas) and
 * `marker/detail` (a section of its inspector): the marker, and where the Run
 * stands with it. `reached` is true on Start in any Run, on End once the Run
 * has reached it (it ended, or waits at End for its started nodes); the
 * context follows the Run, so a contribution reads its data again when
 * `reached` or `run.status` changes. `when: { eq: ["marker.type", "end"] }`
 * limits a contribution to End.
 */
export interface MarkerContext extends LocationContext {
  entity: MarkerEntity;
  marker: { run: string; node: string; type: "start" | "end"; reached: boolean };
}

export interface MarkerItemProps extends DecorationProps<MarkerContext> {
  entity: MarkerEntity;
}
