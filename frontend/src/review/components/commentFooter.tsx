import { CheckCircle2, Hourglass, Undo2 } from "lucide-react";
import { useActorDisplay } from "../../shared/actors/store";
import { authorKind, authorLabel, footerStatus, relativeTime } from "../model/commentStatus";
import { AuthorGlyph, Spinner } from "./commentBits";

// The footer of a sent comment: what happened last, and the human's decision.

export function FooterStatusView({ status }: { status: ReturnType<typeof footerStatus> }) {
  // CORE-63: the name an extension gives the Actor who acted, in the tooltips.
  const name = useActorDisplay(status.kind === "awaiting" ? null : status.actor)?.name;
  switch (status.kind) {
    case "awaiting":
      return (
        <>
          <Spinner dim />
          Awaiting manager reply
        </>
      );
    case "replied":
      return (
        <span className="inline-flex items-center gap-1" title={`Replied by ${authorLabel(status.author, name)}`}>
          <AuthorGlyph kind={authorKind(status.author)} size={10} /> {relativeTime(status.at)}
        </span>
      );
    case "proposed":
      return (
        <span className="inline-flex items-center gap-1 text-st-await" title={`Resolution proposed by ${authorLabel(status.author, name)}`}>
          <Hourglass size={10} />
          <AuthorGlyph kind={authorKind(status.author)} size={10} /> <span className="text-fg-4">{relativeTime(status.at)}</span>
        </span>
      );
    case "resolved":
      return (
        <span className="inline-flex items-center gap-1 text-st-done" title={`Resolved by ${authorLabel(status.by, name)}`}>
          <CheckCircle2 size={10} />
          <AuthorGlyph kind={authorKind(status.by)} size={10} /> <span className="text-fg-4">{relativeTime(status.at)}</span>
        </span>
      );
    case "reopened":
      return (
        <span className="inline-flex items-center gap-1" title={`Reopened by ${authorLabel(status.by, name)}`}>
          <Undo2 size={10} />
          <AuthorGlyph kind={authorKind(status.by)} size={10} /> {relativeTime(status.at)}
        </span>
      );
    case "declined":
      return (
        <span className="inline-flex items-center gap-1" title={`Proposal declined by ${authorLabel(status.by, name)} — open for the agent`}>
          <Undo2 size={10} />
          <AuthorGlyph kind={authorKind(status.by)} size={10} /> {relativeTime(status.at)}
        </span>
      );
  }
}

export function DecisionButton({
  children,
  onClick,
  disabled,
  title,
  testId,
  primary,
  ghost,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled: boolean;
  title: string;
  testId: string;
  primary?: boolean;
  ghost?: boolean;
}) {
  const look = primary
    ? "border-acc bg-acc font-semibold text-on-acc hover:bg-acc-hi"
    : ghost
      ? "border-transparent bg-transparent text-fg-4 hover:bg-bg-4 hover:text-fg-2"
      : "border-line-strong bg-bg-3 text-fg-2 hover:bg-bg-4 hover:text-fg";
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onClick?.();
      }}
      disabled={disabled}
      title={title}
      data-testid={testId}
      className={`inline-flex cursor-pointer items-center gap-1 rounded border px-2 py-0.5 disabled:cursor-not-allowed disabled:opacity-45 ${look}`}
      style={{ fontSize: "10.5px" }}
    >
      {children}
    </button>
  );
}
