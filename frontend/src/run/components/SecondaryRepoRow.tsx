import { useEffect, useRef, useState, type ReactNode } from "react";
import { GitBranch, Trash2 } from "lucide-react";
import { IconButton } from "../../shared/ui/kit";
import MidPath from "../../shared/ui/MidPath";
import { tildify } from "../../lib/homePath";
import { validateRepo } from "../../workspace/api/repos";
import { pickDefaultBranch } from "../../workspace/model/branchSelect";
import { useBranchList } from "../../workspace/store/useBranchList";
import RepoCombobox from "../../workspace/components/RepoCombobox";
import SourceBranchField from "../../workspace/components/SourceBranchField";
import { useRunCreationRightsStore } from "../store/runCreationRightsStore";

/** One secondary repo line of the multi-repo create modal (#465, ADR-0042/0047).
 *  `valid` is `null` while unknown/empty, so `canLaunch` can require every
 *  non-empty line to have resolved. `readOnly` is the ADR-0047 opt-in — default
 *  `false` ⇒ the secondary is writable. */
export interface SecondaryRepo {
  path: string;
  baseBranch: string;
  valid: boolean | null;
  readOnly: boolean;
  /** Checked in « Other repositories of <Projet> » (ADR-0087): dropped when the Projet changes. */
  fromProject?: boolean;
}

interface Props {
  index: number;
  repo: SecondaryRepo;
  recentRepos: string[];
  /** Merge a patch into this row (functional setState in the parent, so it is
   *  stable per index and cannot loop the validation effect). */
  onChange: (index: number, patch: Partial<SecondaryRepo>) => void;
  onRemove: (index: number) => void;
  /** The daemon's home, to read a path as `~/code/…` (the Projet's rows). */
  home?: string | null;
  /** A repository of the Projet (ADR-0087): a checked line, its path fixed. */
  inProject?: boolean;
  /** After the path of a Projet's line: the other Projets that hold it. */
  also?: ReactNode;
}

/**
 * A self-validating secondary-repo row: it owns its own debounced
 * `validateRepo` + `listBranches` effect (mirroring the primary's) and reports
 * `valid`/`baseBranch` back up. Encapsulating the async here is what lets the
 * parent hold a plain array of rows without calling hooks in a loop.
 *
 * #802/ADR-0070: it also fetches its own repo's remotes — one fetch per repo,
 * this row's own. The problem is identical here (a secondary pinned on a base
 * branch several commits behind is the same silent staleness), so it carries the
 * same quick pick and the same sync button as the primary, from the same
 * component.
 */
export default function SecondaryRepoRow({
  index,
  repo,
  recentRepos,
  onChange,
  onRemove,
  home,
  inProject = false,
  also,
}: Props) {
  const [branchOpen, setBranchOpen] = useState(false);
  // CORE-5: no loupe when the disk may not be browsed.
  const canBrowse = useRunCreationRightsStore((s) => s.rights.canBrowse);
  const [validating, setValidating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // #802: the same branch state, the same fetch, the same race rules as the
  // primary field — from the same hook, so the two cannot drift (#571's lesson).
  const {
    branches,
    loading: branchesLoading,
    lastFetchAt,
    fetchError,
    fetching,
    load,
    refetch: refetchRemotes,
    reread: rereadBranches,
    fastForward,
    clear: clearBranches,
  } = useBranchList();

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
        clearBranches();
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
          clearBranches();
          onChange(index, { valid: false });
          return;
        }
        onChange(index, { valid: true });
        const list = await load(path.trim());
        if (cancelled || !list) return;
        // Seed the base branch when the held value is not one THIS repo has
        // (mirror of the primary's #454 membership test, now on `name`). The
        // default is locality-aware (#571): never a remote while a local exists.
        if (list.length > 0 && !list.some((b) => b.name === repo.baseBranch)) {
          const def = pickDefaultBranch(list);
          if (def) onChange(index, { baseBranch: def });
        }
      } catch {
        if (cancelled) return;
        setError("Failed to validate repository");
        clearBranches();
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

  if (inProject) {
    // ADR-0087: one of the Projet's repositories, checked — its path is the
    // Projet's, so it shows as a line: unchecking takes it back out.
    return (
      <div className="ln-secrow" data-testid={`project-repo-row-${index}`}>
        <div className="ln-secrow__line" data-on="" data-invalid={repo.valid === false || undefined}>
          <input
            type="checkbox"
            checked
            onChange={() => onRemove(index)}
            aria-label={`Use ${path} as a secondary repository`}
            data-testid={`project-repo-check-${index}`}
          />
          <MidPath path={tildify(path, home)} className="ln-secrow__path" />
          {also}
          <span className="ln-grow" />
          <AccessSeg index={index} readOnly={repo.readOnly} onChange={(readOnly) => onChange(index, { readOnly })} prefix="project-repo" />
          <button
            type="button"
            onClick={() => setBranchOpen((o) => !o)}
            className="d-sel d-sel--sm d-sel--mono ln-secrow__branch"
            aria-expanded={branchOpen}
            title="Base branch"
            data-testid={`project-repo-branch-${index}`}
          >
            <GitBranch className="d-sel__lead" size={12} strokeWidth={1.5} aria-hidden />
            <span className="d-sel__val">{validating ? "…" : repo.baseBranch || "HEAD"}</span>
          </button>
        </div>
        {error && <span className="ln-err">{error}</span>}
        {branchOpen && repo.valid && (
          <SourceBranchField
            value={repo.baseBranch}
            onChange={(branch) => onChange(index, { baseBranch: branch })}
            branches={branches}
            loading={branchesLoading}
            fetching={fetching}
            lastFetchAt={lastFetchAt}
            fetchError={fetchError}
            onFetch={refetchRemotes}
            onReread={rereadBranches}
            onFastForward={fastForward}
            testIdPrefix={`secondary-branch-${index}`}
            ariaLabel={`Base branch for secondary repository ${index + 1}`}
          />
        )}
      </div>
    );
  }

  return (
    <div className="ln-secrow" data-testid={`secondary-repo-row-${index}`}>
      <div className="ln-secrow__line">
        <div className="ln-grow">
          <RepoCombobox
            id={`secondary-repo-${index}`}
            ariaLabel={`Secondary repository ${index + 1}`}
            value={repo.path}
            onChange={(value) => onChange(index, { path: value })}
            recentRepos={recentRepos}
            repoValid={repo.valid}
            repoValidating={validating}
            repoError={error}
            canBrowse={canBrowse}
            home={home}
          />
        </div>
        {repo.valid && (
          <AccessSeg index={index} readOnly={repo.readOnly} onChange={(readOnly) => onChange(index, { readOnly })} prefix="secondary" />
        )}
        <IconButton
          icon={Trash2}
          label="Remove secondary repository"
          onClick={() => onRemove(index)}
          data-testid={`remove-secondary-repo-${index}`}
        />
      </div>
      {repo.valid && (
        // #802: the same quick pick and the same sync button as the primary, from
        // the same component — the #571 lesson about duplicated branch logic.
        <SourceBranchField
          value={repo.baseBranch}
          onChange={(branch) => onChange(index, { baseBranch: branch })}
          branches={branches}
          loading={branchesLoading}
          fetching={fetching}
          lastFetchAt={lastFetchAt}
          fetchError={fetchError}
          onFetch={refetchRemotes}
          onReread={rereadBranches}
          onFastForward={fastForward}
          testIdPrefix={`secondary-branch-${index}`}
          ariaLabel={`Base branch for secondary repository ${index + 1}`}
        />
      )}
    </div>
  );
}

/**
 * The access of a secondary repository (ADR-0047): read-only or writable, a
 * small segmented choice. `<prefix>-readonly-<i>` / `<prefix>-writable-<i>`.
 */
function AccessSeg({
  index,
  readOnly,
  onChange,
  prefix,
}: {
  index: number;
  readOnly: boolean;
  onChange: (readOnly: boolean) => void;
  prefix: "secondary" | "project-repo";
}) {
  return (
    <div className="d-seg d-seg--sm ln-access" role="radiogroup" aria-label="Access">
      {(
        [
          [true, "read-only"],
          [false, "writable"],
        ] as const
      ).map(([ro, label]) => (
        <button
          key={label}
          type="button"
          role="radio"
          aria-checked={readOnly === ro}
          className="d-seg__btn"
          onClick={() => onChange(ro)}
          data-testid={`${prefix}-${ro ? "readonly" : "writable"}-${index}`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
