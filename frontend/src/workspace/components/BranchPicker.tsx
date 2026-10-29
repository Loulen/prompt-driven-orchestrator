/**
 * The quick pick of the source-branch field (#571, #802): a filter, the local
 * and remote groups, and a footer saying how fresh the list is.
 */

import { Cloud, GitBranch, Laptop, Search } from "lucide-react";
import type { BranchFetchError } from "../../shared/generated/BranchFetchError";
import type { BranchRef } from "../../shared/generated/BranchRef";
import { branchGap, highlightSegments, shortAge } from "../model/branchSelect";
import { GapChip } from "./SyncButton";

interface BranchPickerProps {
  query: string;
  onQueryChange: (query: string) => void;
  onSearchKeyDown: (e: React.KeyboardEvent) => void;
  visibleLocals: BranchRef[];
  visibleRemotes: BranchRef[];
  localCount: number;
  remoteCount: number;
  cursor: number;
  onHover: (index: number) => void;
  value: string;
  onPick: (name: string) => void;
  lastFetchAt: string | null;
  fetchError: BranchFetchError | null;
  fetching: boolean;
  testIdPrefix: string;
}

export function BranchPicker({
  query,
  onQueryChange,
  onSearchKeyDown,
  visibleLocals,
  visibleRemotes,
  localCount,
  remoteCount,
  cursor,
  onHover,
  value,
  onPick,
  lastFetchAt,
  fetchError,
  fetching,
  testIdPrefix,
}: BranchPickerProps) {
  const empty = visibleLocals.length + visibleRemotes.length === 0;
  return (
    <div
      className="absolute left-0 right-0 top-full z-50 mt-1 overflow-hidden rounded-md border border-line-strong bg-surface-2 shadow-xl"
      data-testid={`${testIdPrefix}-popover`}
    >
      <div className="flex items-center gap-2 border-b border-line px-2.5 py-2">
        <Search size={12} className="shrink-0 text-text-3" />
        <input
          autoFocus
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          onKeyDown={onSearchKeyDown}
          placeholder="Filter branches…"
          className="min-w-0 flex-1 bg-transparent text-text placeholder:text-text-3 focus:outline-none"
          style={{ fontSize: "12px" }}
          data-testid={`${testIdPrefix}-search`}
          aria-label="Filter branches"
        />
        <span className="shrink-0 text-text-3" style={{ fontSize: "10px" }}>
          ↑↓ · ⏎ · esc
        </span>
      </div>

      <div className="max-h-64 overflow-y-auto" role="listbox">
        {empty && (
          <div
            className="px-2.5 py-3 text-text-3"
            style={{ fontSize: "11px" }}
            data-testid={`${testIdPrefix}-empty`}
          >
            No branch matches “{query}”.
          </div>
        )}
        <BranchGroup
          label="Local"
          kind="local"
          rows={visibleLocals}
          offset={0}
          cursor={cursor}
          value={value}
          query={query}
          onPick={onPick}
          onHover={onHover}
          testIdPrefix={testIdPrefix}
        />
        <BranchGroup
          label="Remote"
          kind="remote"
          rows={visibleRemotes}
          offset={visibleLocals.length}
          cursor={cursor}
          value={value}
          query={query}
          onPick={onPick}
          onHover={onHover}
          testIdPrefix={testIdPrefix}
        />
      </div>

      <div
        className="border-t border-line px-2.5 py-1.5 text-text-3"
        style={{ fontSize: "10px" }}
        data-testid={`${testIdPrefix}-footer`}
      >
        {localCount} local · {remoteCount} remote · {fetchedLabel(lastFetchAt, fetchError, fetching)}
      </div>
    </div>
  );
}

/**
 * Laptop for a local branch, cloud for a remote-tracking ref — and the neutral
 * branch glyph when the kind is genuinely unknown (a value the repo no longer
 * lists, e.g. an edited Trigger's stored branch). Guessing from the name is never
 * an option: a *local* branch may be called `origin/x` (#571).
 */
export function KindIcon({ kind, size = 12 }: { kind?: "local" | "remote"; size?: number }) {
  const className = "shrink-0 text-text-3";
  if (kind === "remote") return <Cloud size={size} strokeWidth={1.5} className={className} aria-hidden />;
  if (kind === "local") return <Laptop size={size} strokeWidth={1.5} className={className} aria-hidden />;
  return <GitBranch size={size} strokeWidth={1.5} className={className} aria-hidden />;
}

/** "fetched 12 s ago" · "fetched — failed" · "never fetched". */
function fetchedLabel(
  lastFetchAt: string | null,
  fetchError: BranchFetchError | null,
  fetching: boolean,
): string {
  if (fetching) return "fetching…";
  if (fetchError) return "fetched — failed";
  const age = shortAge(lastFetchAt);
  return age ? `fetched ${age} ago` : "never fetched";
}

function BranchGroup({
  label,
  kind,
  rows,
  offset,
  cursor,
  value,
  query,
  onPick,
  onHover,
  testIdPrefix,
}: {
  label: string;
  kind: "local" | "remote";
  rows: BranchRef[];
  /** Index of this group's first row in the flat navigable list. */
  offset: number;
  cursor: number;
  value: string;
  query: string;
  onPick: (name: string) => void;
  onHover: (index: number) => void;
  testIdPrefix: string;
}) {
  if (rows.length === 0) return null;
  return (
    <>
      <div
        className="flex items-center gap-1.5 px-2.5 pt-2 pb-1 uppercase tracking-wider text-text-3"
        style={{ fontSize: "9.5px" }}
      >
        <KindIcon kind={kind} size={10} />
        {label}
        <span className="text-text-3">{rows.length}</span>
      </div>
      {rows.map((branch, i) => {
        const index = offset + i;
        const isSelected = branch.name === value;
        const isCursor = index === cursor;
        return (
          <button
            key={`${kind}-${branch.name}`}
            type="button"
            role="option"
            aria-selected={isSelected}
            onMouseDown={(e) => {
              e.preventDefault();
              onPick(branch.name);
            }}
            onMouseEnter={() => onHover(index)}
            className={`flex w-full items-center gap-2 px-2.5 py-1.5 text-left transition-colors ${
              isSelected ? "bg-selected" : isCursor ? "bg-surface-2" : ""
            }`}
            data-testid={`${testIdPrefix}-option`}
            data-branch={branch.name}
            data-cursor={isCursor ? "true" : undefined}
          >
            <KindIcon kind={branch.kind} />
            <span className="flex min-w-0 flex-1 flex-col leading-tight">
              <span
                className={`truncate font-mono ${isSelected ? "text-primary" : "text-text"}`}
                style={{ fontSize: "11.5px" }}
              >
                {highlightSegments(branch.name, query).map((seg, s) => (
                  <span key={s} className={seg.match ? "text-primary" : undefined}>
                    {seg.text}
                  </span>
                ))}
              </span>
              {branch.last_commit_subject && (
                <span className="truncate text-text-3" style={{ fontSize: "10px" }}>
                  {branch.last_commit_subject}
                </span>
              )}
            </span>
            <GapChip gap={branchGap(branch)} />
            {shortAge(branch.last_commit_at) && (
              <span className="shrink-0 text-text-3" style={{ fontSize: "10px" }}>
                {shortAge(branch.last_commit_at)}
              </span>
            )}
          </button>
        );
      })}
    </>
  );
}
