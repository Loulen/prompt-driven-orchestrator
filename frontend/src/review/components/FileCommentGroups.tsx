import { History } from "lucide-react";
import type { RefPair } from "../../run/model/runRefs";
import { plural } from "../model/commentStatus";
import { wipKey } from "../model/drafts";
import type { ReviewEntry } from "../model/entries";
import CommentEditor from "./CommentEditor";
import type { ReviewCommentsApi } from "../model/commentsApi";

interface Props {
  runId: string;
  pair: RefPair;
  comments: ReviewCommentsApi;
  /** #752: the sent entries whose line changed on the displayed destination. */
  outdatedEntries: ReviewEntry[];
  /** #752: the drafts whose line the displayed hunks no longer show. */
  orphanDrafts: ReviewEntry[];
  renderCard: (entry: ReviewEntry) => React.ReactNode;
}

/**
 * The comments of a file card that cannot sit under their line (#752): the
 * "N outdated comments" group, collapsed, and the drafts on a line that changed
 * — still editable, sendable, deletable.
 */
export default function FileCommentGroups({ runId, pair, comments, outdatedEntries, orphanDrafts, renderCard }: Props) {
  const nOutdated = outdatedEntries.length;
  const showOutdated = comments.showOutdated;
  return (
    <>
      {nOutdated > 0 && (
        <div data-testid="review-outdated-group" data-hidden={showOutdated ? undefined : "true"} className="border-b border-line pb-1">
          <div className="mx-3 mt-2 flex items-center gap-2 text-text-3 text-xs">
            <span className="inline-flex items-center gap-1 text-st-skipped">
              <History size={10} /> {plural(nOutdated, "outdated comment")}
            </span>
            <span className="h-px flex-1 bg-line" />
            <span
              className="cursor-help"
              title="Its line changed between the destination it was written on and this one. Still answerable, still resolvable."
            >
              what is this?
            </span>
          </div>
          {showOutdated && outdatedEntries.map((e) => <div key={e.kind === "sent" ? e.comment.id : e.anchor.line}>{renderCard(e)}</div>)}
        </div>
      )}
      {orphanDrafts.length > 0 && (
        <div data-testid="review-orphan-drafts" className="border-b border-line pb-1">
          <div className="mx-3 mt-2 flex items-center gap-2 text-st-await text-xs">
            ✎ {plural(orphanDrafts.length, "draft")} on a line that changed
            <span className="h-px flex-1 bg-line" />
            <span className="cursor-help text-text-3" title="Drafts are never re-mapped: the line this draft was written under is no longer in the diff. Edit, send or delete it.">
              line changed
            </span>
          </div>
          {orphanDrafts.map((e) =>
            e.kind === "draft" && comments.editingKey === e.draft.key ? (
              <div key={e.draft.key} className="mx-3 my-2">
                <CommentEditor
                  anchor={e.anchor}
                  pair={pair}
                  initial={e.draft.text}
                  wipKey={wipKey(runId, e.anchor, pair)}
                  sendDisabledReason={comments.sendDisabledReason}
                  sendRefused={comments.sendRefused}
                  onSave={(text) => comments.updateDraft(e.draft.key, text)}
                  onSend={(text) => {
                    comments.updateDraft(e.draft.key, text);
                    comments.sendDraft(e.draft.key);
                  }}
                  onCancel={comments.cancelEdit}
                />
              </div>
            ) : (
              <div key={e.kind === "draft" ? e.draft.key : e.anchor.line}>{renderCard(e)}</div>
            ),
          )}
        </div>
      )}
    </>
  );
}
