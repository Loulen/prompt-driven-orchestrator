/**
 * What `run/item/people` hands its decorations (API 0.12), beside the
 * contexts of `contract.ts`.
 */

import type { Actor } from "../generated/Actor";
import type { Presence } from "../generated/Presence";
import type { DecorationContext, EntityContext } from "./contract";

/**
 * `run/item/people` (API 0.12): the row's Run, and the Actors who have it open
 * (its Presences, by order of arrival), live. The core draws no person there.
 */
export interface RunPeopleContext extends DecorationContext {
  entity: RunPeopleEntity;
  presences: Presence[];
}

/** The Run of `run/item/people`: who created it, `null` when unknown. */
export interface RunPeopleEntity extends EntityContext {
  kind: "run";
  created_by: Actor | null;
}
