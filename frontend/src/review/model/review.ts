/**
 * Hand-written shapes of the review payloads built with `json!` on the daemon
 * side (no Rust DTO yet).
 */

import type { ReviewAnchorMapping } from "../../shared/generated/ReviewAnchorMapping";
import type { ReviewComment } from "../../shared/generated/ReviewComment";

/** A comment as listed with a pair: the wire shape plus its mapping fields. */
export type MappedReviewComment = ReviewComment & Partial<ReviewAnchorMapping>;

export interface ReviewCommentsListResponse {
  comments: MappedReviewComment[];
  agent_can_resolve?: boolean;
  /** Present when a pair was given: the SHAs it resolved to. */
  from_sha?: string | null;
  to_sha?: string | null;
}

/** `POST …/review/comments/<id>/resolve|reopen` — the human's verbs (#751). */
export interface ReviewDecisionResponse {
  comment: ReviewComment;
  /** False when the comment was already in the requested state (no event). */
  changed: boolean;
}

export interface SendReviewCommentsResponse {
  sent: ReviewComment[];
  batch_id: string;
  /** True when this send started the manager on demand. */
  manager_started: boolean;
}

/** CORE-239: « Send again » — the ids handed to the manager once more. */
export interface ResendReviewCommentsResponse {
  resent: string[];
  manager_started: boolean;
}
