import { Bell, Eye, EyeOff, MessageSquare, Send } from "lucide-react";
import { plural } from "../model/commentStatus";
import { StateIcon } from "./commentBits";
import type { ReviewComment } from "../../shared/generated/ReviewComment";
import type { StateCounts } from "../model/commentStatus";
import type { ReviewDraft } from "../model/drafts";
import type { ReviewEntry } from "../model/entries";

interface Props {
  pairDrafts: ReviewDraft[];
  send: (keys: string[]) => Promise<void>;
  sendDisabledReason: string | null;
  sendingKeys: ReadonlySet<string>;
  sentCount: number;
  unreadComments: ReviewComment[];
  jumpUnread: () => void;
  jumpComment: (dir: 1 | -1) => void;
  cycleEntries: ReviewEntry[];
  pairStates: StateCounts;
  toggleShowResolved: () => void;
  showResolved: boolean;
}

/**
 * The comment pills of the Review page's top bar (#750, #751): pending drafts
 * → the send-all gesture (also in the footer bar); otherwise the bell of the
 * unread replies, the counts per state (icon + count, only when non-zero) and
 * the eye toggle that hides resolved cards.
 */
export default function ReviewCommentPills({ pairDrafts, send, sendDisabledReason, sendingKeys, sentCount, unreadComments, jumpUnread, jumpComment, cycleEntries, pairStates, toggleShowResolved, showResolved }: Props) {
  return pairDrafts.length > 0 ? (
      <>
        <button
          type="button"
          onClick={() => void send(pairDrafts.map((d) => d.key))}
          disabled={!!sendDisabledReason || sendingKeys.size > 0}
          title={sendDisabledReason ?? "Send every draft to the manager as one message"}
          data-testid="review-send-pill"
          className="rv-pill rv-pill--primary"
        >
          <Send size={12} strokeWidth={1.5} aria-hidden /> Send {plural(pairDrafts.length, "draft")} to manager
        </button>
        {sentCount > 0 && (
          <span
            className="rv-pill text-st-running"
            title={`${sentCount} sent, awaiting reply`}
            data-testid="review-sent-pill"
          >
            <Send size={12} strokeWidth={1.5} aria-hidden /> {sentCount} sent
          </span>
        )}
      </>
    ) : (
      <>
        {unreadComments.length > 0 && (
          // The one filled pill: unread replies, click to jump to the first.
          <button
            type="button"
            onClick={jumpUnread}
            title={`${plural(unreadComments.length, "comment")} with an unread reply — jump to the first`}
            data-testid="review-unread-pill"
            className="rv-pill rv-pill--unread"
          >
            <Bell size={12} strokeWidth={1.5} /> {unreadComments.length}
          </button>
        )}
        {sentCount === 0 ? (
          <button
            type="button"
            onClick={() => jumpComment(1)}
            disabled={cycleEntries.length === 0}
            title="Jump to the next comment  ( c / C )"
            data-testid="review-comments-pill"
            className="rv-pill"
          >
            <MessageSquare size={12} strokeWidth={1.5} /> 0 comments
          </button>
        ) : (
          <span className="flex items-center gap-1" data-testid="review-state-pills">
            {pairStates.open > 0 && (
              <button
                type="button"
                onClick={() => jumpComment(1)}
                title={`${plural(pairStates.open, "open comment")} — jump to the next  ( c / C )`}
                data-testid="review-comments-pill"
                className="rv-pill"
              >
                <StateIcon state="open" size={12} /> {pairStates.open}
              </button>
            )}
            {pairStates.proposed > 0 && (
              <button
                type="button"
                onClick={() => jumpComment(1)}
                title={`${pairStates.proposed} resolution${pairStates.proposed === 1 ? "" : "s"} proposed — your call`}
                data-testid="review-proposed-pill"
                className="rv-pill"
              >
                <StateIcon state="proposed" size={12} /> {pairStates.proposed}
              </button>
            )}
            {pairStates.resolved > 0 && (
              <span
                title={`${pairStates.resolved} resolved (skipped by c / C)`}
                data-testid="review-resolved-pill"
                className="rv-pill"
              >
                <StateIcon state="resolved" size={12} /> {pairStates.resolved}
              </span>
            )}
            <button
              type="button"
              onClick={toggleShowResolved}
              aria-pressed={!showResolved}
              title={showResolved ? "Hide resolved comments" : "Show resolved comments"}
              data-testid="review-toggle-resolved"
              className="d-icon"
            >
              {showResolved ? <Eye size={14} strokeWidth={1.5} /> : <EyeOff size={14} strokeWidth={1.5} />}
            </button>
          </span>
        )}
      </>
    );
}
