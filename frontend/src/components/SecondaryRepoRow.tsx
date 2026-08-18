import { useEffect, useRef, useState } from "react";
import { GitBranch, X } from "lucide-react";
import { validateRepo, listBranches } from "../api";
import RepoCombobox from "./RepoCombobox";

/** One secondary repo line of the multi-repo create modal (#465, ADR-0042/0045).
 *  `valid` is `null` while unknown/empty, so `canLaunch` can require every
 *  non-empty line to have resolved. `readOnly` is the ADR-0045 opt-in — default
 *  `false` ⇒ the secondary is writable. */
export interface SecondaryRepo {
  path: string;
  baseBranch: string;
  valid: boolean | null;
  readOnly: boolean;
}

interface Props {
  index: number;
  repo: SecondaryRepo;
  recentRepos: string[];
  /** Merge a patch into this row (functional setState in the parent, so it is
   *  stable per index and cannot loop the validation effect). */
  onChange: (index: number, patch: Partial<SecondaryRepo>) => void;
  onRemove: (index: number) => void;
}

/**
 * A self-validating secondary-repo row: it owns its own debounced
 * `validateRepo` + `listBranches` effect (mirroring the primary's) and reports
 * `valid`/`baseBranch` back up. Encapsulating the async here is what lets the
 * parent hold a plain array of rows without calling hooks in a loop.
 */
export default function SecondaryRepoRow({
  index,
  repo,
  recentRepos,
  onChange,
  onRemove,
}: Props) {
  const [validating, setValidating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [branches, setBranches] = useState<string[]>([]);
  const [branchesLoading, setBranchesLoading] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const path = repo.path;

  useEffect(() => {
    clearTimeout(debounceRef.current);
    let cancelled = false;
    // All state writes live inside the debounced callback (never synchronously in
    // the effect body) — the same shape as the primary repo's effect, and what keeps
    // `react-hooks/set-state-in-effect` quiet.
    debounceRef.current = setTimeout(async () => {
      if (!path.trim()) {
        if (cancelled) return;
        setError(null);
        setBranches([]);
        onChange(index, { valid: null });
        return;
      }
      setValidating(true);
      setError(null);
      try {
        const result = await validateRepo(path.trim());
        if (cancelled) return;
        if (!result.valid) {
          setError(result.error ?? "Not a valid git repository");
          setBranches([]);
          onChange(index, { valid: false });
          return;
        }
        onChange(index, { valid: true });
        setBranchesLoading(true);
        try {
          const branchList = await listBranches(path.trim());
          if (cancelled) return;
          setBranches(branchList);
          // Seed the base branch when the held value is not one THIS repo has
          // (mirror of the primary's #454 membership test). Default HEAD → prefer
          // main/master, else the first branch.
          if (branchList.length > 0 && !branchList.includes(repo.baseBranch)) {
            const main =
              branchList.find((b) => b === "main") ??
              branchList.find((b) => b === "master") ??
              branchList[0];
            onChange(index, { baseBranch: main });
          }
        } catch {
          if (!cancelled) setBranches([]);
        } finally {
          if (!cancelled) setBranchesLoading(false);
        }
      } catch {
        if (cancelled) return;
        setError("Failed to validate repository");
        setBranches([]);
        onChange(index, { valid: false });
      } finally {
        if (!cancelled) setValidating(false);
      }
    }, 400);

    return () => {
      cancelled = true;
      clearTimeout(debounceRef.current);
    };
    // `onChange` is stable (parent useCallback) and `repo.baseBranch` is only read
    // to decide re-seeding — depending on it would re-run on every keystroke.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, index]);

  let borderClass = "border-line-strong focus:border-acc";
  if (repo.valid === true) borderClass = "border-acc focus:border-acc";
  else if (repo.valid === false)
    borderClass = "border-st-failed focus:border-st-failed";

  return (
    <div className="flex flex-col gap-1.5" data-testid={`secondary-repo-row-${index}`}>
      <div className="flex items-center gap-2">
        <div className="flex-1">
          <RepoCombobox
            value={repo.path}
            onChange={(value) => onChange(index, { path: value })}
            recentRepos={recentRepos}
            repoValid={repo.valid}
            repoValidating={validating}
            repoError={error}
            borderClass={borderClass}
          />
        </div>
        <button
          type="button"
          onClick={() => onRemove(index)}
          className="rounded-md border border-line-strong bg-bg-3 p-1.5 text-fg-3 transition-colors hover:border-st-failed hover:text-st-failed"
          aria-label="Remove secondary repository"
          data-testid={`remove-secondary-repo-${index}`}
        >
          <X size={12} />
        </button>
      </div>
      {repo.valid && (
        <select
          className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none disabled:opacity-40"
          style={{ fontSize: "12px" }}
          disabled={branches.length === 0}
          value={repo.baseBranch}
          onChange={(e) => onChange(index, { baseBranch: e.target.value })}
          data-testid={`secondary-branch-select-${index}`}
        >
          {branchesLoading && <option value="">Loading branches...</option>}
          {!branchesLoading && branches.length === 0 && (
            <option value="">Loading...</option>
          )}
          {branches.map((b) => (
            <option key={b} value={b}>
              {b}
            </option>
          ))}
        </select>
      )}
      {repo.valid && (
        <label
          className="flex items-center gap-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          <input
            type="checkbox"
            className="accent-acc"
            checked={repo.readOnly}
            onChange={(e) => onChange(index, { readOnly: e.target.checked })}
            data-testid={`secondary-readonly-${index}`}
          />
          Read-only (context only; do not modify)
        </label>
      )}
    </div>
  );
}

/** Small labelled icon header reused for the secondary section. */
export function SecondaryRepoLabel() {
  return (
    <span
      className="font-medium text-fg-2 flex items-center gap-1.5"
      style={{ fontSize: "11.5px" }}
    >
      <GitBranch size={12} className="text-fg-3" />
      Secondary repositories
    </span>
  );
}
