import { useCallback, useRef, useState } from "react";
import { fastForwardBranch, fetchRemotes, listBranches } from "../api";
import type { BranchFetchError, BranchList, BranchRef, FastForwardOutcome } from "../types";

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

  /**
   * Which verb an incoming list came from. Three, because they differ on the two
   * questions that decide whether a late answer may overwrite the screen:
   *
   * - `list` — `GET /repos/branches`, may be superseded (see rule 2 of the header);
   * - `fetch` — the only verb that may write the fetch VERDICT;
   * - `fast-forward` — read after the move, so it outranks a plain list in the air,
   *   but it made no remote attempt, so its unfailing `fetch_error: null` says
   *   nothing about whether the last fetch worked and must not erase one.
   */
  type ListSource = "list" | "fetch" | "fast-forward";

  const apply = useCallback((repoPath: string, list: BranchList, from: ListSource) => {
    if (currentRepo.current !== repoPath) return false;
    if (from === "list" && fetchLanded.current) return false;
    setBranches(list.branches);
    setLastFetchAt(list.last_fetch_at);
    // Only a FETCH may write the fetch verdict. The other two make no attempt, so
    // their unfailing `fetch_error: null` is not a success — and letting one land
    // would erase a real failure whenever it answered second, turning "gap unknown"
    // back into numbers presented as current.
    if (from === "fetch") {
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
        apply(repoPath, await fetchRemotes(repoPath), "fetch");
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
        apply(repoPath, list, "list");
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

  /**
   * Fast-forward a local branch of the repo currently displayed (#803, ADR-0070 §2).
   *
   * Three rules the popover does not have to remember:
   *
   * 1. **One in flight per repo.** A second click while the first POST is out is
   *    dropped rather than queued: the daemon would answer the second with
   *    `up_to_date`, and a refusal for a click that did exactly what was asked is a
   *    lie about the repository.
   * 2. **The refreshed list lands from BOTH arms.** The 200 and the 409 carry the
   *    same shape, and a refusal is as good a reason to update the numbers as a
   *    success — it is what lets the `up_to_date`/`no_upstream` races self-heal into
   *    the matching #802 state instead of an error card.
   * 3. **A late answer about the PREVIOUS repo is dropped**, like every other
   *    async result here (`apply` owns that check).
   *
   * The refreshed list lands as `fast-forward`, not `fetch`: it was read AFTER the
   * move, so it outranks any plain list still in the air (rule 2 of this hook's
   * header), but it made no remote attempt and so may not speak for the fetch.
   */
  const ffInFlight = useRef(false);
  const fastForward = useCallback(
    async (branch: string): Promise<FastForwardOutcome | null> => {
      const repoPath = currentRepo.current;
      if (!repoPath || ffInFlight.current) return null;
      ffInFlight.current = true;
      try {
        const outcome = await fastForwardBranch(repoPath, branch);
        if (outcome.kind === "done") apply(repoPath, outcome.list, "fast-forward");
        else if (outcome.kind === "refused" && outcome.list) {
          apply(repoPath, outcome.list, "fast-forward");
        }
        return outcome;
      } finally {
        ffInFlight.current = false;
      }
    },
    [apply],
  );

  return {
    branches,
    loading,
    lastFetchAt,
    fetchError,
    fetching,
    load,
    refetch,
    fastForward,
    clear,
  };
}
