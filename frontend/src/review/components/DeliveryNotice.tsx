import { CircleAlert, PauseCircle, RotateCw } from "lucide-react";
import type { FooterStatus } from "../model/commentStatus";
import { DecisionButton } from "./commentFooter";

/**
 * CORE-239 — a sent comment held on its way to the Pipeline Manager, said on
 * its thread instead of « Awaiting manager reply » for ever: the manager's
 * agent shows a prompt only a person can answer (its folder-trust check — the
 * way out is in the Run's Manager tab, where its terminal is), or the manager
 * ended before answering (« Send again » hands every unanswered comment to a
 * new manager, as written).
 */
export default function DeliveryNotice({
  status,
  runHref,
  onResend,
  resending,
  resendCount,
  resendRefused,
}: {
  status: Extract<FooterStatus, { kind: "blocked" | "exited" }>;
  /** The Run's page, where its Manager tab is. */
  runHref: string;
  onResend?: () => void;
  resending: boolean;
  /** How many comments « Send again » hands over. */
  resendCount: number;
  /** Why sending is refused (a gone branch, the caller's rights): greyed, with why. */
  resendRefused?: string;
}) {
  const exited = status.kind === "exited";
  const text = status.reason ?? (exited ? "The manager exited before answering." : "The manager is waiting for you.");
  return (
    <div
      className={`rv-ct__notice ${exited ? "text-st-failed" : "text-st-await"}`}
      role="status"
      data-testid="review-comment-delivery"
      data-kind={status.kind}
    >
      {exited ? <CircleAlert size={12} aria-hidden /> : <PauseCircle size={12} aria-hidden />}
      <span className="rv-ct__notice-text text-text-2" data-testid="review-comment-delivery-reason">
        {text}
      </span>
      <span className="ml-auto inline-flex shrink-0 items-center gap-1.5">
        <a href={runHref} target="_blank" rel="noreferrer" className="text-text-2 underline decoration-dotted underline-offset-2 hover:text-text"
          data-testid="review-comment-open-run">
          Open the Run
        </a>
        {exited && (
          <DecisionButton
            primary
            onClick={onResend}
            disabled={resending || !onResend}
            disabledReason={resendRefused}
            title={`Hand ${resendCount === 1 ? "this comment" : `the ${resendCount} unanswered comments`} to a new manager, as written`}
            testId="review-comment-resend"
          >
            <RotateCw size={11} /> {resending ? "Sending…" : "Send again"}
          </DecisionButton>
        )}
      </span>
    </div>
  );
}
