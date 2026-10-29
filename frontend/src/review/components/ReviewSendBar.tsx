import type { ReviewDraft } from "../model/drafts";
import { plural } from "../model/commentStatus";
import { anchorLabel } from "../model/drafts";
import { Send } from "lucide-react";
import { Spinner } from "./commentBits";

/**
 * The sticky footer bar of the Review page (#750): appears **only while drafts
 * exist** — `2 drafts · 1 already sent`, one clickable chip per draft (jump to
 * it before committing), « starts the manager first » when the manager is
 * stopped (or the disabled reason instead), and the primary **Send all to
 * manager**, one message. It disappears once nothing is left to send, so the
 * page is quiet by default.
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
  const note = sendDisabledReason ?? (managerRunning ? "" : "starts the manager first");
  return (
    <div className="rv-sendbar" data-testid="review-send-bar">
      <span className="d-chip font-mono">{plural(drafts.length, "draft")}</span>
      {sentCount > 0 && <span className="d-meta">{sentCount} already sent</span>}
      <span className="rv-sendbar__chips">
        {drafts.map((d) => (
          <button
            key={d.key}
            type="button"
            onClick={() => onJump(d)}
            title={d.text.slice(0, 80)}
            data-testid="review-send-chip"
            className="d-chip d-chip--mono cursor-pointer hover:text-text"
          >
            {anchorLabel(d)}
          </button>
        ))}
      </span>
      <span className="flex-1" />
      <span className="d-meta rv-sendbar__note" title={note || undefined} data-testid="review-send-bar-note">
        {note}
      </span>
      <button
        type="button"
        onClick={onSendAll}
        disabled={!!sendDisabledReason || sending}
        title={sendDisabledReason ?? "Send every draft to the manager as one message"}
        data-testid="review-send-all"
        className="d-btn d-btn--primary"
      >
        {sending ? <Spinner /> : <Send size={12} strokeWidth={1.5} aria-hidden />} Send all to manager
      </button>
    </div>
  );
}
