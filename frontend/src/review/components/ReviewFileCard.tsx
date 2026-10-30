import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { DiffView, DiffModeEnum, SplitSide } from "@git-diff-view/react";
import { useTheme } from "../../hooks/useTheme";
import type { DiffFile } from "../../shared/generated/DiffFile";
import type { ReviewSide } from "../../shared/generated/ReviewSide";
import { fetchRunFileAtRef } from "../../run/api/artifacts";
import { fileHunks, filePath, langOf } from "../../run/model/runRefs";
import type { RefPair, ViewMode } from "../../run/model/runRefs";
import type { Anchor } from "../model/drafts";
import type { ReviewEntry } from "../model/entries";
import { stateCounts } from "../model/commentStatus";
import { anchorCssOf, extendDataOf, orphanDraftsOf } from "../model/fileComments";
import { wipKey } from "../model/drafts";
import { entryAt } from "../model/entries";
import type { ReviewCommentsApi } from "../model/commentsApi";
import CommentEditor from "./CommentEditor";
import CommentCard from "./CommentCard";
import FileCommentGroups from "./FileCommentGroups";
import ReviewFileHeader from "./ReviewFileHeader";

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

interface Props {
  runId: string;
  file: DiffFile;
  pair: RefPair;
  view: ViewMode;
  collapsed: boolean;
  onToggle: () => void;
  /** Marked « Viewed » in this browser (dims the header). */
  viewed?: boolean;
  onViewed?: () => void;
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

const NO_ENTRIES: ReviewEntry[] = [];

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
  viewed = false,
  onViewed,
  registerEl,
  entries = NO_ENTRIES,
  comments,
}: Props) {
  const { mode } = useTheme();
  const path = filePath(file);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [contents, setContents] = useState<Contents>({ kind: "idle" });
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
  /** #752: outdated entries live in the group at the top, never in the extend slots. */
  const outdatedEntries = useMemo(() => entries.filter((e) => e.kind === "sent" && e.outdated), [entries]);
  /** #752: drafts anchored on a line the displayed hunks no longer show — listed above the body. */
  const orphanDrafts = useMemo<ReviewEntry[]>(() => orphanDraftsOf(entries, file.hunks), [entries, file.hunks]);
  const inlineEntries = useMemo(
    () => entries.filter((e) => !(e.kind === "sent" && e.outdated) && !orphanDrafts.includes(e)),
    [entries, orphanDrafts],
  );
  const extendData = useMemo(() => extendDataOf(inlineEntries, showResolved), [inlineEntries, showResolved]);

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

  const anchorCss = useMemo(() => anchorCssOf(inlineEntries, cssId), [inlineEntries, cssId]);


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
      data-viewed={viewed}
      className="rv-fc"
    >
      {anchorCss && <style>{anchorCss}</style>}
      <ReviewFileHeader
        runId={runId}
        file={file}
        pair={pair}
        collapsed={collapsed}
        onToggle={onToggle}
        viewed={viewed}
        onViewed={onViewed}
        nDrafts={nDrafts}
        nOutdated={nOutdated}
        sentStates={sentStates}
      />

      {!collapsed && (
        <div data-testid="review-file-body" className="rv-diff overflow-x-auto">
          {comments && (
            <FileCommentGroups
              runId={runId}
              pair={pair}
              comments={comments}
              outdatedEntries={outdatedEntries}
              orphanDrafts={orphanDrafts}
              renderCard={renderCard}
            />
          )}
          {file.binary ? (
            <div className="px-4 py-[18px] text-center text-text-3 text-xs">
              Binary file, not shown
            </div>
          ) : pureRename ? (
            <div className="px-4 py-[18px] text-center text-text-3 text-xs">
              Renamed without changes
            </div>
          ) : file.hunks.length === 0 ? (
            <div className="px-4 py-[18px] text-center text-text-3 text-xs">
              No content changes
            </div>
          ) : (
            <DiffView<ReviewEntry>
              data={data}
              extendData={extendData}
              diffViewMode={view === "split" ? DiffModeEnum.Split : DiffModeEnum.Unified}
              // #759: the library ships its own light/dark syntax theme; handing it the
              // mode keeps code colours legible. `index.css` then maps the
              // wrapper's surface variables onto PDO tokens for both.
              diffViewTheme={mode}
              diffViewFontSize={12}
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
                    pair={pair}
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
                      pair={pair}
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
            <div className="border-t border-line px-3 py-1 text-text-3 text-2xs">
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
