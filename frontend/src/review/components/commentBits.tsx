import { Bot, CheckCircle2, Cpu, Hourglass, MessageSquare, User } from "lucide-react";
import { useActorDisplay } from "../../shared/actors/store";
import type { Actor } from "../../shared/generated/Actor";
import { Avatar, Tooltip } from "../../shared/ui/kit";
import type { AuthorKind, CommentState } from "../model/commentStatus";
import { authorKind, authorLabel } from "../model/commentStatus";

// The small glyphs a review comment is drawn with: its state, its author, the
// draft / sent pill, the spinner of a send.

/**
 * A button of a gesture the caller may not make (CORE-205): with `reason`, its
 * tooltip names it (`label`) and says the refusal's sentence; the button
 * itself carries `aria-disabled` and skips its handler.
 */
export function Refusable({ reason, label, children }: { reason?: string; label: string; children: React.ReactElement }) {
  return reason ? (
    <Tooltip label={label} description={reason}>
      {children}
    </Tooltip>
  ) : (
    children
  );
}

/** The state icon — message-square open, hourglass proposed, check-circle resolved. */
export function StateIcon({ state, title, size = 12 }: { state: CommentState; title?: string; size?: number }) {
  const cls = state === "proposed" ? "text-st-await" : state === "resolved" ? "text-st-done" : "text-text-3";
  return (
    <span className={`inline-flex items-center ${cls}`} title={title} data-testid={`review-state-${state}`}>
      {state === "proposed" ? <Hourglass size={size} /> : state === "resolved" ? <CheckCircle2 size={size} /> : <MessageSquare size={size} />}
    </span>
  );
}

/**
 * The author icon — person you, bot manager, chip node — name in the tooltip.
 * CORE-63: a person an extension names shows as its avatar and name; an agent
 * keeps its icon, the person it acts for in the tooltip.
 */
export function AuthorIcon({ author, actor, size = 12 }: { author: string; actor?: Actor | null; size?: number }) {
  const kind = authorKind(author);
  const display = useActorDisplay(actor);
  if (display && kind === "user") {
    return (
      <span className="inline-flex min-w-0 items-center gap-1 text-text-2" title={display.name} data-testid="review-author" data-author-kind={kind}>
        <Avatar name={display.name} actor={actor} src={display.avatar_url} size={16} />
        <span className="truncate font-medium" data-testid="review-author-name">
          {display.name}
        </span>
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1 text-text-3" title={authorLabel(author, display?.name)} data-testid="review-author" data-author-kind={kind}>
      <AuthorGlyph kind={kind} size={size} />
      {kind === "node" && (
        <span className="font-mono text-text-2 text-2xs">
          {author}
        </span>
      )}
    </span>
  );
}

export function AuthorGlyph({ kind, size }: { kind: AuthorKind; size: number }) {
  return kind === "user" ? <User size={size} /> : kind === "manager" ? <Bot size={size} /> : <Cpu size={size} />;
}

/** The amber (draft) / blue (sent) pill the badges, sidebar and headers share. */
export function Badge({ kind, children, title }: { kind: "draft" | "sent"; children: React.ReactNode; title?: string }) {
  return (
    <span
      title={title}
      className={`text-2xs inline-flex h-[14px] items-center gap-[3px] rounded-[7px] px-[5px] font-medium ${
        kind === "draft" ? "bg-st-await/14 text-st-await" : "bg-st-running/14 text-st-running"
      }`}
      data-testid={`review-badge-${kind}`}
    >
      {children}
    </span>
  );
}

export function Spinner({ dim }: { dim?: boolean }) {
  return (
    <span
      aria-hidden
      className="inline-block h-[9px] w-[9px] animate-spin rounded-full border-[1.5px] border-text-3"
      style={{ borderTopColor: dim ? "var(--text-3)" : "var(--color-st-running)" }}
    />
  );
}
