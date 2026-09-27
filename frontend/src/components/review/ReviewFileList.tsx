import { PanelLeftClose } from "lucide-react";
import type { RefObject } from "react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { baseName, filePath, groupByDir, miniBar, statusLetter } from "../../run/model/runRefs";
import { History, Hourglass } from "lucide-react";
import type { ReviewEntry, StateCounts } from "../../lib/reviewComments";
import { anchorLabel, authorKind, authorLabel, commentState, footerStatus, plural, sortForSidebar } from "../../lib/reviewComments";
import { AuthorGlyph, Badge, StateIcon } from "./CommentCard";

/**
 * The Review page's left column (#749, design option "flat list grouped by
 * directory"): filter box, directory headers, one row per file with its status
 * letter, `+a −d` and a 5-block mini bar. The current file follows the scroll;
 * clicking a row scrolls to it.
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

const LETTER_CLASS: Record<"A" | "M" | "D" | "R", string> = {
  A: "text-st-done",
  M: "text-st-await",
  D: "text-st-failed",
  R: "text-edit-tint",
};

export default function ReviewFileList({
  files,
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
  const q = filter.trim().toLowerCase();
  const shown = files
    .map((file, index) => ({ file, index }))
    .filter(({ file }) => !q || filePath(file).toLowerCase().includes(q));
  const groups = groupByDir(shown.map((s) => s.file)).map((g) => ({
    dir: g.dir,
    files: g.files.map((f) => ({ file: f.file, index: shown[f.index].index })),
  }));

  return (
    <aside
      className="flex min-h-0 flex-col overflow-hidden border-r border-line bg-bg-1"
      data-testid="review-file-list"
    >
      <div className="flex items-center gap-1.5 px-2.5 pb-1.5 pt-2 text-fg-3" style={{ fontSize: "10.5px" }}>
        <button
          type="button"
          onClick={onClose}
          title="Hide file list  ( [ )"
          data-testid="review-list-close"
          className="grid h-5 w-5 cursor-pointer place-items-center rounded text-fg-4 hover:bg-bg-3 hover:text-fg-2"
        >
          <PanelLeftClose size={12} />
        </button>
        <input
          ref={filterRef}
          value={filter}
          onChange={(e) => onFilterChange(e.target.value)}
          placeholder="Filter files…   /"
          data-testid="review-file-filter"
          className="min-w-0 flex-1 rounded border border-line bg-bg-2 px-1.5 py-0.5 text-fg outline-none focus:border-acc-border"
          style={{ fontSize: "11px" }}
        />
        <span className="font-mono text-fg-4" data-testid="review-file-count">
          {shown.length}
        </span>
      </div>

      <div className="flex-1 overflow-y-auto px-1.5 pb-2.5">
        {groups.length === 0 && (
          <div className="px-2 py-2.5 text-fg-4" style={{ fontSize: "11px" }}>
            {files.length === 0 ? "No files" : "No file matches"}
          </div>
        )}
        {groups.map((g) => (
          <div key={g.dir || "."}>
            <div
              className="truncate px-1.5 pb-0.5 pt-1.5 font-mono text-fg-4"
              style={{ fontSize: "10px" }}
              title={g.dir ? `${g.dir}/` : "./"}
            >
              {g.dir ? `${g.dir}/` : "./"}
            </div>
            {g.files.map(({ file, index }) => {
              const p = filePath(file);
              const letter = statusLetter(file);
              const cur = index === currentIndex;
              const pureRename = file.status === "renamed" && file.additions + file.deletions === 0;
              return (
                <button
                  key={p}
                  type="button"
                  onClick={() => onSelect(index)}
                  title={p}
                  data-testid="review-file-row"
                  data-path={p}
                  data-current={cur}
                  className={`grid w-full cursor-pointer grid-cols-[14px_1fr_auto] items-center gap-1.5 rounded px-1.5 py-[3px] text-left ${
                    cur ? "bg-bg-3 shadow-[inset_2px_0_0_var(--color-acc)]" : "hover:bg-bg-3"
                  }`}
                  style={{ fontSize: "11px" }}
                >
                  <span
                    className={`w-[14px] text-center font-mono font-semibold ${LETTER_CLASS[letter]}`}
                    style={{ fontSize: "9.5px" }}
                  >
                    {letter}
                  </span>
                  <span className={`truncate font-mono ${cur ? "text-fg" : "text-fg-2"}`} style={{ fontSize: "10.5px" }}>
                    {baseName(p)}
                  </span>
                  <span className="flex items-center gap-1 font-mono" style={{ fontSize: "10px" }}>
                    {(counts?.get(p)?.drafts ?? 0) > 0 && <Badge kind="draft">{counts!.get(p)!.drafts}</Badge>}
                    {(outdatedCounts?.get(p) ?? 0) > 0 && (
                      <span
                        className="inline-flex items-center text-st-stale"
                        title={`${outdatedCounts!.get(p)} outdated — the line changed on this destination`}
                        data-testid="review-file-row-outdated"
                      >
                        <History size={9} />
                      </span>
                    )}
                    {states?.get(p) ? (
                      <>
                        {states.get(p)!.proposed > 0 && (
                          <span className="inline-flex items-center gap-px text-st-await" title={`${states.get(p)!.proposed} resolution proposed`}>
                            <StateIcon state="proposed" size={9} />
                            {states.get(p)!.proposed}
                          </span>
                        )}
                        {states.get(p)!.open > 0 && (
                          <span className="inline-flex items-center gap-px text-fg-4" title={`${states.get(p)!.open} open`}>
                            <StateIcon state="open" size={9} />
                            {states.get(p)!.open}
                          </span>
                        )}
                        {states.get(p)!.resolved > 0 && (
                          <span className="inline-flex items-center gap-px text-st-done" title={`${states.get(p)!.resolved} resolved`}>
                            <StateIcon state="resolved" size={9} />
                          </span>
                        )}
                      </>
                    ) : (
                      (counts?.get(p)?.sent ?? 0) > 0 && <Badge kind="sent">{counts!.get(p)!.sent}</Badge>
                    )}
                    {file.binary ? (
                      <span className="text-fg-4">bin</span>
                    ) : pureRename ? (
                      <span className="text-fg-4">→</span>
                    ) : (
                      <>
                        <span className="text-st-done">+{file.additions}</span>
                        <span className="text-st-failed">−{file.deletions}</span>
                      </>
                    )}
                    <span className="ml-0.5 inline-flex gap-px" aria-hidden>
                      {miniBar(file).map((b, i) => (
                        <i
                          key={i}
                          className={`block h-2 w-[3px] rounded-[1px] ${
                            b === "a" ? "bg-st-done" : b === "d" ? "bg-st-failed" : "bg-bg-4"
                          }`}
                        />
                      ))}
                    </span>
                  </span>
                </button>
              );
            })}
          </div>
        ))}

        {comments && (
          <div className="mt-3" data-testid="review-comments-section">
            <div
              className="flex items-center justify-between px-1.5 pb-1 pt-1.5 uppercase text-fg-4"
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
              <div className="px-1.5 py-1 text-fg-4" style={{ fontSize: "10px" }}>
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
        className="mt-auto flex flex-wrap gap-2 border-t border-line px-2.5 py-2 text-fg-4"
        style={{ fontSize: "10px" }}
      >
        <span>
          <Kbd>j</Kbd>/<Kbd>k</Kbd> file
        </span>
        <span>
          <Kbd>n</Kbd>/<Kbd>p</Kbd> hunk
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
      className="rounded-[3px] border border-b-2 border-line-strong bg-bg-3 px-1 font-mono text-fg-3"
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
      className={`grid w-full cursor-pointer grid-cols-[auto_auto_1fr_auto] items-center gap-1.5 rounded px-1.5 py-[3px] text-left hover:bg-bg-3 ${
        pair || state === "resolved" ? "opacity-55" : ""
      }`}
      style={{ fontSize: "10.5px" }}
    >
      {state ? (
        <span className={`inline-block h-1.5 w-1.5 rounded-full ${DOT_CLASS[state]}`} aria-hidden />
      ) : (
        <Badge kind="draft">✎</Badge>
      )}
      <span className="font-mono text-fg-3" style={{ fontSize: "10px" }}>
        {anchorLabel(entry.anchor)}
      </span>
      <span className="truncate text-fg-4">{pair ? `at ${pair}` : first}</span>
      {state && (
        <span className="inline-flex items-center gap-1 text-fg-4" title={stateTitle}>
          <StateIcon state={state} size={9} />
          {lastActor && <AuthorGlyph kind={authorKind(lastActor)} size={9} />}
        </span>
      )}
    </button>
  );
}
