import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Copy, History, SquareArrowOutUpRight } from "lucide-react";
import { DiffView, DiffModeEnum, SplitSide } from "@git-diff-view/react";
import { useTheme } from "../../hooks/useTheme";
import type { DiffFile, ReviewSide } from "../../types";
import { fetchRunFileAtRef } from "../../api";
import { baseName, fileHunks, filePath, langOf, statusLetter } from "../../lib/runRefs";
import type { RefPair, ViewMode } from "../../lib/runRefs";
import type { Anchor, CommentState, ReviewEntry } from "../../lib/reviewComments";
import { commentState, entryAt, plural, stateCounts, wipKey } from "../../lib/reviewComments";
import type { ReviewComment } from "../../types";
import CommentEditor from "./CommentEditor";
import CommentCard, { Badge, StateIcon } from "./CommentCard";

/**
 * One file of the Review page (#749): a sticky header (chevron, status letter,
 * `dir/` + basename, `+a −d`, comment badges, copy path, open at destination
 * ref) and the third-party diff body — split or unified, with GitHub-style
 * `↑ ⇕ ↓` context expansion in every hunk separator.
 *
 * The daemon's structured diff carries the hunks; the component needs the
 * **full content at each ref** to expand context. It is fetched lazily — the
 * first time the card scrolls into view — from `GET /runs/<id>/file`, and the
 * card renders its hunks meanwhile (without the expanders, which appear once
 * the content is in). A side that does not exist at its ref (an added file's
 * old side, a deleted file's new side) is not fetched: the component composes
 * it from the diff.
 *
 * Review comments (#750): every entry of this file (draft or sent) occupies the
 * library's **extend slot** under its line — `extendData[side][line]` — and the
 * `+` widget on a line number opens the editor (widget row) for a line without
 * a comment, re-opens the draft for edit when there is one, and only toasts on
 * a sent (immutable) comment: **one comment per line**. Anchored lines carry a
 * dot on the number and a 2px bar on the content cell, through a per-card
 * `<style>` scoped to the card's id: amber for a draft; for a sent comment the
 * colour follows its state (#751) — blue open, amber resolution proposed, green
 * resolved — so a scan of the file shows what is still open. The header's
 * badges are icon + count per state (proposed first). Hiding resolved comments
 * (the page's eye toggle) drops their cards from the extend slots but keeps the
 * green dot on the line.
 *
 * #752 (ADR-0067 §5): a sent entry the daemon re-mapped sits at its **reported**
 * line like any other. An **outdated** entry (its line changed on the displayed
 * destination) is not inline — guessing a nearby line would lie — but in an
 * "N outdated comments" group at the top of the card, collapsed, with its
 * original hunk inside; the header gets a history glyph. `showOutdated` hides
 * those cards, never the counts. Drafts are never re-mapped: a draft whose line
 * no longer exists in the displayed hunks is listed above the body with a small
 * amber "line changed" hint, still editable, sendable, deletable.
 */

/** What the page hands every card to act on comments (#750). */
export interface ReviewCommentsApi {
  /** Draft key currently opened for edit (its card becomes the editor). */
  editingKey: string | null;
  /** Draft keys travelling in a send right now. */
  sendingKeys: ReadonlySet<string>;
  /** The in-flight send has to start the manager first. */
  startingManager: boolean;
  /** `Fork point → Run tip` for the sent card's footer. */
  pairLabel: string;
  sendDisabledReason: string | null;
  saveNew: (anchor: Anchor, text: string) => void;
  sendNew: (anchor: Anchor, text: string) => void;
  editDraft: (key: string) => void;
  updateDraft: (key: string, text: string) => void;
  cancelEdit: () => void;
  deleteDraft: (key: string) => void;
  sendDraft: (key: string) => void;
  /** The `+` landed on a sent comment: nothing to open, the page explains. */
  sentLineClicked: () => void;
  /** #751: the human's decision on a sent comment. */
  resolve: (comment: ReviewComment) => void;
  reopen: (comment: ReviewComment) => void;
  /** Comment ids with a Resolve / Reopen in flight. */
  deciding: ReadonlySet<string>;
  /** Unread replies of a comment in this browser (0 = all seen). */
  unreadOf: (comment: ReviewComment) => number;
  /** Comment ids whose reply just landed live (outline flash). */
  flashing: ReadonlySet<string>;
  markSeen: (comment: ReviewComment) => void;
  /** Eye toggle: false hides resolved cards (their anchor dot stays). */
  showResolved: boolean;
  /** #752: false hides the outdated cards (counts stay). */
  showOutdated: boolean;
  /** #752: the label of the ref a sent comment was written against (its anchored side). */
  writtenOnLabel: (comment: ReviewComment) => string;
}

interface Props {
  runId: string;
  file: DiffFile;
  pair: RefPair;
  view: ViewMode;
  collapsed: boolean;
  onToggle: () => void;
  /** Lets the page track the card for scroll-spy and "click a file → scroll". */
  registerEl: (path: string, el: HTMLDivElement | null) => void;
  /** This file's comments for the current pair (drafts + sent). */
  entries?: ReviewEntry[];
  comments?: ReviewCommentsApi;
}

type Contents =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; old: string | null; new: string | null }
  | { kind: "failed" };

const LETTER_CLASS: Record<"A" | "M" | "D" | "R", string> = {
  A: "text-st-done",
  M: "text-st-await",
  D: "text-st-failed",
  R: "text-edit-tint",
};

const NO_ENTRIES: ReviewEntry[] = [];

/** Line dot / bar colour per sent-comment state (#751). */
const STATE_VAR: Record<CommentState, string> = {
  open: "var(--color-st-running)",
  proposed: "var(--color-st-await)",
  resolved: "var(--color-st-done)",
};

function sideOf(s: SplitSide): ReviewSide {
  return s === SplitSide.old ? "old" : "new";
}

export default function ReviewFileCard({
  runId,
  file,
  pair,
  view,
  collapsed,
  onToggle,
  registerEl,
  entries = NO_ENTRIES,
  comments,
}: Props) {
  const { resolved } = useTheme();
  const path = filePath(file);
  const letter = statusLetter(file);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [contents, setContents] = useState<Contents>({ kind: "idle" });
  const [copied, setCopied] = useState(false);
  const cssId = `rc-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;

  const pureRename = file.status === "renamed" && file.hunks.length === 0;
  const hasBody = !file.binary && !pureRename && file.hunks.length > 0;

  useEffect(() => {
    registerEl(path, rootRef.current);
    return () => registerEl(path, null);
  }, [path, registerEl]);

  // The page keys each card by (pair, path): a pair change remounts the card,
  // so the lazily fetched content never survives the refs it was fetched at.

  const load = useCallback(() => {
    if (!hasBody) return;
    setContents({ kind: "loading" });
    const wantOld = file.old_path && file.status !== "added";
    const wantNew = file.new_path && file.status !== "deleted";
    const oldP = wantOld ? fetchRunFileAtRef(runId, file.old_path!, pair.from) : Promise.resolve(null);
    const newP = wantNew ? fetchRunFileAtRef(runId, file.new_path!, pair.to) : Promise.resolve(null);
    Promise.all([oldP, newP])
      .then(([o, n]) => setContents({ kind: "ready", old: o, new: n }))
      .catch(() => setContents({ kind: "failed" }));
  }, [hasBody, runId, file.old_path, file.new_path, file.status, pair.from, pair.to]);

  // Fetch when the card first becomes visible (and only while expanded).
  useEffect(() => {
    if (contents.kind !== "idle" || collapsed || !hasBody) return;
    const el = rootRef.current;
    if (!el) return;
    if (typeof IntersectionObserver === "undefined") {
      // jsdom / very old browsers: fetch right away, off the effect body.
      const t = window.setTimeout(load, 0);
      return () => window.clearTimeout(t);
    }
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          load();
        }
      },
      { rootMargin: "400px 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [contents.kind, collapsed, hasBody, load]);

  const hunks = useMemo(() => fileHunks(file), [file]);
  const data = useMemo(() => {
    const ready = contents.kind === "ready" ? contents : null;
    return {
      oldFile: {
        fileName: file.old_path ?? file.new_path ?? "",
        fileLang: langOf(file.old_path ?? file.new_path),
        content: ready?.old ?? "",
      },
      newFile: {
        fileName: file.new_path ?? file.old_path ?? "",
        fileLang: langOf(file.new_path ?? file.old_path),
        content: ready?.new ?? "",
      },
      hunks,
    };
  }, [file.old_path, file.new_path, hunks, contents]);

  // --- Comments (#750) --------------------------------------------------------
  const showResolved = comments?.showResolved ?? true;
  const showOutdated = comments?.showOutdated ?? true;
  /** #752: outdated entries live in the group at the top, never in the extend slots. */
  const outdatedEntries = useMemo(() => entries.filter((e) => e.kind === "sent" && e.outdated), [entries]);
  /** #752: drafts anchored on a line the displayed hunks no longer show — listed above the body. */
  const orphanDrafts = useMemo<ReviewEntry[]>(() => {
    const drafts = entries.filter((e) => e.kind === "draft");
    if (drafts.length === 0) return [];
    const present = new Set<string>();
    for (const h of file.hunks) {
      for (const l of h.lines) {
        if (l.old_no != null) present.add(`old:${l.old_no}`);
        if (l.new_no != null) present.add(`new:${l.new_no}`);
      }
    }
    return drafts.filter((e) => !present.has(`${e.anchor.side}:${e.anchor.line}`));
  }, [entries, file.hunks]);
  const inlineEntries = useMemo(
    () => entries.filter((e) => !(e.kind === "sent" && e.outdated) && !orphanDrafts.includes(e)),
    [entries, orphanDrafts],
  );
  const extendData = useMemo(() => {
    const oldFile: Record<string, { data: ReviewEntry }> = {};
    const newFile: Record<string, { data: ReviewEntry }> = {};
    for (const e of inlineEntries) {
      if (!showResolved && e.kind === "sent" && e.comment.status === "resolved") continue;
      const slot = e.anchor.side === "old" ? oldFile : newFile;
      // Sent wins over a stale draft on the same line (entries are sorted so).
      if (!slot[String(e.anchor.line)]) slot[String(e.anchor.line)] = { data: e };
    }
    return { oldFile, newFile };
  }, [inlineEntries, showResolved]);

  const nDrafts = entries.filter((e) => e.kind === "draft").length;
  const nSent = entries.length - nDrafts;
  const nOutdated = outdatedEntries.length;
  const sentStates = useMemo(
    () => stateCounts(entries.flatMap((e) => (e.kind === "sent" ? [e.comment] : []))),
    [entries],
  );

  const renderCard = (entry: ReviewEntry) => {
    if (!comments) return null;
    const key = entry.kind === "draft" ? entry.draft.key : null;
    const sent = entry.kind === "sent" ? entry.comment : null;
    return (
      <CommentCard
        entry={entry}
        sending={key !== null && comments.sendingKeys.has(key)}
        startingManager={comments.startingManager}
        pairLabel={comments.pairLabel}
        sendDisabledReason={comments.sendDisabledReason}
        onEdit={() => key && comments.editDraft(key)}
        onDelete={() => key && comments.deleteDraft(key)}
        onSend={() => key && comments.sendDraft(key)}
        unread={sent ? comments.unreadOf(sent) : 0}
        flash={sent ? comments.flashing.has(sent.id) : false}
        deciding={sent ? comments.deciding.has(sent.id) : false}
        onResolve={sent ? () => comments.resolve(sent) : undefined}
        onReopen={sent ? () => comments.reopen(sent) : undefined}
        onSeen={sent ? () => comments.markSeen(sent) : undefined}
        outdated={entry.kind === "sent" && entry.outdated && sent ? { writtenOn: comments.writtenOnLabel(sent) } : undefined}
        movedFrom={entry.kind === "sent" ? entry.movedFrom : undefined}
      />
    );
  };

  /** Per-card CSS: dot + bar on anchored lines, `+` hidden where a comment sits. */
  const anchorCss = useMemo(() => {
    if (inlineEntries.length === 0) return "";
    const rules: string[] = [];
    for (const e of inlineEntries) {
      const color = e.kind === "draft" ? "var(--color-st-await)" : STATE_VAR[commentState(e.comment)];
      const { side, line } = e.anchor;
      // Split: `td.diff-line-<side>-num > span[data-line-num]`, content cell next.
      const num = `#${cssId} td.diff-line-${side}-num:has(> span[data-line-num="${line}"])`;
      rules.push(`${num}::before{content:"";position:absolute;left:5px;top:7px;width:6px;height:6px;border-radius:50%;background:${color}}`);
      rules.push(`${num} + td.diff-line-${side}-content{box-shadow:inset 2px 0 0 ${color}}`);
      rules.push(`${num} [data-add-widget]{display:none}`);
      // Unified: both numbers on one row, `span[data-line-<side>-num]`.
      const row = `#${cssId} tr.diff-line:has(span[data-line-${side}-num="${line}"])`;
      rules.push(`${row} td.diff-line-num{position:relative}`);
      rules.push(`${row} td.diff-line-num::before{content:"";position:absolute;left:5px;top:7px;width:6px;height:6px;border-radius:50%;background:${color}}`);
      rules.push(`${row} td.diff-line-content{box-shadow:inset 2px 0 0 ${color}}`);
      rules.push(`${row} [data-add-widget]{display:none}`);
    }
    return rules.join("\n");
  }, [inlineEntries, cssId]);

  const copyPath = () => {
    navigator.clipboard?.writeText(path).then(
      () => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1200);
      },
      () => {},
    );
  };

  const dir = path.includes("/") ? path.slice(0, path.lastIndexOf("/") + 1) : "";
  const openHref =
    file.new_path && file.status !== "deleted"
      ? `/runs/${encodeURIComponent(runId)}/file?path=${encodeURIComponent(file.new_path)}&ref=${encodeURIComponent(pair.to)}`
      : null;

  return (
    <div
      ref={rootRef}
      id={cssId}
      data-testid="review-file"
      data-path={path}
      data-collapsed={collapsed}
      data-content={contents.kind}
      data-drafts={nDrafts}
      data-sent={nSent}
      data-proposed={sentStates.proposed}
      data-resolved={sentStates.resolved}
      data-outdated={nOutdated}
      className="mx-3 my-2.5 overflow-hidden rounded-md border border-line bg-bg-2"
    >
      {anchorCss && <style>{anchorCss}</style>}
      <div
        role="button"
        tabIndex={0}
        aria-expanded={!collapsed}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
        data-testid="review-file-header"
        className={`sticky top-0 z-10 flex cursor-pointer select-none items-center gap-2 bg-bg-2 px-2.5 py-[5px] ${
          collapsed ? "" : "border-b border-line"
        }`}
        style={{ fontSize: "11px" }}
      >
        <span className="text-fg-4">
          {collapsed ? <ChevronRight size={11} /> : <ChevronDown size={11} />}
        </span>
        <span className={`w-[14px] text-center font-mono font-semibold ${LETTER_CLASS[letter]}`} style={{ fontSize: "9.5px" }}>
          {letter}
        </span>
        <span className="truncate font-mono" style={{ fontSize: "10.5px" }}>
          {file.status === "renamed" && file.old_path && (
            <span className="text-fg-4">{file.old_path} → </span>
          )}
          <span className="text-fg-4">{dir}</span>
          <span className="font-medium text-fg">{baseName(path)}</span>
        </span>
        {file.binary ? (
          <span className="text-fg-4" style={{ fontSize: "10px" }}>
            binary
          </span>
        ) : pureRename ? (
          <span className="text-fg-4" style={{ fontSize: "10px" }}>
            renamed
          </span>
        ) : (
          <span className="font-mono" style={{ fontSize: "10px" }}>
            <span className="text-st-done">+{file.additions}</span> <span className="text-st-failed">−{file.deletions}</span>
          </span>
        )}
        <span className="ml-auto flex items-center gap-1.5" onClick={(e) => e.stopPropagation()}>
          {nDrafts > 0 && <Badge kind="draft">✎ {plural(nDrafts, "draft")}</Badge>}
          {nOutdated > 0 && (
            <span
              className="inline-flex items-center gap-0.5 text-st-stale"
              style={{ fontSize: "10px" }}
              title={`${plural(nOutdated, "outdated comment")} — the line changed on this destination`}
              data-testid="review-file-outdated"
            >
              <History size={10} /> {nOutdated}
            </span>
          )}
          {sentStates.proposed > 0 && (
            <span className="inline-flex items-center gap-0.5 text-st-await" style={{ fontSize: "10px" }} title={`${sentStates.proposed} resolution proposed`} data-testid="review-file-proposed">
              <StateIcon state="proposed" size={10} /> {sentStates.proposed}
            </span>
          )}
          {sentStates.open > 0 && (
            <span className="inline-flex items-center gap-0.5 text-fg-3" style={{ fontSize: "10px" }} title={`${sentStates.open} open`} data-testid="review-file-open">
              <StateIcon state="open" size={10} /> {sentStates.open}
            </span>
          )}
          {sentStates.resolved > 0 && (
            <span className="inline-flex items-center gap-0.5 text-st-done" style={{ fontSize: "10px" }} title={`${sentStates.resolved} resolved`} data-testid="review-file-resolved">
              <StateIcon state="resolved" size={10} /> {sentStates.resolved}
            </span>
          )}
          <button
            type="button"
            onClick={copyPath}
            title={copied ? "Copied" : "Copy path"}
            data-testid="review-copy-path"
            className="grid h-5 w-[22px] cursor-pointer place-items-center rounded text-fg-4 hover:bg-bg-3 hover:text-fg-2"
          >
            <Copy size={11} />
          </button>
          {openHref && (
            <a
              href={openHref}
              target="_blank"
              rel="noreferrer"
              title="Open file at destination ref"
              data-testid="review-open-file"
              className="grid h-5 w-[22px] place-items-center rounded text-fg-4 hover:bg-bg-3 hover:text-fg-2"
            >
              <SquareArrowOutUpRight size={11} />
            </a>
          )}
        </span>
      </div>

      {!collapsed && (
        <div data-testid="review-file-body" className="overflow-x-auto">
          {nOutdated > 0 && comments && (
            <div data-testid="review-outdated-group" data-hidden={showOutdated ? undefined : "true"} className="border-b border-line pb-1">
              <div className="mx-3 mt-2 flex items-center gap-2 text-fg-3" style={{ fontSize: "10.5px" }}>
                <span className="inline-flex items-center gap-1 text-st-stale">
                  <History size={10} /> {plural(nOutdated, "outdated comment")}
                </span>
                <span className="h-px flex-1 bg-line" />
                <span
                  className="cursor-help"
                  title="Its line changed between the destination it was written on and this one. Still answerable, still resolvable."
                >
                  what is this?
                </span>
              </div>
              {showOutdated && outdatedEntries.map((e) => <div key={e.kind === "sent" ? e.comment.id : e.anchor.line}>{renderCard(e)}</div>)}
            </div>
          )}
          {orphanDrafts.length > 0 && comments && (
            <div data-testid="review-orphan-drafts" className="border-b border-line pb-1">
              <div className="mx-3 mt-2 flex items-center gap-2 text-st-await" style={{ fontSize: "10.5px" }}>
                ✎ {plural(orphanDrafts.length, "draft")} on a line that changed
                <span className="h-px flex-1 bg-line" />
                <span className="cursor-help text-fg-4" title="Drafts are never re-mapped: the line this draft was written under is no longer in the diff. Edit, send or delete it.">
                  line changed
                </span>
              </div>
              {orphanDrafts.map((e) =>
                e.kind === "draft" && comments.editingKey === e.draft.key ? (
                  <div key={e.draft.key} className="mx-3 my-2">
                    <CommentEditor
                      anchor={e.anchor}
                      initial={e.draft.text}
                      wipKey={wipKey(runId, e.anchor, pair)}
                      sendDisabledReason={comments.sendDisabledReason}
                      onSave={(text) => comments.updateDraft(e.draft.key, text)}
                      onSend={(text) => {
                        comments.updateDraft(e.draft.key, text);
                        comments.sendDraft(e.draft.key);
                      }}
                      onCancel={comments.cancelEdit}
                    />
                  </div>
                ) : (
                  <div key={e.kind === "draft" ? e.draft.key : e.anchor.line}>{renderCard(e)}</div>
                ),
              )}
            </div>
          )}
          {file.binary ? (
            <div className="px-4 py-[18px] text-center text-fg-4" style={{ fontSize: "11px" }}>
              Binary file, not shown
            </div>
          ) : pureRename ? (
            <div className="px-4 py-[18px] text-center text-fg-4" style={{ fontSize: "11px" }}>
              Renamed without changes
            </div>
          ) : file.hunks.length === 0 ? (
            <div className="px-4 py-[18px] text-center text-fg-4" style={{ fontSize: "11px" }}>
              No content changes
            </div>
          ) : (
            <DiffView<ReviewEntry>
              data={data}
              extendData={extendData}
              diffViewMode={view === "split" ? DiffModeEnum.Split : DiffModeEnum.Unified}
              // #759: the library ships its own light/dark syntax theme; handing it the
              // resolved theme keeps code colours legible. `index.css` then maps the
              // wrapper's surface variables onto PDO tokens for both.
              diffViewTheme={resolved}
              diffViewFontSize={11}
              diffViewHighlight={false}
              diffViewWrap={false}
              diffViewAddWidget
              renderWidgetLine={({ side, lineNumber, onClose }) => {
                if (!comments) return null;
                const anchor: Anchor = { path, side: sideOf(side), line: lineNumber };
                const existing = entryAt(inlineEntries, anchor);
                if (existing) {
                  return (
                    <WidgetRedirect
                      onMount={() => {
                        if (existing.kind === "draft") comments.editDraft(existing.draft.key);
                        else comments.sentLineClicked();
                        onClose();
                      }}
                    />
                  );
                }
                return (
                  <CommentEditor
                    anchor={anchor}
                    wipKey={wipKey(runId, anchor, pair)}
                    sendDisabledReason={comments.sendDisabledReason}
                    onSave={(text) => {
                      comments.saveNew(anchor, text);
                      onClose();
                    }}
                    onSend={(text) => {
                      comments.sendNew(anchor, text);
                      onClose();
                    }}
                    onCancel={onClose}
                  />
                );
              }}
              renderExtendLine={({ data: entry }) => {
                // The library probes both sides of a line; only the anchored one carries data.
                if (!comments || !entry) return null;
                if (entry.kind === "draft" && comments.editingKey === entry.draft.key) {
                  return (
                    <CommentEditor
                      anchor={entry.anchor}
                      initial={entry.draft.text}
                      wipKey={wipKey(runId, entry.anchor, pair)}
                      sendDisabledReason={comments.sendDisabledReason}
                      onSave={(text) => comments.updateDraft(entry.draft.key, text)}
                      onSend={(text) => {
                        comments.updateDraft(entry.draft.key, text);
                        comments.sendDraft(entry.draft.key);
                      }}
                      onCancel={comments.cancelEdit}
                    />
                  );
                }
                return renderCard(entry);
              }}
            />
          )}
          {contents.kind === "failed" && hasBody && (
            <div className="border-t border-line px-3 py-1 text-fg-4" style={{ fontSize: "10px" }}>
              Full file content unavailable at these refs — context expansion disabled for this file.
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * The `+` landed on a line that already carries a comment: the widget row must
 * close itself and hand over (edit the draft / explain the sent one). Done in an
 * effect — never during render — because closing is the library's state.
 */
function WidgetRedirect({ onMount }: { onMount: () => void }) {
  const ran = useRef(false);
  useEffect(() => {
    if (ran.current) return;
    ran.current = true;
    onMount();
  }, [onMount]);
  return null;
}
