import { Pencil, Trash2 } from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ReviewEntry } from "../model/entries";
import { relativeTime } from "../model/commentStatus";
import { anchorLabel } from "../model/drafts";
import { Badge, Spinner } from "./commentBits";
import SentCard from "./SentCard";


/**
 * One review comment rendered under its diff line (#750): a **draft** card
 * (amber `✎ Draft`, edit / delete on hover, footer "Only in this browser until
 * sent" + `↗ Send to manager` always visible), a **sending** card (greyed,
 * spinner, "starting the manager…" when the send has to start it), or a
 * **sent** card. No edit, no delete on a sent comment: it is a Run event.
 *
 * #751 — the sent card is a **thread**: the agent's replies stack between the
 * body and the footer, one row per reply (author icon — person you, bot
 * manager, chip + id for a node —, time, an hourglass when the reply proposes a
 * resolution, a blue dot while unread). State is one icon plus a 2px left
 * border on the header: message-square open (blue), hourglass **Resolution
 * proposed** (amber), check-circle resolved (green); no filled pills, names
 * live in tooltips. The footer carries the decision: `Resolve` on an open or
 * proposed comment, `Reopen` on a resolved one — and on a proposal, where it
 * declines it and keeps the comment open for the agent. A resolved card
 * **collapses** to one line (icon · anchor · time · first words · reply count ·
 * resolved-by); its header click expands it. A reply that lands live flashes
 * the card's outline for two seconds.
 *
 * #752 — re-mapped comments (ADR-0067 §5). A **reported** card is identical to a
 * plain one; only when the line *number* differs does a discreet `↳ from R212`
 * chip sit in the header. An **outdated** card (its line changed on the displayed
 * destination) is collapsed by default — history glyph in the stale amber, state
 * icon, author, `R239 on <ref>`, time, first words, reply count, lock — and
 * expands to the **original hunk** first (the excerpt stored at send time, the
 * commented line in the stale tint, captioned with the ref it was written on),
 * then body, replies and the usual footer: outdated is a display property, not a
 * state, so Resolve / Reopen work as on any sent card.
 */

interface Props {
  entry: ReviewEntry;
  /** True while this draft travels in a send. */
  sending: boolean;
  /** The send is starting the manager first (message nuance while sending). */
  startingManager: boolean;
  /** Labels of the pair for the sent footer (`Fork point → Run tip`). */
  pairLabel: string;
  sendDisabledReason: string | null;
  onEdit: () => void;
  onDelete: () => void;
  onSend: () => void;
  /** #751 — sent cards only. Replies this browser has not seen yet (the last N rows). */
  unread?: number;
  /** A reply just landed over the WebSocket: outline flash. */
  flash?: boolean;
  /** A Resolve / Reopen request is in flight for this comment. */
  deciding?: boolean;
  onResolve?: () => void;
  onReopen?: () => void;
  /** The card was looked at (hovered, or in view for a moment): clear its unread dots. */
  onSeen?: () => void;
  /** #752 — sent cards only: the line changed on the displayed destination. `writtenOn` labels the ref it was written against. */
  outdated?: { writtenOn: string };
  /** #752 — sent cards only: the written line, when the reported number differs. */
  movedFrom?: number;
}

const REMARK_PLUGINS = [remarkGfm];

export default function CommentCard({
  entry,
  sending,
  startingManager,
  pairLabel,
  sendDisabledReason,
  onEdit,
  onDelete,
  onSend,
  unread = 0,
  flash = false,
  deciding = false,
  onResolve,
  onReopen,
  onSeen,
  outdated,
  movedFrom,
}: Props) {
  const label = anchorLabel(entry.anchor);
  const body = (text: string) => (
    <div
      className="artifact-markdown px-2.5 py-2 text-fg [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:list-disc [&_ul]:pl-5"
      style={{ fontSize: "11.5px", lineHeight: 1.5 }}
      data-testid="review-comment-body"
    >
      <Markdown remarkPlugins={REMARK_PLUGINS}>{text}</Markdown>
    </div>
  );

  if (entry.kind === "sent") {
    return (
      <SentCard
        comment={entry.comment}
        label={label}
        body={body}
        pairLabel={pairLabel}
        unread={unread}
        flash={flash}
        deciding={deciding}
        onResolve={onResolve}
        onReopen={onReopen}
        onSeen={onSeen}
        outdated={outdated}
        movedFrom={movedFrom}
      />
    );
  }

  const d = entry.draft;
  if (sending) {
    return (
      <div
        className="my-2 ml-3 mr-3 max-w-[720px] overflow-hidden rounded-md border border-line-strong bg-bg-2 font-sans opacity-70"
        style={{ fontSize: "11px", whiteSpace: "normal" }}
        data-testid="review-comment"
        data-state="sending"
        data-anchor={label}
      >
        <div className="flex items-center gap-2 border-b border-line bg-bg-3 px-2.5 py-[5px] text-fg-3" style={{ fontSize: "10.5px" }}>
          <Badge kind="sent">
            <Spinner /> Sending
          </Badge>
          <span className="font-medium text-fg-2">you</span>
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {label}
          </span>
          <span className="text-fg-4">· {startingManager ? "starting the manager…" : "handing over…"}</span>
        </div>
        {body(d.text)}
      </div>
    );
  }

  return (
    <div
      className="group my-2 ml-3 mr-3 max-w-[720px] overflow-hidden rounded-md border border-line-strong bg-bg-2 font-sans"
      style={{ fontSize: "11px", whiteSpace: "normal" }}
      data-testid="review-comment"
      data-state="draft"
      data-anchor={label}
    >
      <div className="flex items-center gap-2 border-b border-line bg-bg-3 px-2.5 py-[5px] text-fg-3" style={{ fontSize: "10.5px" }}>
        <Badge kind="draft">✎ Draft</Badge>
        <span className="font-medium text-fg-2">you</span>
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {label}
        </span>
        <span className="text-fg-4">· {relativeTime(d.updated_at)}</span>
        <span className="ml-auto inline-flex gap-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
          <button
            type="button"
            onClick={onEdit}
            title="Edit draft"
            data-testid="review-comment-edit"
            className="grid h-5 w-[22px] cursor-pointer place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg-2"
          >
            <Pencil size={11} />
          </button>
          <button
            type="button"
            onClick={onDelete}
            title="Delete draft"
            data-testid="review-comment-delete"
            className="grid h-5 w-[22px] cursor-pointer place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-st-failed"
          >
            <Trash2 size={11} />
          </button>
        </span>
      </div>
      {body(d.text)}
      <div className="flex items-center gap-2 border-t border-line px-2.5 py-[5px] text-fg-4" style={{ fontSize: "10px" }}>
        <span>Only in this browser until sent.</span>
        <span className="ml-auto">
          <button
            type="button"
            onClick={onSend}
            disabled={!!sendDisabledReason}
            title={sendDisabledReason ?? "Send this comment to the manager (one message)"}
            data-testid="review-comment-send"
            className="cursor-pointer rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 hover:bg-bg-4 hover:text-fg disabled:cursor-not-allowed disabled:opacity-45"
            style={{ fontSize: "10.5px" }}
          >
            ↗ Send to manager
          </button>
        </span>
      </div>
    </div>
  );
}
