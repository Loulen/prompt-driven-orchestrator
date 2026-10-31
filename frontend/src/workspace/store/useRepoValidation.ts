import { useCallback, useEffect, useRef, useState } from "react";
import { validateRepo } from "../api/repos";
import { isRemoteUrl } from "../model/clone";

/**
 * The target-repo field and its verdict (#359), lifted out of `NewRunModal.tsx`.
 *
 * Typing debounces a `GET /repos/validate`; a valid path then hands over to the branch
 * loader (a repository with no branch at all has no commit yet, and is refused here),
 * and an invalid one (or a failed request) clears the branch list, because a
 * branch belonging to the previous repo is worse than none — see #454.
 *
 * `loadBranches` / `clearBranches` come from `useLaunchTargets`, which owns that list.
 * They are called from inside the debounced chain rather than reacted to, so a repo stays
 * "validating" until its branches have landed, exactly as before.
 */
export function useRepoValidation({
  open,
  loadBranches,
  clearBranches,
}: {
  open: boolean;
  /** Resolves with the repo's branches, `null` when the list did not land. */
  loadBranches: (repoPath: string) => Promise<readonly unknown[] | null | void>;
  clearBranches: () => void;
}) {
  const [targetRepo, setTargetRepo] = useState("");
  const [repoValid, setRepoValid] = useState<boolean | null>(null);
  const [repoError, setRepoError] = useState<string | null>(null);
  const [repoValidating, setRepoValidating] = useState(false);
  // The path the verdict is about: while a new value waits out the debounce,
  // `repoValid` still speaks of the previous one.
  const [validatedRepo, setValidatedRepo] = useState<string | null>(null);

  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const handleRepoChange = useCallback((value: string) => {
    setTargetRepo(value);
    // A URL is no repository yet (CORE-97): it has no verdict until cloned.
    if (!value.trim() || isRemoteUrl(value)) {
      setRepoValid(null);
      setRepoError(null);
      setValidatedRepo(null);
      clearBranches();
    }
  }, [clearBranches]);

  /**
   * Point the field at `value` and throw the verdict away with it (#470). The modal stays
   * mounted (#386), so a `repoValid === true` left over from a previous open would
   * otherwise survive next to a repo the user never validated.
   */
  const resetRepo = useCallback((value: string) => {
    setTargetRepo(value);
    setRepoValid(null);
    setRepoError(null);
    setValidatedRepo(null);
  }, []);

  useEffect(() => {
    if (!open || !targetRepo.trim() || isRemoteUrl(targetRepo)) return;

    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      setRepoValidating(true);
      setRepoError(null);
      setValidatedRepo(null);
      try {
        const result = await validateRepo(targetRepo.trim());
        setRepoValid(result.valid);
        if (!result.valid) {
          setRepoError(result.error ?? "Not a valid git repository");
          clearBranches();
        } else {
          const branches = await loadBranches(targetRepo.trim());
          // A repository without a commit has no branch to cut a Run from:
          // the field says so, once, rather than a git error further down.
          if (branches && branches.length === 0) {
            setRepoValid(false);
            setRepoError(`${targetRepo.trim()} has no commit yet`);
            clearBranches();
          } else {
            setValidatedRepo(targetRepo.trim());
          }
        }
      } catch {
        setRepoValid(false);
        setRepoError("Failed to validate repository");
        clearBranches();
      } finally {
        setRepoValidating(false);
      }
    }, 400);

    return () => clearTimeout(debounceRef.current);
    // `loadBranches` closes over the CURRENT source branch (the #454 membership test), so
    // listing it here would re-debounce — and re-validate — on every branch pick. The
    // closure captured when `targetRepo` last changed already holds the value the user had
    // then, which is the one the re-selection must judge against.
  }, [targetRepo, open]); // eslint-disable-line react-hooks/exhaustive-deps

  return {
    targetRepo,
    repoValid,
    repoError,
    repoValidating,
    validatedRepo,
    handleRepoChange,
    resetRepo,
  };
}
