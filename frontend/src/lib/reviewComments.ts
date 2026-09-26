import type { ReviewAnchorMapping } from "../shared/generated/ReviewAnchorMapping";
import type { ReviewComment } from "../shared/generated/ReviewComment";
import type { ReviewSide } from "../shared/generated/ReviewSide";
import type { RunRefs } from "../shared/generated/RunRefs";
import type { RunState } from "../shared/generated/RunState";
import type { SendReviewCommentInput } from "../shared/generated/SendReviewCommentInput";
import type { RefPair } from "./runRefs";

// Review comments of the Review page (#750, ADR-0067 §2; CONTEXT.md
// § "Commentaire de review", "Envoi au manager"). Pure: no React, no fetch.
//
// Two populations share one anchor vocabulary `(path, side, line, from, to)`:
// - **drafts**, browser-only (localStorage per Run; the half-typed text of an
//   open editor in sessionStorage so a stray reload loses nothing);
// - **sent** comments, read from the projected Run state (`review_comments`),
//   immutable, pushed over the WebSocket.
// The page merges both into one list per file and per line.

/** A draft: editable, deletable, only in this browser until sent. */
export interface ReviewDraft {
  /** Local identity (uuid-ish); never sent to the daemon. */
  key: string;
  path: string;
  side: ReviewSide;
  line: number;
  from: string;
  to: string;
  text: string;
  created_at: string;
  updated_at: string;
}

/** The anchor of a comment, draft or sent — what "one comment per line" keys on. */
export interface Anchor {
  path: string;
  side: ReviewSide;
  line: number;
}

/**
 * One entry of the merged list the page renders. A sent entry's `anchor` is
 * where the card sits **on the displayed pair** (#752): the reported line when
 * the daemon mapped it, the written line otherwise. `outdated` marks a comment
 * whose line changed since (card in the file's outdated group, not inline);
 * `movedFrom` is the written line when the reported number differs.
 */
export type ReviewEntry =
  | { kind: "draft"; anchor: Anchor; draft: ReviewDraft }
  | { kind: "sent"; anchor: Anchor; comment: ReviewComment; outdated?: boolean; movedFrom?: number };

/** Comment id → its mapping onto the displayed pair (from `GET …/review/comments?from&to`). */
export type MappingMap = ReadonlyMap<string, ReviewAnchorMapping>;

export function mappingsOf(comments: (ReviewComment & Partial<ReviewAnchorMapping>)[]): Map<string, ReviewAnchorMapping> {
  const m = new Map<string, ReviewAnchorMapping>();
  for (const c of comments) {
    if (typeof c.outdated !== "boolean") continue;
    m.set(c.id, { outdated: c.outdated, mapped_line: c.mapped_line, moved: c.moved });
  }
  return m;
}

export const DRAFTS_KEY_PREFIX = "pdo.review.drafts.";
export const WIP_KEY_PREFIX = "pdo.review.wip.";

export function draftsKey(runId: string): string {
  return `${DRAFTS_KEY_PREFIX}${runId}`;
}

/** The sessionStorage key of an editor's half-typed text. */
export function wipKey(runId: string, anchor: Anchor, pair: RefPair): string {
  return `${WIP_KEY_PREFIX}${runId}|${anchor.path}|${anchor.side}|${anchor.line}|${pair.from}|${pair.to}`;
}

export function readDrafts(runId: string, storage: Pick<Storage, "getItem"> = localStorage): ReviewDraft[] {
  try {
    const raw = storage.getItem(draftsKey(runId));
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isDraft);
  } catch {
    return [];
  }
}

export function writeDrafts(
  runId: string,
  drafts: ReviewDraft[],
  storage: Pick<Storage, "setItem" | "removeItem"> = localStorage,
): void {
  try {
    if (drafts.length === 0) storage.removeItem(draftsKey(runId));
    else storage.setItem(draftsKey(runId), JSON.stringify(drafts));
  } catch {
    // Private mode / quota: the drafts still live for the session in memory.
  }
}

function isDraft(v: unknown): v is ReviewDraft {
  const d = v as Partial<ReviewDraft> | null;
  return (
    !!d &&
    typeof d.key === "string" &&
    typeof d.path === "string" &&
    (d.side === "old" || d.side === "new") &&
    typeof d.line === "number" &&
    typeof d.from === "string" &&
    typeof d.to === "string" &&
    typeof d.text === "string"
  );
}

export function newDraftKey(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c?.randomUUID) return c.randomUUID();
  return `d-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/** Add a draft at `anchor` for `pair`; the caller persists the returned list. */
export function addDraft(drafts: ReviewDraft[], anchor: Anchor, pair: RefPair, text: string, now = new Date()): ReviewDraft[] {
  const ts = now.toISOString();
  return [
    ...drafts,
    { key: newDraftKey(), ...anchor, from: pair.from, to: pair.to, text, created_at: ts, updated_at: ts },
  ];
}

export function updateDraft(drafts: ReviewDraft[], key: string, text: string, now = new Date()): ReviewDraft[] {
  return drafts.map((d) => (d.key === key ? { ...d, text, updated_at: now.toISOString() } : d));
}

export function removeDrafts(drafts: ReviewDraft[], keys: Iterable<string>): ReviewDraft[] {
  const set = new Set(keys);
  return drafts.filter((d) => !set.has(d.key));
}

export function sameAnchor(a: Anchor, b: Anchor): boolean {
  return a.path === b.path && a.side === b.side && a.line === b.line;
}

/** The drafts written against exactly this pair (others stay stored, hidden from the diff body). */
export function draftsForPair(drafts: ReviewDraft[], pair: RefPair): ReviewDraft[] {
  return drafts.filter((d) => d.from === pair.from && d.to === pair.to);
}

/** The sent comments written against exactly this pair. */
export function sentForPair(comments: ReviewComment[] | undefined, pair: RefPair): ReviewComment[] {
  return (comments ?? []).filter((c) => c.from_ref === pair.from && c.to_ref === pair.to);
}

/**
 * The sent entries **at home** on the displayed pair (#752): a comment the daemon
 * mapped onto the pair whose file is in the displayed diff — reported at its
 * mapped line, or outdated at its written line — and, while no mapping is known
 * (not fetched yet, or the daemon could not map it), a comment written against
 * exactly this pair, where it was written. A mapped comment whose file is not
 * in the displayed diff has no card to live in: it is listed under "other".
 */
export function sentEntriesForPair(
  comments: ReviewComment[] | undefined,
  pair: RefPair,
  mappings: MappingMap | undefined,
  pathOrder: string[],
): ReviewEntry[] {
  const paths = new Set(pathOrder);
  const out: ReviewEntry[] = [];
  for (const c of comments ?? []) {
    const m = mappings?.get(c.id);
    const exact = c.from_ref === pair.from && c.to_ref === pair.to;
    if (!m) {
      if (exact) out.push({ kind: "sent", anchor: { path: c.path, side: c.side, line: c.line }, comment: c });
      continue;
    }
    // Mapped, but its file is not in this diff: no card can host it — "other".
    if (!paths.has(c.path)) continue;
    if (m.outdated) {
      out.push({ kind: "sent", anchor: { path: c.path, side: c.side, line: c.line }, comment: c, outdated: true });
      continue;
    }
    const line = m.mapped_line ?? c.line;
    out.push({
      kind: "sent",
      anchor: { path: c.path, side: c.side, line },
      comment: c,
      movedFrom: line !== c.line ? c.line : undefined,
    });
  }
  return out;
}

/** The ids of the comments `sentEntriesForPair` places on the pair — the rest are "other". */
export function homeIds(entries: ReviewEntry[]): Set<string> {
  return new Set(entries.flatMap((e) => (e.kind === "sent" ? [e.comment.id] : [])));
}

/** `1 comment moved · 1 outdated` after a re-map; null when nothing moved or went outdated. */
export function remapSummary(entries: ReviewEntry[]): string | null {
  let moved = 0;
  let outdated = 0;
  for (const e of entries) {
    if (e.kind !== "sent") continue;
    if (e.outdated) outdated += 1;
    else if (e.movedFrom !== undefined) moved += 1;
  }
  if (moved === 0 && outdated === 0) return null;
  const parts: string[] = [];
  if (moved > 0) parts.push(`${plural(moved, "comment")} moved`);
  if (outdated > 0) parts.push(`${outdated} outdated`);
  return parts.join(" · ");
}

/** Per-path outdated count — the history glyph on file rows and headers (#752). */
export function outdatedCountByPath(entries: ReviewEntry[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const e of entries) {
    if (e.kind === "sent" && e.outdated) m.set(e.anchor.path, (m.get(e.anchor.path) ?? 0) + 1);
  }
  return m;
}

/** One line of a stored excerpt (`> 3 | text`): number, text, and whether it is the commented line. */
export interface ExcerptLine {
  no: number;
  text: string;
  marked: boolean;
}

/** Parse the hunk excerpt the daemon stored at send time (the outdated card's original hunk). */
export function parseExcerpt(excerpt: string | undefined): ExcerptLine[] {
  if (!excerpt) return [];
  const out: ExcerptLine[] = [];
  for (const raw of excerpt.split("\n")) {
    const m = /^([> ]) *(\d+) \| ?(.*)$/.exec(raw);
    if (!m) continue;
    out.push({ no: Number(m[2]), text: m[3], marked: m[1] === ">" });
  }
  return out;
}

export const SHOW_OUTDATED_KEY = "pdo.review.showOutdated";

/**
 * The merged list for one pair, in diff order: by path (patch order given), then
 * line, then side (old before new — the left column first). Sent before draft on
 * the same anchor is impossible by construction (one comment per line), but a
 * stale draft on a line just sent still resolves deterministically: sent wins.
 */
export function mergeEntries(
  drafts: ReviewDraft[],
  comments: ReviewComment[] | undefined,
  pair: RefPair,
  pathOrder: string[],
  mappings?: MappingMap,
): ReviewEntry[] {
  const order = new Map(pathOrder.map((p, i) => [p, i]));
  const rank = (p: string) => order.get(p) ?? Number.MAX_SAFE_INTEGER;
  const entries: ReviewEntry[] = [
    ...sentEntriesForPair(comments, pair, mappings, pathOrder),
    ...draftsForPair(drafts, pair).map<ReviewEntry>((d) => ({
      kind: "draft",
      anchor: { path: d.path, side: d.side, line: d.line },
      draft: d,
    })),
  ];
  return entries.sort((a, b) => {
    const r = rank(a.anchor.path) - rank(b.anchor.path);
    if (r !== 0) return r;
    if (a.anchor.path !== b.anchor.path) return a.anchor.path < b.anchor.path ? -1 : 1;
    if (a.anchor.line !== b.anchor.line) return a.anchor.line - b.anchor.line;
    if (a.anchor.side !== b.anchor.side) return a.anchor.side === "old" ? -1 : 1;
    // Same anchor: sent first, so the page shows the immutable one.
    if (a.kind !== b.kind) return a.kind === "sent" ? -1 : 1;
    return 0;
  });
}

/** The entry anchored on this exact line, if any (the "one comment per line" lookup). */
export function entryAt(entries: ReviewEntry[], anchor: Anchor): ReviewEntry | undefined {
  return entries.find((e) => sameAnchor(e.anchor, anchor));
}

/** Per-file counts for the sidebar badges and the card headers. */
export function countsByPath(entries: ReviewEntry[]): Map<string, { drafts: number; sent: number }> {
  const m = new Map<string, { drafts: number; sent: number }>();
  for (const e of entries) {
    const c = m.get(e.anchor.path) ?? { drafts: 0, sent: 0 };
    if (e.kind === "draft") c.drafts += 1;
    else c.sent += 1;
    m.set(e.anchor.path, c);
  }
  return m;
}

/** `ReviewPage.tsx:R48` — basename, GitHub's side letter, line. */
export function anchorLabel(anchor: Anchor): string {
  const base = anchor.path.includes("/") ? anchor.path.slice(anchor.path.lastIndexOf("/") + 1) : anchor.path;
  return `${base}:${anchor.side === "old" ? "L" : "R"}${anchor.line}`;
}

/** `destination side` / `source side` — the header spells the side out. */
export function sideLabel(side: ReviewSide): string {
  return side === "new" ? "destination side" : "source side";
}

/** Drafts → the wire shape of the send endpoint. */
export function toSendInputs(drafts: ReviewDraft[]): SendReviewCommentInput[] {
  return drafts.map((d) => ({ path: d.path, side: d.side, line: d.line, from: d.from, to: d.to, text: d.text }));
}

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

// --- Unread replies: a per-browser "seen" marker, never a Run event -----------

export const SEEN_KEY_PREFIX = "pdo.review.seen.";

/** Comment id → number of replies this browser has seen. */
export type SeenMap = Record<string, number>;

export function seenKey(runId: string): string {
  return `${SEEN_KEY_PREFIX}${runId}`;
}

export function readSeen(runId: string, storage: Pick<Storage, "getItem"> = localStorage): SeenMap {
  try {
    const raw = storage.getItem(seenKey(runId));
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const out: SeenMap = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "number" && Number.isFinite(v)) out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

export function writeSeen(runId: string, seen: SeenMap, storage: Pick<Storage, "setItem" | "removeItem"> = localStorage): void {
  try {
    if (Object.keys(seen).length === 0) storage.removeItem(seenKey(runId));
    else storage.setItem(seenKey(runId), JSON.stringify(seen));
  } catch {
    // Private mode / quota: unread stays in memory for the session.
  }
}

/** How many replies of `c` this browser has not seen. */
export function unreadReplies(c: ReviewComment, seen: SeenMap): number {
  return Math.max(0, (c.replies?.length ?? 0) - (seen[c.id] ?? 0));
}

/**
 * The Diff tab badge (CONTEXT.md « Réponse de review »): **sent** comments with at
 * least one unread reply. A comment the agent resolved directly is not counted
 * — nothing waits on the human there — but its replies still read as new on the
 * card until seen.
 */
export function unreadCommentCount(comments: ReviewComment[] | undefined, seen: SeenMap): number {
  return (comments ?? []).filter((c) => c.status === "sent" && unreadReplies(c, seen) > 0).length;
}

/** Everything seen — what opening the Review page writes. */
export function allSeen(comments: ReviewComment[] | undefined, prev: SeenMap = {}): SeenMap {
  const next: SeenMap = { ...prev };
  for (const c of comments ?? []) next[c.id] = Math.max(next[c.id] ?? 0, c.replies?.length ?? 0);
  return next;
}

/** One card seen (scrolled into view / hovered / acted on). */
export function markSeen(seen: SeenMap, c: ReviewComment): SeenMap {
  const n = c.replies?.length ?? 0;
  if ((seen[c.id] ?? 0) >= n) return seen;
  return { ...seen, [c.id]: n };
}

/** `2 drafts` / `1 draft` — pluralised counter. */
export function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}
