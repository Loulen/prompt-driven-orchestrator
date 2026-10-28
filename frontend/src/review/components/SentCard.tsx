import { useEffect, useRef, useState } from "react";
import { Check, ChevronRight, CornerDownRight, History, Lock, MessageSquare, Undo2 } from "lucide-react";
import { useActorDisplay } from "../../shared/actors/store";
import type { CommentState } from "../model/commentStatus";
import { authorKind, authorLabel, commentState, firstWords, footerStatus, plural, relativeTime } from "../model/commentStatus";
import { parseExcerpt } from "../model/entries";
import type { ReviewComment } from "../../shared/generated/ReviewComment";
import { AuthorGlyph, AuthorIcon, StateIcon } from "./commentBits";
import { DecisionButton, FooterStatusView } from "./commentFooter";
import ReplyRow from "./ReplyRow";

/**
 * A **sent** review comment (#750, #751, #752): its thread, its state, its
 * decision, and — once re-mapped — whether it moved or went outdated. See
 * `CommentCard` for the whole story.
 */

const STATE_COLOR: Record<CommentState, string> = {
  open: "var(--color-st-running)",
  proposed: "var(--color-st-await)",
  resolved: "var(--color-st-done)",
};

/** How long a card has to stay in view before its replies count as seen. */
const SEEN_DWELL_MS = 1500;

export default function SentCard({
  comment: c,
  label,
  body,
  pairLabel,
  unread,
  flash,
  deciding,
  onResolve,
  onReopen,
  onSeen,
  outdated,
  movedFrom,
}: {
  comment: ReviewComment;
  label: string;
  body: (text: string) => React.ReactNode;
  pairLabel: string;
  unread: number;
  flash: boolean;
  deciding: boolean;
  onResolve?: () => void;
  onReopen?: () => void;
  onSeen?: () => void;
  outdated?: { writtenOn: string };
  movedFrom?: number;
}) {
  const state = commentState(c);
  const replies = c.replies ?? [];
  const footer = footerStatus(c);
  // Resolved ⇒ collapsed (GitHub-like), until the header is clicked. The
  // expansion is keyed on the resolution it was opened for, so a comment
  // resolved again later collapses again without an effect. #752: an outdated
  // card collapses the same way (keyed on "outdated" while it stays so).
  const [expandedFor, setExpandedFor] = useState<string | null>(null);
  const collapsible = state === "resolved" || !!outdated;
  const resolutionKey = outdated ? `outdated:${c.resolved_at ?? ""}` : (c.resolved_at ?? "resolved");
  const collapsed = collapsible && expandedFor !== resolutionKey;
  const setExpanded = (fn: (open: boolean) => boolean) =>
    setExpandedFor((prev) => (fn(prev === resolutionKey) ? resolutionKey : null));
  const originalHunk = outdated ? parseExcerpt(c.excerpt) : [];

  // Seen: hover, or in view for a moment. Only while something is unread.
  const rootRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (unread === 0 || !onSeen) return;
    const el = rootRef.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    let timer: number | undefined;
    const io = new IntersectionObserver((entries) => {
      const visible = entries.some((e) => e.isIntersecting);
      window.clearTimeout(timer);
      if (visible) timer = window.setTimeout(() => onSeen(), SEEN_DWELL_MS);
    });
    io.observe(el);
    return () => {
      window.clearTimeout(timer);
      io.disconnect();
    };
  }, [unread, onSeen]);

  const firstUnread = replies.length - unread;
  const resolver = c.resolved_by ?? "user";
  // CORE-63: the names extensions give the Actors who resolved or proposed.
  const resolverName = useActorDisplay(c.resolved_by_actor)?.name;
  const proposerName = useActorDisplay(footer.kind === "proposed" ? footer.actor : null)?.name;
  const stateTitle =
    state === "resolved"
      ? `Resolved by ${authorLabel(resolver, resolverName)}`
      : state === "proposed"
        ? `Resolution proposed by ${authorLabel(footer.kind === "proposed" ? footer.author : "agent", proposerName)}`
        : "Sent · open";

  return (
    <div
      ref={rootRef}
      onMouseEnter={unread > 0 ? onSeen : undefined}
      className={`group my-2 ml-3 mr-3 max-w-[720px] overflow-hidden rounded-md border border-line-strong bg-surface-1 font-sans ${
        flash ? "pdo-review-flash" : ""
      }`}
      style={{ fontSize: "11px", whiteSpace: "normal" }}
      data-testid="review-comment"
      data-state={c.status}
      data-review-state={state}
      data-collapsed={collapsed ? "true" : undefined}
      data-unread={unread > 0 ? unread : undefined}
      data-anchor={label}
      data-comment-id={c.id}
      data-outdated={outdated ? "true" : undefined}
      data-moved-from={movedFrom}
    >
      <div
        role={collapsible ? "button" : undefined}
        tabIndex={collapsible ? 0 : undefined}
        aria-expanded={collapsible ? !collapsed : undefined}
        onClick={collapsible ? () => setExpanded((e) => !e) : undefined}
        onKeyDown={
          collapsible
            ? (e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  setExpanded((x) => !x);
                }
              }
            : undefined
        }
        data-testid="review-comment-header"
        className={`flex items-center gap-2 bg-surface-2 px-2.5 py-[5px] text-text-3 ${collapsed ? "cursor-pointer" : "border-b border-line"}`}
        style={{ fontSize: "10.5px", borderLeft: `2px solid ${outdated ? "var(--color-st-skipped)" : STATE_COLOR[state]}` }}
      >
        {outdated && (
          <span className="inline-flex items-center text-st-skipped" title="Outdated — the line changed since this comment was written" data-testid="review-comment-outdated">
            <History size={11} />
          </span>
        )}
        <StateIcon state={state} title={stateTitle} />
        <AuthorIcon author={c.author} actor={c.actor} />
        {outdated ? (
          <span
            className="font-mono text-text-3"
            style={{ fontSize: "10px" }}
            title={`Written on ${outdated.writtenOn}, at ${label}`}
            data-testid="review-comment-written-on"
          >
            {c.side === "old" ? "L" : "R"}
            {c.line} <span className="font-sans text-text-3">on</span> {outdated.writtenOn}
          </span>
        ) : (
          <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>
            {label}
          </span>
        )}
        {movedFrom !== undefined && !outdated && (
          <span
            className="inline-flex cursor-help items-center gap-0.5 rounded px-1 font-mono text-text-3 hover:bg-hover hover:text-text-2"
            style={{ fontSize: "10px" }}
            title={`Written at ${c.side === "old" ? "L" : "R"}${movedFrom} on ${pairLabel.split(" → ")[c.side === "old" ? 0 : 1] ?? pairLabel}; the line is unchanged, so it follows it here.`}
            data-testid="review-comment-moved-from"
          >
            <CornerDownRight size={10} /> from {c.side === "old" ? "L" : "R"}
            {movedFrom}
          </span>
        )}
        <span className="text-text-3">· {relativeTime(c.sent_at)}</span>
        {collapsible && (
          <span className={`text-text-3 transition-transform ${collapsed ? "" : "rotate-90"}`} aria-hidden>
            <ChevronRight size={10} />
          </span>
        )}
        {collapsed && (
          <span className="flex min-w-0 items-center gap-1.5 truncate text-text-3" style={{ fontSize: "10px" }} data-testid="review-comment-summary">
            <span className="truncate">“{firstWords(c.text)}”</span>
            {replies.length > 0 && (
              <span className="inline-flex items-center gap-0.5 text-text-3" title={plural(replies.length, "reply").replace("replys", "replies")}>
                <MessageSquare size={10} /> {replies.length}
              </span>
            )}
          </span>
        )}
        <span className="ml-auto inline-flex items-center gap-1 text-text-3">
          {state === "resolved" ? (
            <span className="inline-flex items-center gap-0.5 text-text-3" title={`Resolved by ${authorLabel(resolver, resolverName)}`} data-testid="review-comment-resolved-by">
              <AuthorGlyph kind={authorKind(resolver)} size={10} />
              <Check size={10} className="text-st-done" />
            </span>
          ) : (
            <span
              className="grid h-5 w-[22px] cursor-help place-items-center"
              title="Sent comments are Run events: immutable, readable post-mortem. Replies and Resolve / Reopen are events too."
              data-testid="review-comment-lock"
            >
              <Lock size={11} />
            </span>
          )}
        </span>
      </div>

      {!collapsed && (
        <>
          {outdated && (
            <div className="border-b border-line bg-surface-0" data-testid="review-comment-original-hunk">
              <div className="flex items-center gap-1.5 border-b border-line bg-surface-2 px-2.5 py-1 text-text-3" style={{ fontSize: "10px" }}>
                <History size={10} />
                Original hunk ·{" "}
                <span className="rounded border border-line-strong bg-surface-2 px-1.5 font-mono text-text-2" style={{ fontSize: "10px" }}>
                  {outdated.writtenOn}
                </span>{" "}
                · this is what the comment was written against
              </div>
              {originalHunk.length > 0 ? (
                <div className="font-mono" style={{ fontSize: "10.5px", lineHeight: "18px" }}>
                  {originalHunk.map((l) => (
                    <div
                      key={l.no}
                      className="grid grid-cols-[44px_1fr]"
                      data-testid="review-original-line"
                      data-marked={l.marked ? "true" : undefined}
                      style={l.marked ? { background: "color-mix(in oklab, var(--color-st-skipped) 14%, transparent)", boxShadow: "inset 3px 0 0 var(--color-st-skipped)" } : undefined}
                    >
                      <span className="select-none pr-2 text-right text-text-3" style={{ fontSize: "10px" }}>
                        {l.no}
                      </span>
                      <span className="whitespace-pre pl-2.5 text-text-2">{l.text}</span>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="px-2.5 py-1.5 text-text-3" style={{ fontSize: "10px" }}>
                  No excerpt was stored with this comment.
                </div>
              )}
            </div>
          )}
          {body(c.text)}
          {replies.length > 0 && (
            <div className="border-t border-line bg-surface-0" data-testid="review-comment-thread">
              {replies.map((r, i) => (
                <ReplyRow key={`${r.at}-${i}`} reply={r} first={i === 0} unread={i >= firstUnread} />
              ))}
            </div>
          )}
          {/* One line whatever the width: the pair label truncates, the id and the status never wrap. */}
          <div className="flex items-center gap-2 whitespace-nowrap border-t border-line px-2.5 py-[5px] text-text-3" style={{ fontSize: "10px" }}>
            <span className="shrink-0 font-mono" data-testid="review-comment-id">
              {c.id}
            </span>
            <span className="min-w-0 truncate" title={pairLabel}>· {pairLabel}</span>
            {outdated && (
              <span className="inline-flex items-center gap-1 text-st-skipped" title="The line this comment points at changed on the displayed destination">
                <History size={10} /> line changed
              </span>
            )}
            <span className="ml-auto inline-flex shrink-0 items-center gap-1.5" data-testid="review-comment-status" data-kind={footer.kind}>
              <FooterStatusView status={footer} />
              {state === "proposed" ? (
                <>
                  <DecisionButton
                    primary
                    onClick={onResolve}
                    disabled={deciding || !onResolve}
                    title={`Confirm: mark ${c.id} resolved (you keep Reopen)`}
                    testId="review-comment-resolve"
                  >
                    <Check size={11} /> Resolve
                  </DecisionButton>
                  <DecisionButton
                    onClick={onReopen}
                    disabled={deciding || !onReopen}
                    title="Decline the proposal, keep the comment open for the agent"
                    testId="review-comment-reopen"
                  >
                    <Undo2 size={11} /> Reopen
                  </DecisionButton>
                </>
              ) : state === "resolved" ? (
                <DecisionButton ghost onClick={onReopen} disabled={deciding || !onReopen} title="Reopen this comment" testId="review-comment-reopen">
                  <Undo2 size={11} /> Reopen
                </DecisionButton>
              ) : footer.kind !== "awaiting" ? (
                <DecisionButton ghost onClick={onResolve} disabled={deciding || !onResolve} title="Resolve this comment yourself" testId="review-comment-resolve">
                  <Check size={11} /> Resolve
                </DecisionButton>
              ) : null}
            </span>
          </div>
        </>
      )}
    </div>
  );
}
