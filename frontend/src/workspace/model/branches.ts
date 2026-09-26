/**
 * The client-side outcome of a fast-forward, rebuilt from the 200/409 bodies.
 */

import type { BranchList } from "../../shared/generated/BranchList";
import type { FastForwardRefusal } from "../../shared/generated/FastForwardRefusal";
import type { FastForwardResult } from "../../shared/generated/FastForwardResult";

/**
 * The outcome of `POST /repos/fast-forward` (#803). Both arms carry the REFRESHED
 * branch list, so a refusal is as good a reason to update the numbers on screen as
 * a success — which is what lets the two benign races self-heal.
 */
export type FastForwardOutcome =
  | { kind: "done"; result: FastForwardResult; list: BranchList }
  | { kind: "refused"; refusal: FastForwardRefusal; list: BranchList | null }
  /** The call itself failed (daemon down). Nothing moved, nothing is known. */
  | { kind: "error"; message: string };
