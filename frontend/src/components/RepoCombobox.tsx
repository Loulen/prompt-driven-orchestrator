import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowUp, Check, Folder, FolderGit2, Link2, Search } from "lucide-react";
import { browseRepos } from "../api";
import type { BrowseEntry } from "../api";

interface Props {
  value: string;
  onChange: (path: string) => void;
  recentRepos: string[];
  repoValid: boolean | null;
  repoValidating: boolean;
  repoError: string | null;
  borderClass: string;
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

export default function RepoCombobox({
  value,
  onChange,
  recentRepos,
  repoValid,
  repoValidating,
  repoError,
  borderClass,
}: Props) {
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // --- Filesystem explorer (#131): a self-contained nested modal. Picking flows
  // through the existing `onChange` → validation path, so no new selection logic. ---
  const [explorerOpen, setExplorerOpen] = useState(false);
  const [currentDir, setCurrentDir] = useState("");
  const [parent, setParent] = useState<string | null>(null);
  const [entries, setEntries] = useState<BrowseEntry[]>([]);
  const [browseLoading, setBrowseLoading] = useState(false);
  const [browseError, setBrowseError] = useState<string | null>(null);
  const [truncated, setTruncated] = useState(false);

  const filtered = recentRepos.filter((r) =>
    r.toLowerCase().includes(value.toLowerCase()),
  );

  const showDropdown = dropdownOpen && recentRepos.length > 0 && filtered.length > 0;

  const handleFocus = useCallback(() => {
    setDropdownOpen(true);
  }, []);

  const handleSelect = useCallback(
    (repo: string) => {
      onChange(repo);
      setDropdownOpen(false);
      inputRef.current?.focus();
    },
    [onChange],
  );

  // Navigate the explorer to `path` (omit → backend default root). Always lands on a
  // 200 shape: an in-body `error` (e.g. permission denied) is surfaced inline while
  // the breadcrumb is kept, so the user is never stranded on a blank pane.
  const navigateTo = useCallback(async (path?: string) => {
    setBrowseLoading(true);
    setBrowseError(null);
    try {
      const data = await browseRepos(path);
      setCurrentDir(data.path);
      setParent(data.parent);
      setEntries(data.entries);
      setTruncated(data.truncated);
      setBrowseError(data.error);
    } catch (e) {
      setBrowseError(e instanceof Error ? e.message : "Failed to browse");
    } finally {
      setBrowseLoading(false);
    }
  }, []);

  const openExplorer = useCallback(() => {
    setDropdownOpen(false);
    setExplorerOpen(true);
    // Open-at (Option B): a current absolute value opens at that dir (usually the
    // last repo, pre-filled from recents); else the backend default. A stale value
    // degrades gracefully — the backend clamps a non-existent path to the default.
    const start = value.trim().startsWith("/") ? value.trim() : undefined;
    void navigateTo(start);
  }, [value, navigateTo]);

  const pickCurrentDir = useCallback(() => {
    // Pick the folder the user is standing in (git-dotted or not — ADR-0001: any
    // folder is pickable; the authoritative git check gates it downstream).
    onChange(currentDir);
    setExplorerOpen(false);
  }, [currentDir, onChange]);

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
  // registration order, so `stopPropagation` alone would be unreliable here).
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && !explorerOpen) setDropdownOpen(false);
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [explorerOpen]);

  // Explorer Escape — active only while open; closes the explorer alone. The parent
  // New Run modal has no Escape handler, so it is never at risk.
  useEffect(() => {
    if (!explorerOpen) return;
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        setExplorerOpen(false);
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [explorerOpen]);

  return (
    <div ref={containerRef} className="relative">
      <div className="relative">
        <input
          ref={inputRef}
          id="target-repo"
          className={`w-full rounded-md border bg-bg-3 px-2.5 py-1.5 pr-9 font-mono text-fg placeholder:text-fg-4 transition-colors focus:outline-none ${borderClass}`}
          style={{ fontSize: "12px" }}
          placeholder="/path/to/your/repo"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onFocus={handleFocus}
          data-testid="target-repo-input"
          autoComplete="off"
        />
        <button
          type="button"
          onClick={openExplorer}
          className="absolute inset-y-0 right-0 flex items-center px-2.5 text-fg-4 transition-colors hover:text-fg-2"
          title="Browse for a repository"
          aria-label="Browse for a repository"
          data-testid="repo-browse-trigger"
        >
          <Search size={14} />
        </button>
      </div>
      {repoValidating && (
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          Validating...
        </span>
      )}
      {repoError && (
        <span className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="repo-error">
          {repoError}
        </span>
      )}
      {repoValid && !repoError && (
        <span className="text-acc" style={{ fontSize: "10.5px" }} data-testid="repo-valid">
          Valid git repository
        </span>
      )}

      {showDropdown && (
        <ul
          className="absolute left-0 right-0 z-50 mt-1 max-h-52 overflow-y-auto rounded-md border border-line bg-bg-4 shadow-lg"
          data-testid="recent-repos-dropdown"
        >
          {filtered.map((repo) => {
            const { folder, parent } = splitPath(repo);
            const isActive = repo === value;
            return (
              <li key={repo}>
                <button
                  type="button"
                  className={`flex w-full items-center gap-2 px-2.5 py-1.5 text-left transition-colors hover:bg-bg-5 ${
                    isActive ? "bg-bg-5" : ""
                  }`}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    handleSelect(repo);
                  }}
                  data-testid="recent-repo-item"
                >
                  <div className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate font-mono font-semibold text-fg" style={{ fontSize: "12px" }}>
                      {folder}
                    </span>
                    {parent && (
                      <span className="truncate font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
                        {parent}
                      </span>
                    )}
                  </div>
                  {isActive && (
                    <Check size={14} className="shrink-0 text-acc" />
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      )}

      {/* Filesystem explorer (#131). Own full-screen backdrop at z-[60] (above the
          parent modal's z-50); `stopPropagation` keeps a backdrop click from bubbling
          up the React tree to the parent New Run modal's close handler. */}
      {explorerOpen && (
        <div
          className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
          data-testid="repo-browse-backdrop"
          onClick={(e) => {
            e.stopPropagation();
            setExplorerOpen(false);
          }}
        >
          <div
            className="flex max-h-[70vh] w-[460px] flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
            data-testid="repo-browser-modal"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Header: up affordance + breadcrumb */}
            <div className="flex items-center gap-2 border-b border-line px-3 py-2">
              <button
                type="button"
                onClick={() => parent && void navigateTo(parent)}
                disabled={parent == null}
                className="flex shrink-0 items-center justify-center rounded p-1 text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent"
                title="Up one level"
                aria-label="Up one level"
                data-testid="repo-browse-up"
              >
                <ArrowUp size={14} />
              </button>
              <span
                className="min-w-0 flex-1 truncate font-mono text-fg-2"
                style={{ fontSize: "11.5px" }}
                title={currentDir}
                data-testid="repo-browse-path"
              >
                {currentDir || "…"}
              </span>
            </div>

            {/* Body: entry list */}
            <div className="min-h-[120px] flex-1 overflow-y-auto">
              {browseError && (
                <div
                  className="px-3 py-2 font-mono text-st-failed"
                  style={{ fontSize: "11px" }}
                  data-testid="repo-browse-error"
                >
                  {browseError}
                </div>
              )}
              {truncated && (
                <div className="px-3 py-1 text-fg-4" style={{ fontSize: "10.5px" }}>
                  Showing first 1000 folders
                </div>
              )}
              {browseLoading && entries.length === 0 && (
                <div className="px-3 py-2 text-fg-4" style={{ fontSize: "11.5px" }}>
                  Loading…
                </div>
              )}
              {!browseLoading && !browseError && entries.length === 0 && (
                <div className="px-3 py-2 text-fg-4" style={{ fontSize: "11.5px" }}>
                  No subfolders here
                </div>
              )}
              <ul>
                {entries.map((entry) => (
                  <li key={entry.path}>
                    <button
                      type="button"
                      onClick={() => void navigateTo(entry.path)}
                      className="flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-bg-5"
                      data-testid="repo-browse-entry"
                    >
                      {entry.is_git_repo ? (
                        <FolderGit2
                          size={14}
                          className="shrink-0 text-acc"
                          data-testid="repo-browse-git-dot"
                        />
                      ) : (
                        <Folder size={14} className="shrink-0 text-fg-4" />
                      )}
                      <span className="min-w-0 flex-1 truncate font-mono text-fg" style={{ fontSize: "12px" }}>
                        {entry.name}
                      </span>
                      {entry.is_symlink && (
                        <Link2
                          size={12}
                          className="shrink-0 text-fg-4"
                          aria-label="symlink"
                          data-testid="repo-browse-symlink"
                        />
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            </div>

            {/* Footer: cancel + pick-current-dir */}
            <div className="flex items-center justify-end gap-2 border-t border-line px-3 py-2">
              <button
                type="button"
                onClick={() => setExplorerOpen(false)}
                className="rounded-md px-3 py-1.5 text-fg-3 transition-colors hover:text-fg"
                style={{ fontSize: "11.5px" }}
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={pickCurrentDir}
                disabled={!currentDir}
                className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
                style={{ fontSize: "11.5px" }}
                data-testid="repo-browse-select"
              >
                Select this folder
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
