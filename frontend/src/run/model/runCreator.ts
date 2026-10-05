import type { Actor } from "../../shared/generated/Actor";
import type { RunListEntry } from "../../shared/generated/RunListEntry";

/** The Actor that created a Run (0.8), `null` on a Run created before. */
export function runCreator(run: Pick<RunListEntry, "created_by">): Actor | null {
  return run.created_by ?? null;
}
