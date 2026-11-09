import type { SendReviewCommentInput } from "../../shared/generated/SendReviewCommentInput";
import { request } from "../../shared/http/client";
import type {
  ResendReviewCommentsResponse,
  ReviewCommentsListResponse,
  ReviewDecisionResponse,
  SendReviewCommentsResponse,
} from "../model/review";

/**
 * #750: the Run's sent review comments, on their own endpoint. With a `pair`
 * (#752, ADR-0067 §5) the daemon re-maps every anchor onto it **on read**: each
 * comment carries `outdated` and, when the line still exists, `mapped_line` /
 * `moved`. Nothing is written to the event log.
 */
export function fetchReviewComments(
  runId: string,
  pair?: { from: string; to: string },
): Promise<ReviewCommentsListResponse> {
  const qs = pair ? `?from=${encodeURIComponent(pair.from)}&to=${encodeURIComponent(pair.to)}` : "";
  return request<ReviewCommentsListResponse>("GET", `/runs/${encodeURIComponent(runId)}/review/comments${qs}`);
}

/**
 * #750: send a batch of drafts to the manager — one message for the batch, the
 * manager started on demand. A `409 run_branch_gone` (branch deleted / archived
 * Run) surfaces as an `ApiError` whose message is the daemon's reason.
 */
export function sendReviewComments(
  runId: string,
  comments: SendReviewCommentInput[],
): Promise<SendReviewCommentsResponse> {
  return request<SendReviewCommentsResponse>("POST", `/runs/${encodeURIComponent(runId)}/review/comments/send`, {
    body: { comments },
    label: "Send review comments",
  });
}

/**
 * CORE-239: « Send again » — hand comments the manager left unanswered (it
 * exited) to it once more, as written; the manager is started on demand.
 */
export function resendReviewComments(runId: string, ids: string[]): Promise<ResendReviewCommentsResponse> {
  return request<ResendReviewCommentsResponse>("POST", `/runs/${encodeURIComponent(runId)}/review/comments/resend`, {
    body: { ids },
    label: "Send review comments again",
  });
}

/** #751: the human resolves a comment (accepts a proposal, or closes it outright). */
export function resolveReviewComment(runId: string, commentId: string): Promise<ReviewDecisionResponse> {
  return request<ReviewDecisionResponse>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/review/comments/${encodeURIComponent(commentId)}/resolve`,
    { body: {}, label: "Resolve review comment" },
  );
}

/** #751: back to `sent` — reopen a resolved comment, or decline a pending proposal. */
export function reopenReviewComment(runId: string, commentId: string): Promise<ReviewDecisionResponse> {
  return request<ReviewDecisionResponse>(
    "POST",
    `/runs/${encodeURIComponent(runId)}/review/comments/${encodeURIComponent(commentId)}/reopen`,
    { body: {}, label: "Reopen review comment" },
  );
}
