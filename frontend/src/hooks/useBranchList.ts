import { useCallback, useRef, useState } from "react";
import { fetchRemotes, listBranches } from "../api";
import type { BranchFetchError, BranchList, BranchRef } from "../types";

/**
 * One repo's branches and the freshness of the numbers on them (#802, ADR-0070).
 *
 * Shared by the primary field (`useLaunchTargets`) and every secondary row
 * (`SecondaryRepoRow`), for the reason #571 spelled out when `pickDefaultBranch`
 * was extracted: two copies of branch logic drift, and the drift only shows up
 * when someone launches from the wrong ref. The race rules below are subtle
 * enough that owning them twice would be a promise to get one of them wrong.
 *
 * Two rules the call sites must not have to remember:
 *
 * 1. **The fetch never blocks.** `load` resolves as soon as the LIST is in; the
 *    fetch lands later, in place. The caller (`useRepoValidation`) awaits `load`
 *    to keep a repo "validating" until its branches show — awaiting the network
 *    there would hold that spinner for the fetch's full 20 s budget.
 * 2. **A fetch result outranks a plain list result for the same repo.** The two
 *    are started together and answer in either order; the fetch returns the same
 *    list *refreshed*, so letting a late `listBranches` land on top would put the
 *    pre-fetch écart back on screen — the exact staleness this feature removes.
 */
export function useBranchList() {
  const [branches, setBranches] = useState<BranchRef[]>([]);
  const [loading, setLoading] = useState(false);
  const [lastFetchAt, setLastFetchAt] = useState<string | null>(null);
  const [fetchError, setFetchError] = useState<BranchFetchError | null>(null);
  const [fetching, setFetching] = useState(false);

  /**
   * Which repo the answers on screen belong to. A fetch takes seconds and the
   * user can change repo meanwhile, so every async result is checked against
   * this before it lands: a late answer about the PREVIOUS repo overwriting the
   * current one is the multi-repo shape of the #454 stale-branch bug.
   */
  const currentRepo = useRef("");
  /** Whether a fetch has already landed for `currentRepo` — see rule 2 above. */
  const fetchLanded = useRef(false);

  const clear = useCallback(() => {
    currentRepo.current = "";
    fetchLanded.current = false;
    setBranches([]);
    setLastFetchAt(null);
    setFetchError(null);
    setFetching(false);
  }, []);

  const apply = useCallback((repoPath: string, list: BranchList, fromFetch: boolean) => {
    if (currentRepo.current !== repoPath) return false;
    if (!fromFetch && fetchLanded.current) return false;
    setBranches(list.branches);
    setLastFetchAt(list.last_fetch_at);
    // Only a FETCH may write the fetch verdict. `GET /repos/branches` makes no
    // attempt, so its unfailing `fetch_error: null` is not a success — and letting
    // it land would erase a real failure whenever the plain list answered second,
    // turning "gap unknown" back into numbers presented as current.
    if (fromFetch) {
      fetchLanded.current = true;
      setFetchError(list.fetch_error);
    }
    return true;
  }, []);

  /**
   * Fetch this repo's remotes and refresh the list in place. Never blocking,
   * never destructive: a failure only sets `fetchError`, so the list, the
   * selection and Launch all stay exactly as they were (ADR-0070 §1).
   */
  const sync = useCallback(
    async (repoPath: string) => {
      setFetching(true);
      try {
        apply(repoPath, await fetchRemotes(repoPath), true);
      } catch (e) {
        // The request itself failed (daemon down, repo refused). Same contract as
        // a named fetch failure: say the écart is unknown, change nothing else.
        if (currentRepo.current === repoPath) {
          setFetchError({
            kind: "failed",
            message: e instanceof Error ? e.message : String(e),
          });
        }
      } finally {
        if (currentRepo.current === repoPath) setFetching(false);
      }
    },
    [apply],
  );

  /**
   * Load `repoPath`'s branches AND fetch its remotes, in parallel. Resolves with
   * the loaded list (or `null` if the load failed or was superseded) so the
   * caller can seed its selection from it.
   */
  const load = useCallback(
    async (repoPath: string): Promise<BranchRef[] | null> => {
      currentRepo.current = repoPath;
      fetchLanded.current = false;
      setLoading(true);
      setFetchError(null);
      // Fired, not awaited: the list is what the user needs now; the refreshed
      // numbers land a beat later, in place.
      void sync(repoPath);
      try {
        const list = await listBranches(repoPath);
        if (currentRepo.current !== repoPath) return null;
        apply(repoPath, list, false);
        return list.branches;
      } catch {
        if (currentRepo.current === repoPath) setBranches([]);
        return null;
      } finally {
        if (currentRepo.current === repoPath) setLoading(false);
      }
    },
    [sync, apply],
  );

  /** The sync popover's explicit refetch, on the repo currently displayed. */
  const refetch = useCallback(() => {
    if (currentRepo.current) void sync(currentRepo.current);
  }, [sync]);

  return { branches, loading, lastFetchAt, fetchError, fetching, load, refetch, clear };
}
