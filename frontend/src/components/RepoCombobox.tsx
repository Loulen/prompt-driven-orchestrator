import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Search } from "lucide-react";
import FsExplorerModal from "./FsExplorerModal";

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

  // --- Filesystem explorer (#131, lifted into `FsExplorerModal` in #431). This
  // component is now just its first consumer: it owns the open/closed boolean and
  // passes the legacy testids, so the picking flow still runs through the existing
  // `onChange` → validation path and nothing observable moved. ---
  const [explorerOpen, setExplorerOpen] = useState(false);

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
          startPath={value.trim().startsWith("/") ? value.trim() : undefined}
          onPick={onChange}
          onClose={() => setExplorerOpen(false)}
        />
      )}
    </div>
  );
}
