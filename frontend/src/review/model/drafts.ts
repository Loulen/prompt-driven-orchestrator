import type { ReviewSide } from "../../shared/generated/ReviewSide";
import type { SendReviewCommentInput } from "../../shared/generated/SendReviewCommentInput";
import type { RefPair } from "../../run/model/runRefs";

// Review comments of the Review page (#750, ADR-0067 §2; CONTEXT.md
// § "Commentaire de review", "Envoi au manager"). Pure: no React, no fetch.
//
// Two populations share one anchor vocabulary `(path, side, line, from, to)`:
// - **drafts**, browser-only (localStorage per Run; the half-typed text of an
//   open editor in sessionStorage so a stray reload loses nothing);
// - **sent** comments, read from the projected Run state (`review_comments`),
//   immutable, pushed over the WebSocket.
// The page merges both into one list per file and per line.
//
// This file holds the drafts and their anchor; `entries.ts` the merged list,
// `commentStatus.ts` a sent comment's states, `seen.ts` the unread replies.

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


/** `ReviewPage.tsx:R48` — basename, GitHub's side letter, line. */
export function anchorLabel(anchor: Anchor): string {
  const [file, line] = anchorParts(anchor);
  return `${file}${line}`;
}

/** `["ReviewPage.tsx", ":R48"]` — the label in the two parts a narrow column cuts apart. */
export function anchorParts(anchor: Anchor): [string, string] {
  const base = anchor.path.includes("/") ? anchor.path.slice(anchor.path.lastIndexOf("/") + 1) : anchor.path;
  return [base, `:${anchor.side === "old" ? "L" : "R"}${anchor.line}`];
}

/** `destination side` / `source side` — the header spells the side out. */
export function sideLabel(side: ReviewSide): string {
  return side === "new" ? "destination side" : "source side";
}

/** Drafts → the wire shape of the send endpoint. */
export function toSendInputs(drafts: ReviewDraft[]): SendReviewCommentInput[] {
  return drafts.map((d) => ({ path: d.path, side: d.side, line: d.line, from: d.from, to: d.to, text: d.text }));
}

