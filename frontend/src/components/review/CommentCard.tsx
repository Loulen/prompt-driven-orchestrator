import { useEffect, useRef, useState } from "react";
import { Bot, Check, CheckCircle2, ChevronRight, CornerDownRight, Cpu, History, Hourglass, Lock, MessageSquare, Pencil, Trash2, Undo2, User } from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { AuthorKind, CommentState, ReviewEntry } from "../../lib/reviewComments";
import { anchorLabel, authorKind, authorLabel, commentState, firstWords, footerStatus, parseExcerpt, plural, relativeTime } from "../../lib/reviewComments";
import type { ReviewComment } from "../../types";

/**
 * One review comment rendered under its diff line (#750): a **draft** card
 * (amber `✎ Draft`, edit / delete on hover, footer "Only in this browser until
 * sent" + `↗ Send to manager` always visible), a **sending** card (greyed,
 * spinner, "starting the manager…" when the send has to start it), or a
 * **sent** card. No edit, no delete on a sent comment: it is a Run event.
 *
 * #751 — the sent card is a **thread**: the agent's replies stack between the
 * body and the footer, one row per reply (author icon — person you, bot
 * manager, chip + id for a node —, time, an hourglass when the reply proposes a
 * resolution, a blue dot while unread). State is one icon plus a 2px left
 * border on the header: message-square open (blue), hourglass **Resolution
 * proposed** (amber), check-circle resolved (green); no filled pills, names
 * live in tooltips. The footer carries the decision: `Resolve` on an open or
 * proposed comment, `Reopen` on a resolved one — and on a proposal, where it
 * declines it and keeps the comment open for the agent. A resolved card
 * **collapses** to one line (icon · anchor · time · first words · reply count ·
 * resolved-by); its header click expands it. A reply that lands live flashes
 * the card's outline for two seconds.
 *
 * #752 — re-mapped comments (ADR-0067 §5). A **reported** card is identical to a
 * plain one; only when the line *number* differs does a discreet `↳ from R212`
 * chip sit in the header. An **outdated** card (its line changed on the displayed
 * destination) is collapsed by default — history glyph in the stale amber, state
 * icon, author, `R239 on <ref>`, time, first words, reply count, lock — and
 * expands to the **original hunk** first (the excerpt stored at send time, the
 * commented line in the stale tint, captioned with the ref it was written on),
 * then body, replies and the usual footer: outdated is a display property, not a
 * state, so Resolve / Reopen work as on any sent card.
 */

interface Props {
  entry: ReviewEntry;
  /** True while this draft travels in a send. */
  sending: boolean;
  /** The send is starting the manager first (message nuance while sending). */
  startingManager: boolean;
  /** Labels of the pair for the sent footer (`Fork point → Run tip`). */
  pairLabel: string;
  sendDisabledReason: string | null;
  onEdit: () => void;
  onDelete: () => void;
  onSend: () => void;
  /** #751 — sent cards only. Replies this browser has not seen yet (the last N rows). */
  unread?: number;
  /** A reply just landed over the WebSocket: outline flash. */
  flash?: boolean;
  /** A Resolve / Reopen request is in flight for this comment. */
  deciding?: boolean;
  onResolve?: () => void;
  onReopen?: () => void;
  /** The card was looked at (hovered, or in view for a moment): clear its unread dots. */
  onSeen?: () => void;
  /** #752 — sent cards only: the line changed on the displayed destination. `writtenOn` labels the ref it was written against. */
  outdated?: { writtenOn: string };
  /** #752 — sent cards only: the written line, when the reported number differs. */
  movedFrom?: number;
}

const REMARK_PLUGINS = [remarkGfm];

export default function CommentCard({
  entry,
  sending,
  startingManager,
  pairLabel,
  sendDisabledReason,
  onEdit,
  onDelete,
  onSend,
  unread = 0,
  flash = false,
  deciding = false,
  onResolve,
  onReopen,
  onSeen,
  outdated,
  movedFrom,
}: Props) {
  const label = anchorLabel(entry.anchor);
  const body = (text: string) => (
    <div
      className="artifact-markdown px-2.5 py-2 text-fg [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:list-disc [&_ul]:pl-5"
      style={{ fontSize: "11.5px", lineHeight: 1.5 }}
      data-testid="review-comment-body"
    >
      <Markdown remarkPlugins={REMARK_PLUGINS}>{text}</Markdown>
    </div>
  );

  if (entry.kind === "sent") {
    return (
      <SentCard
        comment={entry.comment}
        label={label}
        body={body}
        pairLabel={pairLabel}
        unread={unread}
        flash={flash}
        deciding={deciding}
        onResolve={onResolve}
        onReopen={onReopen}
        onSeen={onSeen}
        outdated={outdated}
        movedFrom={movedFrom}
      />
    );
  }

  const d = entry.draft;
  if (sending) {
    return (
      <div
        className="my-2 ml-3 mr-3 max-w-[720px] overflow-hidden rounded-md border border-line-strong bg-bg-2 font-sans opacity-70"
        style={{ fontSize: "11px", whiteSpace: "normal" }}
        data-testid="review-comment"
        data-state="sending"
        data-anchor={label}
      >
        <div className="flex items-center gap-2 border-b border-line bg-bg-3 px-2.5 py-[5px] text-fg-3" style={{ fontSize: "10.5px" }}>
          <Badge kind="sent">
            <Spinner /> Sending
          </Badge>
          <span className="font-medium text-fg-2">you</span>
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {label}
          </span>
          <span className="text-fg-4">· {startingManager ? "starting the manager…" : "handing over…"}</span>
        </div>
        {body(d.text)}
      </div>
    );
  }

  return (
    <div
      className="group my-2 ml-3 mr-3 max-w-[720px] overflow-hidden rounded-md border border-line-strong bg-bg-2 font-sans"
      style={{ fontSize: "11px", whiteSpace: "normal" }}
      data-testid="review-comment"
      data-state="draft"
      data-anchor={label}
    >
      <div className="flex items-center gap-2 border-b border-line bg-bg-3 px-2.5 py-[5px] text-fg-3" style={{ fontSize: "10.5px" }}>
        <Badge kind="draft">✎ Draft</Badge>
        <span className="font-medium text-fg-2">you</span>
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {label}
        </span>
        <span className="text-fg-4">· {relativeTime(d.updated_at)}</span>
        <span className="ml-auto inline-flex gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
          <button
            type="button"
            onClick={onEdit}
            title="Edit draft"
            data-testid="review-comment-edit"
            className="grid h-5 w-[22px] cursor-pointer place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg-2"
          >
            <Pencil size={11} />
          </button>
          <button
            type="button"
            onClick={onDelete}
            title="Delete draft"
            data-testid="review-comment-delete"
            className="grid h-5 w-[22px] cursor-pointer place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-st-failed"
          >
            <Trash2 size={11} />
          </button>
        </span>
      </div>
      {body(d.text)}
      <div className="flex items-center gap-2 border-t border-line px-2.5 py-[5px] text-fg-4" style={{ fontSize: "10px" }}>
        <span>Only in this browser until sent.</span>
        <span className="ml-auto">
          <button
            type="button"
            onClick={onSend}
            disabled={!!sendDisabledReason}
            title={sendDisabledReason ?? "Send this comment to the manager (one message)"}
            data-testid="review-comment-send"
            className="cursor-pointer rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 hover:bg-bg-4 hover:text-fg disabled:cursor-not-allowed disabled:opacity-45"
            style={{ fontSize: "10.5px" }}
          >
            ↗ Send to manager
          </button>
        </span>
      </div>
    </div>
  );
}

const STATE_COLOR: Record<CommentState, string> = {
  open: "var(--color-st-running)",
  proposed: "var(--color-st-await)",
  resolved: "var(--color-st-done)",
};

/** How long a card has to stay in view before its replies count as seen. */
const SEEN_DWELL_MS = 1500;

function SentCard({
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
  const stateTitle =
    state === "resolved"
      ? `Resolved by ${authorLabel(resolver)}`
      : state === "proposed"
        ? `Resolution proposed by ${authorLabel(footer.kind === "proposed" ? footer.author : "agent")}`
        : "Sent · open";

  return (
    <div
      ref={rootRef}
      onMouseEnter={unread > 0 ? onSeen : undefined}
      className={`group my-2 ml-3 mr-3 max-w-[720px] overflow-hidden rounded-md border border-line-strong bg-bg-2 font-sans ${
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
        className={`flex items-center gap-2 bg-bg-3 px-2.5 py-[5px] text-fg-3 ${collapsed ? "cursor-pointer" : "border-b border-line"}`}
        style={{ fontSize: "10.5px", borderLeft: `2px solid ${outdated ? "var(--color-st-stale)" : STATE_COLOR[state]}` }}
      >
        {outdated && (
          <span className="inline-flex items-center text-st-stale" title="Outdated — the line changed since this comment was written" data-testid="review-comment-outdated">
            <History size={11} />
          </span>
        )}
        <StateIcon state={state} title={stateTitle} />
        <AuthorIcon author={c.author} />
        {outdated ? (
          <span
            className="font-mono text-fg-4"
            style={{ fontSize: "10px" }}
            title={`Written on ${outdated.writtenOn}, at ${label}`}
            data-testid="review-comment-written-on"
          >
            {c.side === "old" ? "L" : "R"}
            {c.line} <span className="font-sans text-fg-4">on</span> {outdated.writtenOn}
          </span>
        ) : (
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {label}
          </span>
        )}
        {movedFrom !== undefined && !outdated && (
          <span
            className="inline-flex cursor-help items-center gap-0.5 rounded px-1 font-mono text-fg-4 hover:bg-bg-4 hover:text-fg-2"
            style={{ fontSize: "10px" }}
            title={`Written at ${c.side === "old" ? "L" : "R"}${movedFrom} on ${pairLabel.split(" → ")[c.side === "old" ? 0 : 1] ?? pairLabel}; the line is unchanged, so it follows it here.`}
            data-testid="review-comment-moved-from"
          >
            <CornerDownRight size={10} /> from {c.side === "old" ? "L" : "R"}
            {movedFrom}
          </span>
        )}
        <span className="text-fg-4">· {relativeTime(c.sent_at)}</span>
        {collapsible && (
          <span className={`text-fg-4 transition-transform ${collapsed ? "" : "rotate-90"}`} aria-hidden>
            <ChevronRight size={10} />
          </span>
        )}
        {collapsed && (
          <span className="flex min-w-0 items-center gap-1.5 truncate text-fg-3" style={{ fontSize: "10px" }} data-testid="review-comment-summary">
            <span className="truncate">“{firstWords(c.text)}”</span>
            {replies.length > 0 && (
              <span className="inline-flex items-center gap-0.5 text-fg-4" title={plural(replies.length, "reply").replace("replys", "replies")}>
                <MessageSquare size={10} /> {replies.length}
              </span>
            )}
          </span>
        )}
        <span className="ml-auto inline-flex items-center gap-1 text-fg-4">
          {state === "resolved" ? (
            <span className="inline-flex items-center gap-0.5 text-fg-4" title={`Resolved by ${authorLabel(resolver)}`} data-testid="review-comment-resolved-by">
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
            <div className="border-b border-line bg-bg-1" data-testid="review-comment-original-hunk">
              <div className="flex items-center gap-1.5 border-b border-line bg-bg-3 px-2.5 py-1 text-fg-4" style={{ fontSize: "10px" }}>
                <History size={10} />
                Original hunk ·{" "}
                <span className="rounded border border-line-strong bg-bg-3 px-1.5 font-mono text-fg-2" style={{ fontSize: "10px" }}>
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
                      style={l.marked ? { background: "var(--color-st-stale-bg)", boxShadow: "inset 3px 0 0 var(--color-st-stale)" } : undefined}
                    >
                      <span className="select-none pr-2 text-right text-fg-4" style={{ fontSize: "10px" }}>
                        {l.no}
                      </span>
                      <span className="whitespace-pre pl-2.5 text-fg-2">{l.text}</span>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="px-2.5 py-1.5 text-fg-4" style={{ fontSize: "10px" }}>
                  No excerpt was stored with this comment.
                </div>
              )}
            </div>
          )}
          {body(c.text)}
          {replies.length > 0 && (
            <div className="border-t border-line bg-bg-1" data-testid="review-comment-thread">
              {replies.map((r, i) => {
                const kind = authorKind(r.author);
                const isUnread = i >= firstUnread;
                return (
                  <div
                    key={`${r.at}-${i}`}
                    className={`grid grid-cols-[14px_1fr] gap-2 px-2.5 py-[7px] ${i > 0 ? "border-t border-dashed border-line-soft" : ""}`}
                    data-testid="review-reply"
                    data-author={r.author}
                    data-unread={isUnread ? "true" : undefined}
                    data-proposes={r.proposes_resolution ? "true" : undefined}
                  >
                    <span className="mt-px text-fg-3" title={authorLabel(r.author)}>
                      <AuthorGlyph kind={kind} size={12} />
                    </span>
                    <div className="min-w-0">
                      <div className="flex items-center gap-1.5 text-fg-3" style={{ fontSize: "10.5px" }}>
                        {kind === "node" && (
                          <span className="font-mono text-fg-2" style={{ fontSize: "10px" }} title={`node ${r.author}`}>
                            {r.author}
                          </span>
                        )}
                        <span className="text-fg-4">{relativeTime(r.at)}</span>
                        {r.proposes_resolution && (
                          <span className="inline-flex items-center text-st-await" title="Proposes to resolve" data-testid="review-reply-proposes">
                            <Hourglass size={10} />
                          </span>
                        )}
                        {isUnread && (
                          <span className="ml-0.5 h-1.5 w-1.5 rounded-full bg-st-running" title="New reply" data-testid="review-reply-unread" aria-label="new" />
                        )}
                      </div>
                      <div
                        className="artifact-markdown text-fg [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:list-disc [&_ul]:pl-5"
                        style={{ fontSize: "11.5px", lineHeight: 1.5 }}
                        data-testid="review-reply-body"
                      >
                        <Markdown remarkPlugins={REMARK_PLUGINS}>{r.text}</Markdown>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
          <div className="flex items-center gap-2 border-t border-line px-2.5 py-[5px] text-fg-4" style={{ fontSize: "10px" }}>
            <span className="font-mono" data-testid="review-comment-id">
              {c.id}
            </span>
            <span>· {pairLabel}</span>
            {outdated && (
              <span className="inline-flex items-center gap-1 text-st-stale" title="The line this comment points at changed on the displayed destination">
                <History size={10} /> line changed
              </span>
            )}
            <span className="ml-auto inline-flex items-center gap-1.5" data-testid="review-comment-status" data-kind={footer.kind}>
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

function FooterStatusView({ status }: { status: ReturnType<typeof footerStatus> }) {
  switch (status.kind) {
    case "awaiting":
      return (
        <>
          <Spinner dim />
          Awaiting manager reply
        </>
      );
    case "replied":
      return (
        <span className="inline-flex items-center gap-1" title={`Replied by ${authorLabel(status.author)}`}>
          <AuthorGlyph kind={authorKind(status.author)} size={10} /> {relativeTime(status.at)}
        </span>
      );
    case "proposed":
      return (
        <span className="inline-flex items-center gap-1 text-st-await" title={`Resolution proposed by ${authorLabel(status.author)}`}>
          <Hourglass size={10} />
          <AuthorGlyph kind={authorKind(status.author)} size={10} /> <span className="text-fg-4">{relativeTime(status.at)}</span>
        </span>
      );
    case "resolved":
      return (
        <span className="inline-flex items-center gap-1 text-st-done" title={`Resolved by ${authorLabel(status.by)}`}>
          <CheckCircle2 size={10} />
          <AuthorGlyph kind={authorKind(status.by)} size={10} /> <span className="text-fg-4">{relativeTime(status.at)}</span>
        </span>
      );
    case "reopened":
      return (
        <span className="inline-flex items-center gap-1" title={`Reopened by ${authorLabel(status.by)}`}>
          <Undo2 size={10} />
          <AuthorGlyph kind={authorKind(status.by)} size={10} /> {relativeTime(status.at)}
        </span>
      );
    case "declined":
      return (
        <span className="inline-flex items-center gap-1" title={`Proposal declined by ${authorLabel(status.by)} — open for the agent`}>
          <Undo2 size={10} />
          <AuthorGlyph kind={authorKind(status.by)} size={10} /> {relativeTime(status.at)}
        </span>
      );
  }
}

function DecisionButton({
  children,
  onClick,
  disabled,
  title,
  testId,
  primary,
  ghost,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled: boolean;
  title: string;
  testId: string;
  primary?: boolean;
  ghost?: boolean;
}) {
  const look = primary
    ? "border-acc bg-acc font-semibold text-on-acc hover:bg-acc-hi"
    : ghost
      ? "border-transparent bg-transparent text-fg-4 hover:bg-bg-4 hover:text-fg-2"
      : "border-line-strong bg-bg-3 text-fg-2 hover:bg-bg-4 hover:text-fg";
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onClick?.();
      }}
      disabled={disabled}
      title={title}
      data-testid={testId}
      className={`inline-flex cursor-pointer items-center gap-1 rounded border px-2 py-0.5 disabled:cursor-not-allowed disabled:opacity-45 ${look}`}
      style={{ fontSize: "10.5px" }}
    >
      {children}
    </button>
  );
}

/** The state icon — message-square open, hourglass proposed, check-circle resolved. */
export function StateIcon({ state, title, size = 12 }: { state: CommentState; title?: string; size?: number }) {
  const cls = state === "proposed" ? "text-st-await" : state === "resolved" ? "text-st-done" : "text-fg-3";
  return (
    <span className={`inline-flex items-center ${cls}`} title={title} data-testid={`review-state-${state}`}>
      {state === "proposed" ? <Hourglass size={size} /> : state === "resolved" ? <CheckCircle2 size={size} /> : <MessageSquare size={size} />}
    </span>
  );
}

/** The author icon — person you, bot manager, chip node — name in the tooltip. */
export function AuthorIcon({ author, size = 12 }: { author: string; size?: number }) {
  const kind = authorKind(author);
  return (
    <span className="inline-flex items-center gap-1 text-fg-3" title={authorLabel(author)} data-testid="review-author" data-author-kind={kind}>
      <AuthorGlyph kind={kind} size={size} />
      {kind === "node" && (
        <span className="font-mono text-fg-2" style={{ fontSize: "10px" }}>
          {author}
        </span>
      )}
    </span>
  );
}

export function AuthorGlyph({ kind, size }: { kind: AuthorKind; size: number }) {
  return kind === "user" ? <User size={size} /> : kind === "manager" ? <Bot size={size} /> : <Cpu size={size} />;
}

/** The amber (draft) / blue (sent) pill the badges, sidebar and headers share. */
export function Badge({ kind, children, title }: { kind: "draft" | "sent"; children: React.ReactNode; title?: string }) {
  return (
    <span
      title={title}
      className={`inline-flex h-[14px] items-center gap-[3px] rounded-[7px] px-[5px] font-medium ${
        kind === "draft" ? "bg-st-await-bg text-st-await" : "bg-st-running-bg text-st-running"
      }`}
      style={{ fontSize: "9.5px" }}
      data-testid={`review-badge-${kind}`}
    >
      {children}
    </span>
  );
}

export function Spinner({ dim }: { dim?: boolean }) {
  return (
    <span
      aria-hidden
      className="inline-block h-[9px] w-[9px] animate-spin rounded-full border-[1.5px] border-fg-4"
      style={{ borderTopColor: dim ? "var(--color-fg-3)" : "var(--color-st-running)" }}
    />
  );
}
