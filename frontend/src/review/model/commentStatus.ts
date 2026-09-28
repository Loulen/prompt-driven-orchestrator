import type { ReviewComment } from "../../shared/generated/ReviewComment";
import type { RunRefs } from "../../shared/generated/RunRefs";
import type { RunState } from "../../shared/generated/RunState";
import type { ReviewEntry } from "./entries";
import { unreadCommentCount } from "./seen";
import type { SeenMap } from "./seen";

// A sent comment as the Review page reads it (#750, #751, #752): whether it can
// still be sent, its state, its author, its footer, the toolbar's pending pill.

/**
 * Why sending is disabled, or null when it is allowed. Mirrors the daemon's
 * `run_branch_gone` refusal so the buttons grey out *before* a click: an
 * archived Run has no branch; a `tip` ref whose SHA no longer resolves means
 * `pdo/run-<id>` was deleted.
 */
export function sendDisabledReason(run: Pick<RunState, "run_id" | "status"> | null, refs: RunRefs | null): string | null {
  if (!run) return null;
  const branch = `pdo/run-${run.run_id}`;
  if (run.status === "archived") {
    return `Run archived — branch ${branch} no longer exists. Comments stay readable; sending to the manager is disabled.`;
  }
  const tip = refs?.refs.find((r) => r.id === "tip");
  if (refs && tip && tip.sha === null) {
    return `Run branch ${branch} no longer exists — nothing for the manager to act on. Comments stay readable; sending is disabled.`;
  }
  return null;
}

/** `just now` / `3 min ago` / `14:05` — the cards' relative time. */
export function relativeTime(iso: string, now = new Date()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const s = Math.round((now.getTime() - t) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  return new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** Pending for the Review quick access (CONTEXT.md « Accès rapide Review »): sent, not resolved. */
export function pendingCount(comments: ReviewComment[] | undefined): number {
  return (comments ?? []).filter((c) => c.status !== "resolved").length;
}

/**
 * The colour of the toolbar's Review pill (#752): the number is always
 * `pendingCount`, the tone carries the nuance — outlined blue while comments
 * merely wait, solid blue when ≥ 1 reply is unread (the Diff tab's pill colour),
 * amber when ≥ 1 resolution is proposed (a decision waits on you; wins over blue).
 */
export type PendingTone = "pending" | "unread" | "proposed";

export function pendingTone(comments: ReviewComment[] | undefined, seen: SeenMap): PendingTone {
  const list = comments ?? [];
  if (list.some((c) => c.status === "sent" && c.proposal_pending)) return "proposed";
  if (unreadCommentCount(list, seen) > 0) return "unread";
  return "pending";
}

/** `Review` / `Review · 2 pending` / `Review · 2 pending, 1 unread reply, 1 resolution proposed`. */
export function reviewQuickAccessTitle(comments: ReviewComment[] | undefined, seen: SeenMap): string {
  const list = comments ?? [];
  const pending = pendingCount(list);
  if (pending === 0) return "Review";
  const parts = [`${pending} pending`];
  const unread = unreadCommentCount(list, seen);
  if (unread > 0) parts.push(`${unread} unread ${unread === 1 ? "reply" : "replies"}`);
  const proposed = list.filter((c) => c.status === "sent" && c.proposal_pending).length;
  if (proposed > 0) parts.push(`${proposed} resolution${proposed === 1 ? "" : "s"} proposed`);
  return `Review · ${parts.join(", ")}`;
}

// ---------------------------------------------------------------------------
// #751 — the conversation: states, authors, footer status, unread replies.
// ---------------------------------------------------------------------------

/** The three states a sent comment reads as: open (blue), proposed (amber), resolved (green). */
export type CommentState = "open" | "proposed" | "resolved";

export function commentState(c: ReviewComment): CommentState {
  if (c.status === "resolved") return "resolved";
  return c.proposal_pending ? "proposed" : "open";
}

/** Sidebar / `c`-cycle order: what needs a decision first, resolved last. */
export function stateRank(state: CommentState): number {
  return state === "proposed" ? 0 : state === "open" ? 1 : 2;
}

export interface StateCounts {
  open: number;
  proposed: number;
  resolved: number;
}

export function stateCounts(comments: Iterable<ReviewComment>): StateCounts {
  const n: StateCounts = { open: 0, proposed: 0, resolved: 0 };
  for (const c of comments) n[commentState(c)] += 1;
  return n;
}

/** Per-file state counts for the sidebar rows and the file headers. */
export function stateCountsByPath(entries: ReviewEntry[]): Map<string, StateCounts> {
  const m = new Map<string, StateCounts>();
  for (const e of entries) {
    if (e.kind !== "sent") continue;
    const c = m.get(e.anchor.path) ?? { open: 0, proposed: 0, resolved: 0 };
    c[commentState(e.comment)] += 1;
    m.set(e.anchor.path, c);
  }
  return m;
}

/**
 * The sidebar's Comments order: drafts first (the reader's own pending work),
 * then proposed → open → resolved, each group in diff order.
 */
export function sortForSidebar(entries: ReviewEntry[]): ReviewEntry[] {
  const rank = (e: ReviewEntry) => (e.kind === "draft" ? -1 : stateRank(commentState(e.comment)));
  return entries
    .map((e, i) => ({ e, i }))
    .sort((a, b) => rank(a.e) - rank(b.e) || a.i - b.i)
    .map(({ e }) => e);
}

/** Who an author string is: the human (`user`), the manager, or a node of the Run. */
export type AuthorKind = "user" | "manager" | "node";

export function authorKind(author: string): AuthorKind {
  if (author === "user" || author === "you") return "user";
  if (author === "manager" || author === "agent") return "manager";
  return "node";
}

/** `you` / `manager` / the node id — the tooltip text next to an author icon. */
export function authorLabel(author: string): string {
  const k = authorKind(author);
  return k === "user" ? "you" : k === "manager" ? "manager" : author;
}

/** First line, truncated with an ellipsis — the collapsed resolved card's summary. */
export function firstWords(text: string, max = 44): string {
  const line = text.split("\n").find((l) => l.trim().length > 0)?.trim() ?? "";
  return line.length > max ? `${line.slice(0, max).trimEnd()}…` : line;
}

/** What the sent card's footer says, right of the id and the pair. */
export type FooterStatus =
  | { kind: "awaiting" }
  | { kind: "replied"; author: string; at: string }
  | { kind: "proposed"; author: string; at: string }
  | { kind: "resolved"; by: string; at: string }
  | { kind: "reopened"; by: string; at: string }
  | { kind: "declined"; by: string; at: string };

export function footerStatus(c: ReviewComment): FooterStatus {
  const replies = c.replies ?? [];
  const last = replies[replies.length - 1];
  if (c.status === "resolved") return { kind: "resolved", by: c.resolved_by ?? "user", at: c.resolved_at ?? last?.at ?? c.sent_at };
  if (c.proposal_pending) {
    const proposing = [...replies].reverse().find((r) => r.proposes_resolution) ?? last;
    return { kind: "proposed", author: proposing?.author ?? "agent", at: proposing?.at ?? c.sent_at };
  }
  if (c.reopened_at && (!last || c.reopened_at >= last.at)) {
    return { kind: c.proposal_declined ? "declined" : "reopened", by: c.reopened_by ?? "user", at: c.reopened_at };
  }
  if (last) return { kind: "replied", author: last.author, at: last.at };
  return { kind: "awaiting" };
}

/** `2 drafts` / `1 draft` — pluralised counter. */
export function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}
