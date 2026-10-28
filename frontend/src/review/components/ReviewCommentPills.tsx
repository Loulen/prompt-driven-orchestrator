import { Bell, Eye, EyeOff, MessageSquare } from "lucide-react";
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
          className="flex cursor-pointer items-center gap-1 rounded border border-primary/30 bg-selected px-2 py-0.5 text-primary hover:bg-primary/20 disabled:cursor-not-allowed disabled:opacity-45"
          style={{ fontSize: "10.5px" }}
        >
          ↗ Send {plural(pairDrafts.length, "draft")} to manager
        </button>
        {sentCount > 0 && (
          <span
            className="flex items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-st-running"
            title={`${sentCount} sent, awaiting reply`}
            data-testid="review-sent-pill"
            style={{ fontSize: "10.5px" }}
          >
            ↗ {sentCount} sent
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
            className="flex cursor-pointer items-center gap-1 rounded border border-st-running bg-st-running px-2 py-0.5 font-semibold text-primary-ink hover:opacity-90"
            style={{ fontSize: "10.5px" }}
          >
            <Bell size={11} /> {unreadComments.length}
          </button>
        )}
        {sentCount === 0 ? (
          <button
            type="button"
            onClick={() => jumpComment(1)}
            disabled={cycleEntries.length === 0}
            title="Jump to the next comment  ( c / C )"
            data-testid="review-comments-pill"
            className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3 hover:text-text-2 disabled:cursor-not-allowed disabled:opacity-50"
            style={{ fontSize: "10.5px" }}
          >
            <MessageSquare size={11} /> 0 comments
          </button>
        ) : (
          <span className="flex items-center gap-1" data-testid="review-state-pills">
            {pairStates.open > 0 && (
              <button
                type="button"
                onClick={() => jumpComment(1)}
                title={`${plural(pairStates.open, "open comment")} — jump to the next  ( c / C )`}
                data-testid="review-comments-pill"
                className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3 hover:text-text-2"
                style={{ fontSize: "10.5px" }}
              >
                <StateIcon state="open" size={11} /> {pairStates.open}
              </button>
            )}
            {pairStates.proposed > 0 && (
              <button
                type="button"
                onClick={() => jumpComment(1)}
                title={`${pairStates.proposed} resolution${pairStates.proposed === 1 ? "" : "s"} proposed — your call`}
                data-testid="review-proposed-pill"
                className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3 hover:text-text-2"
                style={{ fontSize: "10.5px" }}
              >
                <StateIcon state="proposed" size={11} /> {pairStates.proposed}
              </button>
            )}
            {pairStates.resolved > 0 && (
              <span
                title={`${pairStates.resolved} resolved (skipped by c / C)`}
                data-testid="review-resolved-pill"
                className="flex items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3"
                style={{ fontSize: "10.5px" }}
              >
                <StateIcon state="resolved" size={11} /> {pairStates.resolved}
              </span>
            )}
            <button
              type="button"
              onClick={toggleShowResolved}
              aria-pressed={!showResolved}
              title={showResolved ? "Hide resolved comments" : "Show resolved comments"}
              data-testid="review-toggle-resolved"
              className={`grid h-5 w-[22px] cursor-pointer place-items-center rounded border border-line-strong ${
                showResolved ? "bg-surface-2 text-text-3 hover:text-text-2" : "bg-surface-2 text-text"
              }`}
            >
              {showResolved ? <Eye size={11} /> : <EyeOff size={11} />}
            </button>
          </span>
        )}
      </>
    );
}
