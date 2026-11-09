import { useCallback, useMemo, useState } from "react";
import type { MutableRefObject } from "react";
import type { ReviewComment } from "../../shared/generated/ReviewComment";
import type { RunState } from "../../shared/generated/RunState";
import type { RefPair } from "../../run/model/runRefs";
import { reopenReviewComment, resendReviewComments, resolveReviewComment, sendReviewComments } from "../api/comments";
import { authorLabel, lostCommentIds, plural } from "../model/commentStatus";
import { addDraft, removeDrafts, toSendInputs, updateDraft } from "../model/drafts";
import type { Anchor, ReviewDraft } from "../model/drafts";
import { markSeen as markSeenIn, readSeen, unreadReplies, writeSeen } from "../model/seen";
import type { ReviewCommentsApi } from "../model/commentsApi";
import type { ReviewCommentState } from "./useReviewCommentState";
import type { ReviewEntries } from "./useReviewEntries";
import type { ReviewRefusals } from "./useReviewRefusals";

interface Args {
  runId: string;
  runRef: MutableRefObject<RunState | null>;
  refreshRun: () => Promise<void>;
  pair: RefPair;
  state: ReviewCommentState;
  entries: ReviewEntries;
  flashing: ReadonlySet<string>;
  sendDisabledReason: string | null;
  /** CORE-205: the gestures the caller may not make, and why. */
  refusals?: ReviewRefusals;
  jumpTo: (anchor: Anchor) => void;
  /** CORE-239: the Run's sent comments and its `has_manager` (undefined until known). */
  reviewComments?: ReviewComment[];
  managerRunning?: boolean;
}

/**
 * What the operator does with the comments of the Review page (#750, #751):
 * write, edit, delete and send drafts (one message per batch, the manager
 * started on demand; a refused send keeps the drafts), resolve or reopen a
 * sent comment, mark a card seen, jump to the next comment or to the first
 * unread reply. A gesture the caller may not make does nothing (CORE-205):
 * its button stays, greyed, with the refusal's sentence.
 */
export function useReviewGestures({
  runId,
  runRef,
  refreshRun,
  pair,
  state,
  entries,
  flashing,
  sendDisabledReason,
  refusals,
  jumpTo,
  reviewComments,
  managerRunning,
}: Args) {
  const sendRefused = refusals?.send;
  const resolveOf = refusals?.resolveOf;
  const reopenOf = refusals?.reopenOf;
  const { drafts, persistDrafts, showToast, deciding, setDeciding, setSeen, seen } = state;
  const { cycleEntries, unreadComments, pairLabel, writtenOnLabel } = entries;
  const { commentCursor, setCommentCursor, setEditingKey, setSendingKeys, setStartingManager } = state;

  const jumpComment = useCallback(
    (dir: 1 | -1) => {
      if (cycleEntries.length === 0) return;
      const next = (commentCursor + dir + cycleEntries.length) % cycleEntries.length;
      setCommentCursor(next);
      jumpTo(cycleEntries[next].anchor);
    },
    [cycleEntries, commentCursor, jumpTo, setCommentCursor],
  );

  const markSeen = useCallback(
    (c: ReviewComment) => {
      setSeen((prev) => markSeenIn(prev, c));
      // Storage too: another tab's Diff badge must not keep counting this one.
      writeSeen(runId, markSeenIn(readSeen(runId), c));
    },
    [runId, setSeen],
  );
  /** The bell: jump to the first comment with an unread reply and mark it seen. */
  const jumpUnread = useCallback(() => {
    const target = unreadComments[0];
    if (!target) return;
    jumpTo({ path: target.path, side: target.side, line: target.line });
    markSeen(target);
  }, [unreadComments, jumpTo, markSeen]);

  const decide = useCallback(
    async (c: ReviewComment, verb: "resolve" | "reopen") => {
      if (deciding.has(c.id)) return;
      const refused = verb === "resolve" ? resolveOf?.(c.id) : reopenOf?.(c.id);
      if (refused) {
        showToast(refused, true);
        return;
      }
      setDeciding((d) => new Set([...d, c.id]));
      try {
        const res = await (verb === "resolve" ? resolveReviewComment(runId, c.id) : reopenReviewComment(runId, c.id));
        markSeen(res.comment);
        await refreshRun();
        if (!res.changed) {
          showToast(`${c.id} was already ${verb === "resolve" ? "resolved" : "open"}.`);
        } else if (verb === "resolve") {
          showToast(`${c.id} resolved — collapsed; Reopen stays in its footer.`);
        } else if (c.status === "resolved") {
          showToast(`${c.id} reopened — back to open, the agent sees it in pdo review list.`);
        } else {
          showToast(`${c.id}: proposal declined — the comment stays open for ${authorLabel(res.comment.replies?.at(-1)?.author ?? "agent")}.`);
        }
      } catch (e: unknown) {
        showToast(`${verb === "resolve" ? "Resolve" : "Reopen"} failed — ${e instanceof Error ? e.message : String(e)}`, true);
      } finally {
        setDeciding((d) => {
          const next = new Set(d);
          next.delete(c.id);
          return next;
        });
      }
    },
    [deciding, runId, markSeen, showToast, refreshRun, setDeciding, resolveOf, reopenOf],
  );

  /** Send `keys` out of `source` (defaults to the current drafts; `sendNew` passes the list it just grew). */
  const send = useCallback(
    async (keys: string[], source: ReviewDraft[] = drafts) => {
      const why = sendDisabledReason ?? sendRefused;
      if (why) {
        showToast(why, true);
        return;
      }
      const targets = source.filter((d) => keys.includes(d.key));
      if (targets.length === 0) return;
      setSendingKeys(new Set(keys));
      const mustStart = !(runRef.current?.has_manager ?? false);
      setStartingManager(mustStart);
      if (mustStart) showToast("Manager not running — starting it before sending…");
      try {
        const res = await sendReviewComments(runId, toSendInputs(targets));
        persistDrafts(removeDrafts(source, keys));
        setEditingKey((k) => (k && keys.includes(k) ? null : k));
        await refreshRun();
        showToast(
          `${plural(res.sent.length, "comment")} sent as one message${res.manager_started ? " — manager started" : ""}. Open the Manager tab to see it; replies show up inline.`,
        );
      } catch (e: unknown) {
        // Nothing is lost: the drafts stay drafts, the operator retries.
        showToast(`Send failed — ${e instanceof Error ? e.message : String(e)}. Your drafts are kept.`, true);
      } finally {
        setSendingKeys(new Set());
        setStartingManager(false);
      }
    },
    [drafts, runId, runRef, sendDisabledReason, sendRefused, showToast, persistDrafts, refreshRun, setEditingKey, setSendingKeys, setStartingManager],
  );

  // CORE-239: « Send again » hands every comment the manager left unanswered to a new one.
  const lostIds = useMemo(() => lostCommentIds(reviewComments, managerRunning), [reviewComments, managerRunning]);
  const [resending, setResending] = useState(false);
  const resend = useCallback(async () => {
    const why = sendDisabledReason ?? sendRefused;
    if (why) {
      showToast(why, true);
      return;
    }
    if (lostIds.length === 0 || resending) return;
    setResending(true);
    try {
      const res = await resendReviewComments(runId, lostIds);
      await refreshRun();
      showToast(
        `${plural(res.resent.length, "comment")} sent again${res.manager_started ? " — manager started" : ""}. Replies show up inline.`,
      );
    } catch (e: unknown) {
      showToast(`Send again failed — ${e instanceof Error ? e.message : String(e)}`, true);
    } finally {
      setResending(false);
    }
  }, [lostIds, resending, runId, sendDisabledReason, sendRefused, showToast, refreshRun]);
  const delivery = useMemo(
    () => ({
      managerRunning,
      runHref: `/#/runs/${encodeURIComponent(runId)}`,
      onResend: () => void resend(),
      resending,
      resendCount: lostIds.length,
      resendRefused: sendDisabledReason ?? sendRefused ?? undefined,
    }),
    [managerRunning, runId, resend, resending, lostIds.length, sendDisabledReason, sendRefused],
  );

  const { editingKey, sendingKeys, startingManager, showResolved, showOutdated } = state;
  const commentsApi = useMemo<ReviewCommentsApi>(
    () => ({
      editingKey,
      sendingKeys,
      startingManager,
      pairLabel,
      sendDisabledReason,
      sendRefused,
      saveNew: (anchor, text) => {
        persistDrafts(addDraft(drafts, anchor, pair, text));
        showToast("Draft saved — kept in this browser until you send it.");
      },
      sendNew: (anchor, text) => {
        const next = addDraft(drafts, anchor, pair, text);
        persistDrafts(next);
        const created = next[next.length - 1];
        void send([created.key], next);
      },
      editDraft: (key) => setEditingKey(key),
      updateDraft: (key, text) => {
        persistDrafts(updateDraft(drafts, key, text));
        setEditingKey(null);
      },
      cancelEdit: () => setEditingKey(null),
      deleteDraft: (key) => {
        persistDrafts(removeDrafts(drafts, [key]));
        setEditingKey((k) => (k === key ? null : k));
        showToast("Draft deleted.");
      },
      sendDraft: (key) => void send([key]),
      sentLineClicked: () =>
        showToast("This line already has a sent comment. The agent's replies show up under it; add a new comment on another line.", true),
      resolve: (c) => void decide(c, "resolve"),
      reopen: (c) => void decide(c, "reopen"),
      resolveRefused: (c) => resolveOf?.(c.id),
      reopenRefused: (c) => reopenOf?.(c.id),
      deciding,
      unreadOf: (c) => unreadReplies(c, seen),
      flashing,
      markSeen,
      showResolved,
      showOutdated,
      writtenOnLabel,
      delivery,
    }),
    [
      editingKey,
      sendingKeys,
      startingManager,
      pairLabel,
      sendDisabledReason,
      sendRefused,
      resolveOf,
      reopenOf,
      drafts,
      pair,
      persistDrafts,
      showToast,
      send,
      decide,
      deciding,
      seen,
      flashing,
      markSeen,
      showResolved,
      showOutdated,
      writtenOnLabel,
      setEditingKey,
      delivery,
    ],
  );

  return { jumpComment, jumpUnread, send, commentsApi };
}
