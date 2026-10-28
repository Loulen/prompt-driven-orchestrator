import { ChevronsDownUp, ChevronsUpDown, PanelLeftClose } from "lucide-react";
import type { RefObject } from "react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { filePath } from "../../run/model/runRefs";
import { Hourglass } from "lucide-react";
import type { StateCounts } from "../model/commentStatus";
import type { ReviewEntry } from "../model/entries";
import { authorKind, authorLabel, commentState, footerStatus, plural, sortForSidebar } from "../model/commentStatus";
import { anchorLabel } from "../model/drafts";
import { dirCount, matchesFilter } from "../model/fileTree";
import type { TreeRow } from "../model/fileTree";
import { AuthorGlyph, Badge, StateIcon } from "./commentBits";
import { DirRow, FileRow } from "./ReviewTreeRows";

/**
 * The Review page's left column (#749): filter box, then one row per file with
 * its status letter, `+a −d` and a 5-block mini bar. The current file follows
 * the scroll; clicking a row scrolls to it.
 *
 * CORE-84: the files sit in a folder tree (`fileTree`), the way a forge's
 * "Files changed" shows them — chevron + folder icon per folder, a chain of
 * single-folder folders on one row, each folder carrying the count of
 * unresolved comments it holds; a click folds or unfolds it, the button next to
 * the file count folds or unfolds them all. A filter keeps the tree and opens
 * the branches that hold a match.
 *
 * #750: each row carries its comment badges (drafts amber, sent blue), and a
 * **Comments** section lists every comment of the Run — badge, anchor, first
 * words — click to jump. Comments written against another ref pair are greyed
 * with their pair; clicking one restores that pair first.
 *
 * #751: sent comments read by **state**. File rows carry icon + count per state
 * (proposed first — it needs a decision); the Comments section sorts
 * proposed → open → resolved, each row ending with two small icons (state,
 * last author), resolved rows dimmed; the section header carries the "needs
 * you" count as an hourglass.
 */

interface Props {
  files: DiffFile[];
  /** CORE-84: the tree rows to show (`visibleRows`: collapse state and filter applied). */
  rows: TreeRow[];
  onToggleDir: (path: string) => void;
  /** Fold or unfold every folder; absent when the diff has no folder. */
  onToggleAllDirs?: () => void;
  allDirsCollapsed?: boolean;
  filter: string;
  onFilterChange: (v: string) => void;
  filterRef: RefObject<HTMLInputElement | null>;
  currentIndex: number;
  onSelect: (index: number) => void;
  onClose: () => void;
  /** #750: per-path comment counts for the row badges. */
  counts?: Map<string, { drafts: number; sent: number }>;
  /** #751: per-path sent-comment state counts (replace the plain "sent" badge when given). */
  stateCounts?: Map<string, StateCounts>;
  /** #752: per-path outdated count — a history glyph on the row when ≥ 1. */
  outdatedCounts?: Map<string, number>;
  /** #750: the Comments section — current pair first, other pairs greyed. */
  comments?: { current: ReviewEntry[]; other: { entry: ReviewEntry; pair: string }[] };
  onJumpEntry?: (entry: ReviewEntry) => void;
  onJumpOther?: (entry: ReviewEntry) => void;
}

export default function ReviewFileList({
  files,
  rows,
  onToggleDir,
  onToggleAllDirs,
  allDirsCollapsed,
  filter,
  onFilterChange,
  filterRef,
  currentIndex,
  onSelect,
  onClose,
  counts,
  stateCounts: states,
  outdatedCounts,
  comments,
  onJumpEntry,
  onJumpOther,
}: Props) {
  const current = comments ? sortForSidebar(comments.current) : [];
  const needsYou = current.filter((e) => e.kind === "sent" && commentState(e.comment) === "proposed").length;
  // The count is the files the filter keeps, folded or not.
  const matching = files.filter((f) => matchesFilter(filePath(f), filter)).length;
  // A folder's count: the drafts and the sent comments not resolved, of every file it holds.
  const unresolved = (p: string) => {
    const drafts = counts?.get(p)?.drafts ?? 0;
    const st = states?.get(p);
    return drafts + (st ? st.open + st.proposed : (counts?.get(p)?.sent ?? 0));
  };

  return (
    <aside
      className="flex min-h-0 flex-col overflow-hidden border-r border-line bg-surface-0"
      data-testid="review-file-list"
    >
      <div className="flex items-center gap-1.5 px-2.5 pb-1.5 pt-2 text-text-3" style={{ fontSize: "10.5px" }}>
        <button
          type="button"
          onClick={onClose}
          title="Hide file list  ( [ )"
          data-testid="review-list-close"
          className="grid h-5 w-5 cursor-pointer place-items-center rounded text-text-3 hover:bg-hover hover:text-text-2"
        >
          <PanelLeftClose size={12} />
        </button>
        <input
          ref={filterRef}
          value={filter}
          onChange={(e) => onFilterChange(e.target.value)}
          placeholder="Filter files…   /"
          data-testid="review-file-filter"
          className="min-w-0 flex-1 rounded border border-line bg-surface-1 px-1.5 py-0.5 text-text outline-none focus:border-primary/30"
          style={{ fontSize: "11px" }}
        />
        <span className="font-mono text-text-3" data-testid="review-file-count">
          {matching}
        </span>
        {onToggleAllDirs && (
          <button
            type="button"
            onClick={onToggleAllDirs}
            title={allDirsCollapsed ? "Expand all folders" : "Collapse all folders"}
            data-testid="review-dirs-toggle"
            data-collapsed={allDirsCollapsed ? "true" : "false"}
            className="grid h-5 w-5 cursor-pointer place-items-center rounded text-text-3 hover:bg-hover hover:text-text-2"
          >
            {allDirsCollapsed ? <ChevronsUpDown size={12} /> : <ChevronsDownUp size={12} />}
          </button>
        )}
      </div>

      <div className="flex-1 overflow-y-auto px-1.5 pb-2.5">
        {rows.length === 0 && (
          <div className="px-2 py-2.5 text-text-3" style={{ fontSize: "11px" }}>
            {files.length === 0 ? "No files" : "No file matches"}
          </div>
        )}
        {rows.map(({ node, depth, open }) =>
          node.kind === "dir" ? (
            <DirRow
              key={`d:${node.path}`}
              node={node}
              depth={depth}
              open={open}
              count={dirCount(node, unresolved)}
              onToggle={() => onToggleDir(node.path)}
            />
          ) : (
            <FileRow
              key={node.path}
              file={files[node.index]}
              depth={depth}
              current={node.index === currentIndex}
              onSelect={() => onSelect(node.index)}
              drafts={counts?.get(node.path)?.drafts ?? 0}
              sent={counts?.get(node.path)?.sent ?? 0}
              states={states?.get(node.path)}
              outdated={outdatedCounts?.get(node.path) ?? 0}
            />
          ),
        )}

        {comments && (
          <div className="mt-3" data-testid="review-comments-section">
            <div
              className="flex items-center justify-between px-1.5 pb-1 pt-1.5 uppercase text-text-3"
              style={{ fontSize: "10px", letterSpacing: ".04em" }}
            >
              <span>Comments</span>
              {comments.current.length + comments.other.length > 0 && (
                <span className="inline-flex items-center gap-1.5 normal-case tracking-normal" data-testid="review-comments-count">
                  {plural(comments.current.filter((e) => e.kind === "draft").length, "draft")} ·{" "}
                  {comments.current.filter((e) => e.kind === "sent").length} sent
                  {needsYou > 0 && (
                    <span className="inline-flex items-center gap-px text-st-await" title={`${needsYou} need${needsYou === 1 ? "s" : ""} your decision`} data-testid="review-needs-you">
                      <Hourglass size={9} />
                      {needsYou}
                    </span>
                  )}
                </span>
              )}
            </div>
            {comments.current.length + comments.other.length === 0 ? (
              <div className="px-1.5 py-1 text-text-3" style={{ fontSize: "10px" }}>
                None yet. Hover a line number and press +.
              </div>
            ) : (
              <>
                {current.map((e) => (
                  <CommentRow key={entryKey(e)} entry={e} onClick={() => onJumpEntry?.(e)} />
                ))}
                {comments.other.map(({ entry, pair }) => (
                  <CommentRow key={entryKey(entry)} entry={entry} pair={pair} onClick={() => onJumpOther?.(entry)} />
                ))}
              </>
            )}
          </div>
        )}
      </div>

      <div
        className="mt-auto flex flex-wrap gap-2 border-t border-line px-2.5 py-2 text-text-3"
        style={{ fontSize: "10px" }}
      >
        <span>
          <Kbd>j</Kbd>/<Kbd>k</Kbd> file
        </span>
        <span>
          <Kbd>n</Kbd>/<Kbd>p</Kbd> hunk
        </span>
        <span>
          <Kbd>←</Kbd>/<Kbd>→</Kbd> fold folder
        </span>
        <span>
          <Kbd>u</Kbd> view
        </span>
        <span>
          <Kbd>[</Kbd> list
        </span>
        <span>
          <Kbd>c</Kbd>/<Kbd>C</Kbd> comment
        </span>
      </div>
    </aside>
  );
}

function Kbd({ children }: { children: string }) {
  return (
    <kbd
      className="rounded-[3px] border border-b-2 border-line-strong bg-surface-2 px-1 font-mono text-text-3"
      style={{ fontSize: "10px" }}
    >
      {children}
    </kbd>
  );
}

function entryKey(e: ReviewEntry): string {
  return e.kind === "draft" ? `d:${e.draft.key}` : `s:${e.comment.id}`;
}

const DOT_CLASS = { open: "bg-st-running", proposed: "bg-st-await", resolved: "bg-st-done" } as const;

function CommentRow({ entry, pair, onClick }: { entry: ReviewEntry; pair?: string; onClick: () => void }) {
  const text = entry.kind === "draft" ? entry.draft.text : entry.comment.text;
  const first = text.split("\n")[0].slice(0, 28);
  const state = entry.kind === "sent" ? commentState(entry.comment) : null;
  const status = entry.kind === "sent" ? footerStatus(entry.comment) : null;
  // The last actor on the thread: who replied / proposed / resolved / reopened.
  const lastActor =
    status && status.kind !== "awaiting" ? ("author" in status ? status.author : status.by) : null;
  const stateTitle =
    state === "proposed"
      ? `resolution proposed${lastActor ? ` by ${authorLabel(lastActor)}` : ""}`
      : state === "resolved"
        ? `resolved by ${authorLabel(entry.kind === "sent" ? (entry.comment.resolved_by ?? "user") : "user")}`
        : lastActor
          ? `replied by ${authorLabel(lastActor)}`
          : "awaiting reply";
  return (
    <button
      type="button"
      onClick={onClick}
      title={pair ? `${text.slice(0, 120)}\n(at ${pair})` : `${text.slice(0, 120)}${state ? `\n${stateTitle}` : ""}`}
      data-testid="review-comment-row"
      data-kind={entry.kind}
      data-review-state={state ?? undefined}
      data-other-pair={pair ? "true" : undefined}
      className={`grid w-full cursor-pointer grid-cols-[auto_auto_1fr_auto] items-center gap-1.5 rounded px-1.5 py-[3px] text-left hover:bg-hover ${
        pair || state === "resolved" ? "opacity-55" : ""
      }`}
      style={{ fontSize: "10.5px" }}
    >
      {state ? (
        <span className={`inline-block h-1.5 w-1.5 rounded-full ${DOT_CLASS[state]}`} aria-hidden />
      ) : (
        <Badge kind="draft">✎</Badge>
      )}
      <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>
        {anchorLabel(entry.anchor)}
      </span>
      <span className="truncate text-text-3">{pair ? `at ${pair}` : first}</span>
      {state && (
        <span className="inline-flex items-center gap-1 text-text-3" title={stateTitle}>
          <StateIcon state={state} size={9} />
          {lastActor && <AuthorGlyph kind={authorKind(lastActor)} size={9} />}
        </span>
      )}
    </button>
  );
}
