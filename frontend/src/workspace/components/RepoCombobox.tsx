import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Check, ChevronDown, Search } from "lucide-react";
import FsExplorerModal from "./FsExplorerModal";
import { tildify } from "../../lib/homePath";

/** A group of repositories the dropdown offers, under a heading (a Projet's name). */
export interface RepoChoiceGroup {
  label: string;
  repos: string[];
}

interface Props {
  value: string;
  onChange: (path: string) => void;
  recentRepos: string[];
  repoValid: boolean | null;
  repoValidating: boolean;
  repoError: string | null;
  /** The field's border: its colour, and `focus-within:` for the focused one. */
  borderClass: string;
  /**
   * Open the explorer HERE instead of at the field's current value (#824). For a
   * host walking someone to a repository they have never seen — it is not in
   * recents, so the field cannot point at it and the folder would have to be
   * climbed to by hand. Read when the explorer opens; changing it later does not
   * move an open explorer, which would fight the user's own navigation.
   */
  explorerStartPath?: string;
  /**
   * The loupe (browsing the disk). `false` hides it: the daemon would refuse the
   * browse (CORE-5). Shown by default.
   */
  canBrowse?: boolean;
  /**
   * When set, the dropdown offers these groups instead of the recent
   * repositories: the repositories a Run may be created in, by Projet (CORE-5).
   * A chevron opens it.
   */
  groups?: RepoChoiceGroup[];
  /** One line closing the grouped dropdown (« 1 Project(s) hidden… »). */
  groupsNote?: string | null;
  /** One line of help under the field. */
  help?: string | null;
  /** The daemon's home, to read a grouped repository as `~/code/…`. */
  home?: string | null;
  /** A mark after the path, before the loupe (New Run's « default »): never over the path. */
  badge?: ReactNode;
  /** An icon at the start of the field (New Run's globe for a URL, CORE-97). */
  leading?: ReactNode;
  /** Replaces the verdict lines under the field (a URL has none: it is cloned first). */
  hint?: ReactNode;
}

function splitPath(fullPath: string): { folder: string; parent: string } {
  const trimmed = fullPath.replace(/\/+$/, "");
  const lastSlash = trimmed.lastIndexOf("/");
  if (lastSlash <= 0) return { folder: trimmed, parent: "" };
  return {
    folder: trimmed.slice(lastSlash + 1),
    parent: trimmed.slice(0, lastSlash),
  };
}

/**
 * A path on one line, cut in its middle: the folders shrink with an ellipsis,
 * the last segment stays whole while it fits (`~/work/clients/very-lo…/shop-web`).
 * Laid over the input, which keeps the value, the caret and the full path in its
 * title; it takes no pointer, so a click lands in the input.
 */
function MidPath({ path, className }: { path: string; className: string }) {
  const trimmed = path.replace(/\/+$/, "");
  const cut = trimmed.lastIndexOf("/");
  const head = cut > 0 ? trimmed.slice(0, cut) : "";
  const tail = cut > 0 ? trimmed.slice(cut) : trimmed || path;
  return (
    <span
      aria-hidden
      className={`pointer-events-none absolute inset-0 flex items-center overflow-hidden whitespace-nowrap font-mono ${className}`}
      style={{ fontSize: "12px" }}
      data-testid="target-repo-path"
    >
      {head && (
        <span className="min-w-[1ch] shrink truncate text-text-2" data-testid="target-repo-path-head">
          {head}
        </span>
      )}
      <span className="max-w-full shrink-0 truncate text-text" data-testid="target-repo-path-tail">
        {tail}
      </span>
    </span>
  );
}

export default function RepoCombobox({
  value,
  onChange,
  recentRepos,
  repoValid,
  repoValidating,
  repoError,
  borderClass,
  explorerStartPath,
  canBrowse = true,
  groups,
  groupsNote,
  help,
  home,
  badge,
  leading,
  hint,
}: Props) {
  const [dropdownOpen, setDropdownOpen] = useState(false);
  // CORE-108: out of focus, the path is shown cut in its middle over the input;
  // focused, the input shows the raw value to edit.
  const [focused, setFocused] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // --- Filesystem explorer (#131, lifted into `FsExplorerModal` in #431). This
  // component is now just its first consumer: it owns the open/closed boolean and
  // passes the legacy testids, so the picking flow still runs through the existing
  // `onChange` → validation path and nothing observable moved. ---
  const [explorerOpen, setExplorerOpen] = useState(false);

  const filtered = recentRepos.filter((r) =>
    r.toLowerCase().includes(value.toLowerCase()),
  );

  const showDropdown = dropdownOpen && recentRepos.length > 0 && filtered.length > 0;

  // CORE-5: the restricted form offers its groups. A value picked from them
  // shows every choice again; a typed one narrows them.
  const grouped = groups !== undefined;
  const narrowing = grouped && !groups.some((g) => g.repos.includes(value));
  const shownGroups = (groups ?? [])
    .map((g) => ({
      ...g,
      repos: narrowing ? g.repos.filter((r) => r.toLowerCase().includes(value.toLowerCase())) : g.repos,
    }))
    .filter((g) => g.repos.length > 0);
  const showGroups = grouped && dropdownOpen && (shownGroups.length > 0 || !!groupsNote);

  const handleFocus = useCallback(() => {
    setFocused(true);
    setDropdownOpen(true);
  }, []);

  const showPath = !focused && value.trim() !== "";
  const pad = `${leading ? "pl-2" : "pl-2.5"} ${badge ? "pr-2" : canBrowse || grouped ? "pr-0.5" : "pr-2.5"}`;

  const handleSelect = useCallback(
    (repo: string) => {
      onChange(repo);
      setDropdownOpen(false);
      inputRef.current?.focus();
    },
    [onChange],
  );

  const openExplorer = useCallback(() => {
    setDropdownOpen(false);
    setExplorerOpen(true);
  }, []);

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setDropdownOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  // Recents-dropdown Escape — gated on `!explorerOpen` so it never fires while the
  // explorer is the top layer (the two sibling document listeners fire in
  // registration order, so the explorer's `stopPropagation` alone would be
  // unreliable here). This coupling is the reason the boolean stays in the parent
  // rather than hiding inside the modal.
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && !explorerOpen) setDropdownOpen(false);
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [explorerOpen]);

  return (
    <div ref={containerRef} className="relative">
      {/* CORE-108: one row — the leading icon, the text box, the mark, the loupe.
          The mark and the loupe are siblings of the text box, so a long path
          shrinks before them and never runs under them. */}
      <div className={`flex items-center rounded-md border bg-surface-2 transition-colors ${borderClass}`}>
        {leading && <span className="pointer-events-none flex shrink-0 items-center pl-2.5">{leading}</span>}
        <div className="relative min-w-0 flex-1">
          <input
            ref={inputRef}
            id="target-repo"
            className={`w-full bg-transparent ${pad} py-1.5 font-mono placeholder:text-text-3 focus:outline-none ${
              showPath ? "text-transparent" : "text-text"
            }`}
            style={{ fontSize: "12px" }}
            placeholder="/path/to/your/repo"
            title={value || undefined}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            onFocus={handleFocus}
            onBlur={() => setFocused(false)}
            data-testid="target-repo-input"
            autoComplete="off"
          />
          {showPath && <MidPath path={tildify(value, home)} className={pad} />}
        </div>
        {badge && <span className="flex shrink-0 items-center">{badge}</span>}
        {canBrowse ? (
          <button
            type="button"
            onClick={openExplorer}
            className="flex shrink-0 items-center self-stretch px-2.5 text-text-3 transition-colors hover:text-text-2"
            title="Browse for a repository"
            aria-label="Browse for a repository"
            data-testid="repo-browse-trigger"
          >
            <Search size={14} />
          </button>
        ) : (
          grouped && (
            <button
              type="button"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setDropdownOpen((o) => !o);
                inputRef.current?.focus();
              }}
              className="flex shrink-0 items-center self-stretch px-2.5 text-text-3 transition-colors hover:text-text-2"
              aria-label="Show the repositories"
              data-testid="repo-groups-trigger"
            >
              <ChevronDown size={14} />
            </button>
          )
        )}
      </div>
      {hint}
      {!hint && repoValidating && (
        <span className="text-text-3" style={{ fontSize: "10.5px" }}>
          Validating...
        </span>
      )}
      {!hint && repoError && (
        <span className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="repo-error">
          {repoError}
        </span>
      )}
      {!hint && repoValid && !repoError && (
        <span className="text-primary" style={{ fontSize: "10.5px" }} data-testid="repo-valid">
          Valid git repository
        </span>
      )}
      {help && (
        <span className="block text-text-3" style={{ fontSize: "10.5px" }} data-testid="repo-help">
          {help}
        </span>
      )}

      {showGroups && (
        <div
          className="absolute left-0 right-0 z-50 mt-1 max-h-64 overflow-y-auto rounded-md border border-line bg-surface-2 py-1 shadow-lg"
          data-testid="repo-groups-dropdown"
        >
          {shownGroups.map((group) => (
            <div key={group.label} role="group" aria-label={group.label} data-testid="repo-group">
              <div
                className="px-2.5 pb-0.5 pt-1.5 font-medium uppercase tracking-wider text-text-3"
                style={{ fontSize: "9.5px" }}
              >
                {group.label}
              </div>
              {group.repos.map((repo) => (
                <button
                  key={repo}
                  type="button"
                  title={repo}
                  className={`flex w-full items-center gap-2 px-2.5 py-1 text-left font-mono text-text transition-colors hover:bg-active ${
                    repo === value ? "bg-surface-2" : ""
                  }`}
                  style={{ fontSize: "12px" }}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    handleSelect(repo);
                  }}
                  data-testid="repo-group-item"
                >
                  <span className="min-w-0 flex-1 truncate">{tildify(repo, home)}</span>
                  {repo === value && <Check size={14} className="shrink-0 text-primary" />}
                </button>
              ))}
            </div>
          ))}
          {groupsNote && (
            <div
              className="mt-1 border-t border-line px-2.5 pb-0.5 pt-1.5 text-text-3"
              style={{ fontSize: "10.5px" }}
              data-testid="repo-groups-note"
            >
              {groupsNote}
            </div>
          )}
        </div>
      )}

      {!grouped && showDropdown && (
        <ul
          className="absolute left-0 right-0 z-50 mt-1 max-h-52 overflow-y-auto rounded-md border border-line bg-surface-2 shadow-lg"
          data-testid="recent-repos-dropdown"
        >
          {filtered.map((repo) => {
            const { folder, parent } = splitPath(repo);
            const isActive = repo === value;
            return (
              <li key={repo}>
                <button
                  type="button"
                  className={`flex w-full items-center gap-2 px-2.5 py-1.5 text-left transition-colors hover:bg-active ${
                    isActive ? "bg-surface-2" : ""
                  }`}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    handleSelect(repo);
                  }}
                  data-testid="recent-repo-item"
                >
                  <div className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate font-mono font-semibold text-text" style={{ fontSize: "12px" }}>
                      {folder}
                    </span>
                    {parent && (
                      <span className="truncate font-mono text-text-3" style={{ fontSize: "10.5px" }}>
                        {parent}
                      </span>
                    )}
                  </div>
                  {isActive && (
                    <Check size={14} className="shrink-0 text-primary" />
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      )}

      {/* Filesystem explorer (#131 → `FsExplorerModal`, #431). The legacy testids are
          passed explicitly — including the irregular `repo-browser-modal` (`browser`,
          not `browse`) — so both `RepoCombobox.test.tsx` and the CI e2e spec
          `e2e/repo-explorer-pick.spec.ts` keep passing unedited. */}
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
