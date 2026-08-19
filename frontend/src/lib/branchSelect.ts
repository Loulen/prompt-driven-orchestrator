import type { BranchRef } from "../types";

/**
 * The branch a fresh repo seeds its source/base select with (#454, #571).
 *
 * Never a remote while any local exists: `main` local → `master` local → first
 * local → (no local at all) a remote ending in `/main` → `/master` → first
 * remote. Locality-aware so a repo whose default local is `master` can never
 * seed itself on `origin/main` — the class of bug the #454 rule was written to
 * kill. Returns `undefined` for an empty list.
 *
 * Shared by the primary select (`useLaunchTargets`) and every secondary row
 * (`SecondaryRepoRow`) so the two can never drift — this duplication was the
 * documented trap of #571.
 */
export function pickDefaultBranch(list: BranchRef[]): string | undefined {
  const locals = list.filter((b) => b.kind === "local");
  const local =
    locals.find((b) => b.name === "main") ??
    locals.find((b) => b.name === "master") ??
    locals[0];
  if (local) return local.name;
  const remotes = list.filter((b) => b.kind === "remote");
  const remote =
    remotes.find((b) => b.name.endsWith("/main")) ??
    remotes.find((b) => b.name.endsWith("/master")) ??
    remotes[0];
  return remote?.name;
}
