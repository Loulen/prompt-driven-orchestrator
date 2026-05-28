import { useCallback, useEffect, useRef, useState } from "react";
import { Check } from "lucide-react";

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

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setDropdownOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") setDropdownOpen(false);
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, []);

  return (
    <div ref={containerRef} className="relative">
      <input
        ref={inputRef}
        id="target-repo"
        className={`w-full rounded-md border bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 transition-colors focus:outline-none ${borderClass}`}
        style={{ fontSize: "12px" }}
        placeholder="/path/to/your/repo"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onFocus={handleFocus}
        data-testid="target-repo-input"
        autoComplete="off"
      />
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
    </div>
  );
}
