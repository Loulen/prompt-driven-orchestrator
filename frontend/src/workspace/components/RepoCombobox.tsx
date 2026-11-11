import { useCallback, useRef, useState, type ReactNode } from "react";
import { Check, ChevronDown, CircleCheck, FolderGit2, FolderSearch, History, LoaderCircle, Search } from "lucide-react";
import FsExplorerModal from "./FsExplorerModal";
import { tildify } from "../../lib/homePath";
import { AnchoredPanel } from "../../shared/ui/kit/layer";
import { IconButton } from "../../shared/ui/kit";
import MidPath from "../../shared/ui/MidPath";

/** A group of repositories the dropdown offers, under a heading (a Projet's name). */
export interface RepoChoiceGroup {
  label: string;
  repos: string[];
  /** The Projet the group is, when it is one: a repository picked under it is picked in it. */
  projectId?: string;
}

interface Props {
  value: string;
  onChange: (path: string) => void;
  recentRepos: string[];
  repoValid: boolean | null;
  repoValidating: boolean;
  repoError: string | null;
  /**
   * Open the explorer HERE instead of at the field's current value (#824). For a
   * host walking someone to a repository they have never seen — it is not in
   * recents, so the field cannot point at it and the folder would have to be
   * climbed to by hand. Read when the explorer opens; changing it later does not
   * move an open explorer, which would fight the user's own navigation.
   */
  explorerStartPath?: string;
  /**
   * Why the loupe (browsing the disk) is greyed: the daemon would refuse the
   * browse (CORE-5); its tooltip says this sentence (CORE-205). Active by default.
   */
  browseRefused?: string;
  /**
   * When set, the dropdown offers these groups instead of the recent
   * repositories: the repositories a Run may be created in, by Projet (CORE-5).
   */
  groups?: RepoChoiceGroup[];
  /** One line closing the grouped dropdown (« 1 Project(s) hidden… »). */
  groupsNote?: string | null;
  /** One line of help under the field. */
  help?: string | null;
  /** The daemon's home, to read a repository as `~/code/…`. */
  home?: string | null;
  /** A mark after the path (New Run's « default »): never over the path. */
  badge?: ReactNode;
  /** A mark after a path of the list (« default » on the Projet's primary), in its group. */
  rowBadge?: (path: string, group: RepoChoiceGroup) => ReactNode;
  /**
   * The field only searches the offered repositories: what is typed filters
   * the list and is never taken as a repository; a repository is picked from
   * the list. For whoever may not read the disk (no path or URL of its own).
   */
  searchOnly?: boolean;
  /** A repository picked from the list, with the group it was picked under. */
  onPick?: (path: string, group: RepoChoiceGroup) => void;
  /** The icon at the start of the field (New Run's globe for a URL, CORE-97). */
  leading?: ReactNode;
  /** Replaces the verdict under the field (a URL has none: it is cloned first). */
  hint?: ReactNode;
  /**
   * The form's own refusal of a valid repository (New Run's « not a repository
   * of the Projet »): the field reads invalid, shows no valid verdict, and says
   * this unless the repository's own error already speaks.
   */
  refusal?: ReactNode;
  /** Accessible name of the field (a secondary row has no visible label). */
  ariaLabel?: string;
  /** `id` of the input, for an outer `<label htmlFor>`. */
  id?: string;
}

/**
 * The repository field of the launch forms: a path or a URL typed in a field
 * that reads like the kit's dropdown, the chosen path cut in its middle while
 * the field is not focused (CORE-108), the verdict at its end; the chevron
 * (or focusing the field) lists the recent repositories, or the groups the
 * Droits allow (CORE-5), each path cut in its middle. The loupe, beside the
 * field, opens the folder explorer. Search-only, the field types a search
 * through the offered repositories, never a path.
 */
export default function RepoCombobox({
  value,
  onChange,
  recentRepos,
  repoValid,
  repoValidating,
  repoError,
  explorerStartPath,
  browseRefused,
  groups,
  groupsNote,
  help,
  home,
  badge,
  rowBadge,
  leading,
  hint,
  refusal,
  ariaLabel,
  id = "target-repo",
  searchOnly = false,
  onPick,
}: Props) {
  const [dropdownOpen, setDropdownOpen] = useState(false);
  // CORE-108: out of focus, the path is shown cut in its middle over the input;
  // focused, the input shows the raw value to edit.
  const [focused, setFocused] = useState(false);
  const fieldRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  // The filesystem explorer (#131, `FsExplorerModal` since #431): the picking
  // flow runs through `onChange` and its validation, like a typed path.
  const [explorerOpen, setExplorerOpen] = useState(false);
  // Search-only: what is typed, a filter of the list, never the value; `null`
  // until something is typed (the field shows the repository picked).
  const [query, setQuery] = useState<string | null>(null);

  // CORE-5: the restricted form offers its groups; otherwise the recents, in
  // one group. A value picked from them shows every choice again; a typed one
  // narrows them.
  const grouped = groups !== undefined;
  const offered: RepoChoiceGroup[] = grouped ? groups : [{ label: "Recent", repos: recentRepos }];
  const narrowing = searchOnly ? !!query?.trim() : !offered.some((g) => g.repos.includes(value));
  const needle = (searchOnly ? (query ?? "").trim() : value).toLowerCase();
  const shownGroups = offered
    .map((g) => ({
      ...g,
      repos: narrowing ? g.repos.filter((r) => r.toLowerCase().includes(needle)) : g.repos,
    }))
    .filter((g) => g.repos.length > 0);
  const noMatch = searchOnly && shownGroups.length === 0;
  const showList = dropdownOpen && !explorerOpen && (shownGroups.length > 0 || noMatch || (grouped && !!groupsNote));

  const showPath = !focused && value.trim() !== "";
  const invalid = !hint && (repoValid === false || !!repoError || !!refusal);

  const handleSelect = useCallback(
    (repo: string, group: RepoChoiceGroup) => {
      if (onPick) onPick(repo, group);
      else onChange(repo);
      setQuery(null);
      setDropdownOpen(false);
      inputRef.current?.focus();
    },
    [onChange, onPick],
  );

  const openExplorer = useCallback(() => {
    setDropdownOpen(false);
    setExplorerOpen(true);
  }, []);

  const verdict = hint ? null : repoValidating ? (
    <LoaderCircle size={12} strokeWidth={2} className="d-spin ln-field__verdict" aria-label="Validating" role="img" />
  ) : repoValid && !repoError && !refusal ? (
    <span className="ln-field__verdict ln-field__verdict--ok" role="img" aria-label="Valid git repository" title="Valid git repository" data-testid="repo-valid">
      <CircleCheck size={12} strokeWidth={2} aria-hidden />
    </span>
  ) : null;

  return (
    <div className="ln-repo">
      <div className="ln-repo__line">
        {/* CORE-108: one row — the icon, the text box, the mark, the verdict,
            the chevron. The mark is a sibling of the text box, so a long path
            shrinks before it and never runs under it. */}
        <div ref={fieldRef} className="ln-field" data-invalid={invalid || undefined} data-open={showList || undefined}>
          {leading ??
            (searchOnly && (focused || value.trim() === "") ? (
              <Search size={13} strokeWidth={1.5} className="ln-field__lead" aria-hidden data-testid="repo-search-icon" />
            ) : (
              <FolderGit2 size={13} strokeWidth={1.5} className="ln-field__lead" aria-hidden />
            ))}
          <div className="ln-field__box">
            <input
              ref={inputRef}
              id={id}
              className={showPath ? "ln-field__input text-transparent" : "ln-field__input"}
              placeholder={searchOnly ? "Search your Projects' repositories" : "Path or URL"}
              aria-label={ariaLabel}
              aria-invalid={invalid || undefined}
              aria-expanded={showList}
              aria-haspopup="listbox"
              title={value || undefined}
              value={searchOnly && query !== null ? query : value}
              onChange={(e) => {
                const next = e.target.value;
                if (searchOnly) {
                  setQuery(next);
                  setDropdownOpen(true);
                  return;
                }
                onChange(next);
                // A path typed (or pasted) whole is a pick: the list steps aside.
                setDropdownOpen(!offered.some((g) => g.repos.includes(next)));
              }}
              onFocus={(e) => {
                setFocused(true);
                // Search-only, typing replaces the repository shown with a search.
                if (searchOnly) e.target.select();
                // A field that already holds one of the offered repositories
                // opens its list on the chevron or ↓, not on a mere focus.
                if (narrowing || (searchOnly && value.trim() === "")) setDropdownOpen(true);
              }}
              onBlur={() => {
                // The rows and the chevron keep the focus here (mousedown is
                // cancelled on them): a blur is leaving the field.
                setFocused(false);
                setDropdownOpen(false);
                setQuery(null);
              }}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown" && !dropdownOpen) {
                  e.preventDefault();
                  setDropdownOpen(true);
                } else if (searchOnly && e.key === "Enter") {
                  // A search is no repository: Enter takes the first match, if any.
                  e.preventDefault();
                  const first = shownGroups[0];
                  if (dropdownOpen && first) handleSelect(first.repos[0], first);
                }
              }}
              spellCheck={false}
              autoComplete="off"
              data-testid="target-repo-input"
            />
            {showPath && (
              <span className="ln-field__over" aria-hidden>
                <MidPath path={tildify(value, home)} title={false} testId="target-repo-path" />
              </span>
            )}
          </div>
          {badge && <span className="ln-field__badge">{badge}</span>}
          {verdict}
          <button
            type="button"
            tabIndex={-1}
            className="ln-field__chev"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              setDropdownOpen((o) => !o);
              inputRef.current?.focus();
            }}
            aria-label="Show the repositories"
            data-testid={grouped ? "repo-groups-trigger" : "repo-list-trigger"}
          >
            <ChevronDown size={12} strokeWidth={1.5} aria-hidden />
          </button>
        </div>
        <IconButton
          icon={FolderSearch}
          label="Browse folders"
          disabledReason={browseRefused}
          onClick={openExplorer}
          data-testid="repo-browse-trigger"
        />
      </div>
      {hint}
      {invalid &&
        (repoError ? (
          <span className="ln-err" data-testid="repo-error">
            {repoError}
          </span>
        ) : (
          refusal
        ))}
      {help && (
        <span className="ln-faint ln-help" data-testid="repo-help">
          {help}
        </span>
      )}

      <AnchoredPanel
        anchor={fieldRef}
        open={showList}
        onClose={() => setDropdownOpen(false)}
        className="ln-repo__pop"
        role="listbox"
        aria-label="Repositories"
      >
        <div className="d-pop__list" data-testid={grouped ? "repo-groups-dropdown" : "recent-repos-dropdown"}>
          {shownGroups.map((group) => (
            <div key={group.projectId ?? group.label} role="group" aria-label={group.label} data-testid="repo-group" data-project-id={group.projectId}>
              <div className="d-caps d-pop__group ln-repo__group" title={group.label}>
                {group.label}
              </div>
              {group.repos.map((repo) => (
                <div
                  key={repo}
                  role="option"
                  aria-selected={repo === value}
                  title={repo}
                  className="d-pop__row ln-repo__row"
                  onMouseDown={(e) => {
                    e.preventDefault();
                    handleSelect(repo, group);
                  }}
                  data-testid={grouped ? "repo-group-item" : "recent-repo-item"}
                >
                  <span className="d-pop__mark">{repo === value && <Check size={12} strokeWidth={2} aria-hidden />}</span>
                  <History size={13} strokeWidth={1.5} className="d-pop__ic" aria-hidden />
                  <MidPath path={tildify(repo, home)} title={false} className="ln-repo__rowpath" />
                  {rowBadge?.(repo, group)}
                </div>
              ))}
            </div>
          ))}
          {noMatch && (
            <div className="d-pop__empty" data-testid="repo-no-match">
              No repository matches.
            </div>
          )}
          {groupsNote && (
            <div className="d-pop__empty ln-repo__note" data-testid="repo-groups-note">
              {groupsNote}
            </div>
          )}
        </div>
      </AnchoredPanel>

      {/* The filesystem explorer (#131 → `FsExplorerModal`, #431), with the
          legacy testids — the irregular `repo-browser-modal` included — that
          `RepoCombobox.test.tsx` and `e2e/repo-explorer-pick.spec.ts` pin. */}
      {explorerOpen && (
        <FsExplorerModal
          testIdPrefix="repo-browse"
          modalTestId="repo-browser-modal"
          // Open-at (Option B): a current absolute value opens at that dir (usually the
          // last repo, pre-filled from recents); else the backend default. A stale value
          // degrades gracefully — the backend clamps a non-existent path to the default.
          // An explicit override from the host wins over both (#824).
          startPath={
            explorerStartPath?.trim().startsWith("/")
              ? explorerStartPath.trim()
              : value.trim().startsWith("/")
                ? value.trim()
                : undefined
          }
          onPick={onChange}
          onClose={() => setExplorerOpen(false)}
        />
      )}
    </div>
  );
}
