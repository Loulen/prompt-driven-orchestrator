import { Check, LockOpen } from "lucide-react";
import { Button } from "../../../shared/ui/kit";
import NodeBanner from "./NodeBanner";
import type { AwaitingInfo } from "../../../shared/generated/AwaitingInfo";
import { useWaitingLabel } from "../../store/useWaitingLabel";

/**
 * #588 / ADR-0069 — the awaiting-user banner of a NodeRun: one tinted line,
 * four contents keyed on `awaiting.cause`, what to do in the tooltip:
 * - `declared` (A1/A2): « Question » and the agent's message verbatim; the
 *   Enter hint (the frozen-pane hint when no live socket, A4; « take control
 *   to reply » for a spectator, #870);
 * - `completion_not_released` (A3): « Done, awaiting approval », Mark ready and
 *   Approve;
 * - `child_awaiting` (C): « A child run is waiting for you » and Open, NO
 *   Enter hint — Enter in the parent's pane lifts nothing;
 * - `children_pending`: the orchestrator gave its turn back (ADR-0064).
 * `data-cause` is the assertion surface, the copy may move.
 */
export default function AwaitingBanner({
  awaiting,
  terminalLive,
  terminalSpectating = false,
  canRelease,
  canMarkComplete,
  onRelease,
  onMarkComplete,
  onOpenChildRun,
}: {
  awaiting: AwaitingInfo | null | undefined;
  /** A live PTY socket is open below: Enter can reach the agent. */
  terminalLive: boolean;
  /** #870: this browser only watches the terminal; its Enter reaches nothing
   *  until it takes control. */
  terminalSpectating?: boolean;
  canRelease: boolean;
  /** CORE-42: « Mark complete » is allowed to the caller (else plain text). */
  canMarkComplete: boolean;
  onRelease: () => void;
  onMarkComplete: () => void;
  onOpenChildRun?: (childRunId: string) => void;
}) {
  const cause = awaiting?.cause ?? "declared";
  const waitingLabel = useWaitingLabel(awaiting?.since);
  let tone: "awaiting" | "running" = "awaiting";
  let title: string;
  let text: string | undefined;
  let quote = false;
  let actions: React.ReactNode = null;
  // What to do, in the tooltip: the banner itself says the fact only.
  let hint: string | null = null;
  if (cause === "completion_not_released") {
    title = "Done, awaiting approval";
    text = "completion not released";
    actions = (
      <>
        {canRelease && (
          <Button size="sm" icon={LockOpen} onClick={onRelease} data-testid="awaiting-banner-release">
            Mark ready
          </Button>
        )}
        {canMarkComplete && (
          <Button size="sm" icon={Check} onClick={onMarkComplete} data-testid="awaiting-banner-mark-complete">
            Approve
          </Button>
        )}
      </>
    );
    hint = !terminalLive
      ? "This pane is a snapshot: reopen the session to keep talking, or use the buttons."
      : terminalSpectating
        ? "Another browser has control: take control to reply in the terminal."
        : "Not done yet? Reply in the terminal and press Enter: the agent keeps working.";
  } else if (cause === "child_awaiting") {
    title = "A child run is waiting for you";
    text = awaiting?.message ?? "answer it there";
    const childId = awaiting?.child_run_id ?? null;
    actions = childId && onOpenChildRun && (
      <Button size="sm" data-testid="awaiting-open-child" title={`Open ${childId}`} onClick={() => onOpenChildRun(childId)}>
        Open {childId.slice(-7)}
      </Button>
    );
    hint = "This node resumes on its own once the child run is answered.";
  } else if (cause === "children_pending") {
    tone = "running";
    title = "Waiting on child runs";
    text = "completes once every child run ends";
  } else {
    // `declared` — with or without a message.
    title = awaiting?.message ? "Question" : "Awaiting you";
    text = awaiting?.message ?? undefined;
    quote = true;
    hint = !terminalLive
      ? "This pane is a snapshot: reopen the session to reply."
      : terminalSpectating
        ? "Another browser has control of the terminal: take control to reply, then press Enter."
        : "Reply in the terminal and press Enter to resume.";
  }
  return (
    <NodeBanner
      tone={tone}
      title={title}
      text={text}
      quote={quote}
      aside={waitingLabel && <span data-testid="awaiting-banner-since">{waitingLabel}</span>}
      actions={actions}
      testId="awaiting-banner"
      tip={[title, text, hint].filter(Boolean).join(" · ")}
      data={{
        "data-cause": cause,
        "data-terminal-live": terminalLive,
        "data-terminal-spectating": terminalSpectating,
      }}
    >
      {hint && (
        <span className="sr-only" data-testid="awaiting-banner-hint">
          {hint}
        </span>
      )}
    </NodeBanner>
  );
}
