import type { Actor } from "../../shared/generated/Actor";

/**
 * The Actor that created a Run (`created_by`, persisted since the extension
 * API 0.8), `null` on a Run created before, or by a daemon that does not
 * record it.
 */
export function runCreator(run: object): Actor | null {
  const creator = (run as { created_by?: Actor | null }).created_by;
  return creator ?? null;
}
