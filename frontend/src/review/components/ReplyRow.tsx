import { Hourglass } from "lucide-react";
import { useActorDisplay } from "../../shared/actors/store";
import type { ReviewReply } from "../../shared/generated/ReviewReply";
import { authorKind, authorLabel, relativeTime } from "../model/commentStatus";
import { AuthorGlyph } from "./commentBits";
import ReviewMarkdown from "./ReviewMarkdown";

/**
 * One reply of a sent comment's thread (#751): the author's icon (and its id
 * for a node), the time, an hourglass when it proposes a resolution, a blue
 * dot while unread, then its markdown. CORE-63: an agent acting for a person
 * an extension names shows `Alice (agent)` next to its icon.
 */
export default function ReplyRow({ reply: r, first, unread }: { reply: ReviewReply; first: boolean; unread: boolean }) {
  const kind = authorKind(r.author);
  const name = useActorDisplay(r.actor)?.name;
  return (
    <div
      className={`grid grid-cols-[14px_1fr] gap-2 px-2.5 py-[7px] ${first ? "" : "border-t border-dashed border-line-soft"}`}
      data-testid="review-reply"
      data-author={r.author}
      data-unread={unread ? "true" : undefined}
      data-proposes={r.proposes_resolution ? "true" : undefined}
    >
      <span className="mt-px text-text-3" title={authorLabel(r.author, name)}>
        <AuthorGlyph kind={kind} size={12} />
      </span>
      <div className="min-w-0">
        <div className="flex items-center gap-1.5 text-text-3" style={{ fontSize: "10.5px" }}>
          {name && (
            <span className="text-text-2" data-testid="review-reply-actor">
              {kind === "user" ? name : `${name} (agent)`}
            </span>
          )}
          {kind === "node" && (
            <span className="font-mono text-text-2" style={{ fontSize: "10px" }} title={`node ${r.author}`}>
              {r.author}
            </span>
          )}
          <span className="text-text-3">{relativeTime(r.at)}</span>
          {r.proposes_resolution && (
            <span className="inline-flex items-center text-st-await" title="Proposes to resolve" data-testid="review-reply-proposes">
              <Hourglass size={10} />
            </span>
          )}
          {unread && (
            <span className="ml-0.5 h-1.5 w-1.5 rounded-full bg-st-running" title="New reply" data-testid="review-reply-unread" aria-label="new" />
          )}
        </div>
        <div
          className="artifact-markdown text-text [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:list-disc [&_ul]:pl-5"
          style={{ fontSize: "11.5px", lineHeight: 1.5 }}
          data-testid="review-reply-body"
        >
          <ReviewMarkdown text={r.text} />
        </div>
      </div>
    </div>
  );
}
