import type { ReviewDraft } from "../../lib/reviewComments";
import { anchorLabel, plural } from "../../lib/reviewComments";
import { Badge, Spinner } from "./CommentCard";

/**
 * The sticky footer bar of the Review page (#750): appears **only while drafts
 * exist** — `✎ 2 · 2 drafts ready to send · 1 already sent`, one clickable chip
 * per draft (jump to it before committing), the note "One message to the
 * manager · starts it first" when the manager is stopped (or the disabled
 * reason instead), and the primary **Send all to manager**. It disappears once
 * nothing is left to send, so the page is quiet by default.
 */

interface Props {
  drafts: ReviewDraft[];
  sentCount: number;
  sending: boolean;
  managerRunning: boolean;
  sendDisabledReason: string | null;
  onJump: (draft: ReviewDraft) => void;
  onSendAll: () => void;
}

export default function ReviewSendBar({ drafts, sentCount, sending, managerRunning, sendDisabledReason, onJump, onSendAll }: Props) {
  if (drafts.length === 0) return null;
  return (
    <div className="sticky bottom-2 z-[15] mx-3" data-testid="review-send-bar">
      <div
        className="flex items-center gap-2.5 rounded-md border border-acc-border px-3 py-1.5 text-fg-2 shadow-[0_-6px_24px_rgba(0,0,0,.35)] backdrop-blur-[4px]"
        style={{ fontSize: "11px", background: "rgba(20,23,29,.96)" }}
      >
        <Badge kind="draft">✎ {drafts.length}</Badge>
        <span>
          <span className="font-medium text-fg">{plural(drafts.length, "draft")}</span> ready to send
          {sentCount > 0 && <span> · {sentCount} already sent</span>}
        </span>
        <span className="inline-flex flex-wrap gap-1">
          {drafts.map((d) => (
            <button
              key={d.key}
              type="button"
              onClick={() => onJump(d)}
              title={d.text.slice(0, 80)}
              data-testid="review-send-chip"
              className="cursor-pointer rounded-[3px] border border-line-strong bg-bg-3 px-[5px] font-mono text-fg-3 hover:border-st-await hover:text-fg"
              style={{ fontSize: "9.5px" }}
            >
              {anchorLabel(d)}
            </button>
          ))}
        </span>
        <span className="flex-1" />
        <span className="text-fg-4" data-testid="review-send-bar-note">
          {sendDisabledReason ?? `One message to the manager${managerRunning ? "" : " · starts it first"}`}
        </span>
        <button
          type="button"
          onClick={onSendAll}
          disabled={!!sendDisabledReason || sending}
          title={sendDisabledReason ?? undefined}
          data-testid="review-send-all"
          className="inline-flex cursor-pointer items-center gap-1.5 rounded border border-acc bg-acc px-2 py-0.5 font-semibold text-on-acc hover:bg-acc-hi disabled:cursor-not-allowed disabled:opacity-45"
          style={{ fontSize: "10.5px" }}
        >
          {sending ? <Spinner /> : "↗"} Send all to manager
        </button>
      </div>
    </div>
  );
}
