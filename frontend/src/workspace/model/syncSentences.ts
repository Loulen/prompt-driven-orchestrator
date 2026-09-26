/** The plain sentences the sync popover and the fast-forward card say (#802, #803). */

import { shortAge } from "./branchSelect";

/** `~/…/prompt-driven-orchestrator` — a path the popover can fit on one line. */
export function shortPath(path: string, keep = 2): string {
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= keep) return path;
  return `…/${parts.slice(-keep).join("/")}`;
}

/** "Fetched 12 s ago." / "Never fetched." */
export function fetchedSentence(lastFetchAt: string | null): string {
  const age = shortAge(lastFetchAt);
  return age ? `Fetched ${age} ago.` : "Never fetched.";
}
