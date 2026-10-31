import { ChevronsDownUp, ChevronsUpDown, PanelLeftClose, Search } from "lucide-react";
import { IconButton } from "../../shared/ui/kit";
import type { RefObject } from "react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { filePath } from "../../run/model/runRefs";
import { Hourglass } from "lucide-react";
import type { StateCounts } from "../model/commentStatus";
import type { ReviewEntry } from "../model/entries";
import { authorKind, authorLabel, commentState, footerStatus, plural, sortForSidebar } from "../model/commentStatus";
import { anchorLabel, anchorParts } from "../model/drafts";
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
  /** Files marked « Viewed » for this pair: a check on the row, the name dimmed. */
  viewed?: Set<string>;
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
  viewed,
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
    <aside className="rv-tree" data-testid="review-file-list">
      <div className="rv-tree__head">
        <IconButton
          icon={PanelLeftClose}
          label="Hide file list"
          kbd="["
          size="sm"
          onClick={onClose}
          data-testid="review-list-close"
        />
        <label className="rv-search">
          <Search size={13} strokeWidth={1.5} aria-hidden />
          <input
            ref={filterRef}
            value={filter}
            onChange={(e) => onFilterChange(e.target.value)}
            placeholder="Filter files"
            aria-label="Filter files (/)"
            title="Filter files (/)"
            data-testid="review-file-filter"
          />
        </label>
        <span className="rv-bar__meta" title="Files" data-testid="review-file-count">
          {matching}
        </span>
        {onToggleAllDirs && (
          <IconButton
            icon={allDirsCollapsed ? ChevronsUpDown : ChevronsDownUp}
            label={allDirsCollapsed ? "Expand all folders" : "Collapse all folders"}
            size="sm"
            onClick={onToggleAllDirs}
            data-testid="review-dirs-toggle"
            data-collapsed={allDirsCollapsed ? "true" : "false"}
          />
        )}
      </div>

      <div className="rv-tree__scroll" role="tree" aria-label="Files">
        {rows.length === 0 && (
          <div className="px-4 py-2 text-sm text-text-3">{files.length === 0 ? "No files" : "No file matches"}</div>
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
              viewed={viewed?.has(node.path) ?? false}
            />
          ),
        )}

        {comments && (
          <section className="rv-csec" data-testid="review-comments-section" aria-label="Comments">
            <div className="rv-csec__head">
              <span className="d-caps">Comments</span>
              <span className="rv-grow" />
              {comments.current.length + comments.other.length > 0 && (
                <span className="d-meta inline-flex items-center gap-1.5" data-testid="review-comments-count">
                  {plural(comments.current.filter((e) => e.kind === "draft").length, "draft")} ·{" "}
                  {comments.current.filter((e) => e.kind === "sent").length} sent
                  {needsYou > 0 && (
                    <span className="inline-flex items-center gap-px text-st-await" title={`${needsYou} need${needsYou === 1 ? "s" : ""} your decision`} data-testid="review-needs-you">
                      <Hourglass size={10} />
                      {needsYou}
                    </span>
                  )}
                </span>
              )}
            </div>
            {comments.current.length + comments.other.length === 0 ? (
              <div className="px-4 py-1 text-xs text-text-3">None yet</div>
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
          </section>
        )}
      </div>
    </aside>
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
  const at = anchorLabel(entry.anchor);
  const [atFile, atLine] = anchorParts(entry.anchor);
  return (
    <button
      type="button"
      onClick={onClick}
      title={pair ? `${text.slice(0, 120)}\n(at ${pair})` : `${text.slice(0, 120)}${state ? `\n${stateTitle}` : ""}`}
      data-testid="review-comment-row"
      data-kind={entry.kind}
      data-review-state={state ?? undefined}
      data-other-pair={pair ? "true" : undefined}
      data-dim={pair || state === "resolved" ? "true" : undefined}
      className="rv-crow"
    >
      {state ? (
        <span className={`inline-block h-1.5 w-1.5 rounded-full ${DOT_CLASS[state]}`} aria-hidden />
      ) : (
        <Badge kind="draft">✎</Badge>
      )}
      {/* Same cut as a folded thread: the file name gives way, the line stays. */}
      <span className="rv-crow__at" title={at} data-testid="review-comment-row-anchor">
        <span className="rv-crow__at-file">{atFile}</span>
        <span className="shrink-0">{atLine}</span>
      </span>
      <span className="truncate">{pair ? `at ${pair}` : first}</span>
      {state && (
        <span className="inline-flex items-center gap-1" title={stateTitle}>
          <StateIcon state={state} size={10} />
          {lastActor && <AuthorGlyph kind={authorKind(lastActor)} size={10} />}
        </span>
      )}
    </button>
  );
}
