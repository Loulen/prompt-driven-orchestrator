import { AlertCircle } from "lucide-react";
import type { AwaitingInfo } from "../../../shared/generated/AwaitingInfo";
import { useWaitingLabel } from "../../store/useWaitingLabel";

/**
 * #588 / ADR-0069 — the awaiting-user banner of a NodeRun (design v2, 2026-09-12).
 * One amber shape, four contents keyed on `awaiting.cause`:
 * - `declared` (A1/A2): « The agent is waiting for your answer », the agent's
 *   message verbatim when there is one, the Enter hint (or the frozen-pane hint
 *   when no live socket, A4; « take control to reply » for a spectator, #870);
 * - `completion_not_released` (A3): « The agent is done and waits for your go »,
 *   the daemon's sentence with the two buttons as inline links;
 * - `child_awaiting` (C): « A child run is waiting for you » + « Open <child> → »,
 *   and explicitly NO Enter hint — Enter in the parent's pane lifts nothing;
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
  const linkClass = "cursor-pointer underline decoration-st-await/60 hover:decoration-st-await";
  let title: string;
  let body: React.ReactNode;
  let hint: React.ReactNode = null;
  if (cause === "completion_not_released") {
    title = "The agent is done and waits for your go";
    body = (
      <>
        Completion not released. Click{" "}
        {canRelease ? (
          <button type="button" className={linkClass} onClick={onRelease} data-testid="awaiting-banner-release">
            Mark ready for completion
          </button>
        ) : (
          <em>Mark ready for completion</em>
        )}{" "}
        so the agent can finish, or{" "}
        {canMarkComplete ? (
          <button type="button" className={linkClass} onClick={onMarkComplete} data-testid="awaiting-banner-mark-complete">
            Mark complete
          </button>
        ) : (
          <em>Mark complete</em>
        )}{" "}
        to take the artifacts as they are.
      </>
    );
    hint = !terminalLive
      ? "This pane is a snapshot — reopen the session to keep talking, or use the buttons."
      : terminalSpectating
        ? "Not done yet? Another browser has control — take control to reply in the terminal."
        : "Not done yet? Reply in the terminal and press Enter — the agent keeps working.";
  } else if (cause === "child_awaiting") {
    title = "A child run is waiting for you";
    const childId = awaiting?.child_run_id ?? null;
    body = (
      <>
        {awaiting?.message ?? "a child run is awaiting you"} — answer it there; this node resumes on its own.
      </>
    );
    hint = (
      <span className="flex items-center gap-2">
        <span>Nothing to type here: this node is blocked in pdo run wait, not asking a question.</span>
        {childId && onOpenChildRun && (
          <button
            type="button"
            data-testid="awaiting-open-child"
            className={`ml-auto shrink-0 ${linkClass}`}
            onClick={() => onOpenChildRun(childId)}
          >
            Open {childId} →
          </button>
        )}
      </span>
    );
  } else if (cause === "children_pending") {
    title = "Waiting for child runs";
    body = "This node gave its turn back and completes once every child run is terminal.";
  } else {
    // `declared` — with or without a message.
    title = awaiting?.message ? "The agent is waiting for your answer" : "The agent is waiting for you";
    body = awaiting?.message ?? null;
    hint = !terminalLive
      ? "This pane is a snapshot — reopen the session to reply, or use the buttons below."
      : terminalSpectating
        ? "Another browser has control of the terminal below — take control to reply, then press Enter to resume."
        : awaiting?.message
        ? "Reply in the terminal below and press Enter to resume."
        : "Open the terminal below to see what it asked, reply and press Enter to resume.";
  }
  return (
    <div
      className="flex items-start gap-2 border-b border-st-await/30 bg-st-await/14 px-3 py-2"
      data-testid="awaiting-banner"
      data-cause={cause}
      data-terminal-live={terminalLive}
      data-terminal-spectating={terminalSpectating}
    >
      <AlertCircle size={14} className="mt-0.5 shrink-0 text-st-await" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-baseline gap-2">
          <span
            className="text-st-await"
            style={{ fontSize: "11.5px", fontWeight: 600 }}
            data-testid="awaiting-banner-title"
          >
            {title}
          </span>
          {waitingLabel && (
            <span className="ml-auto shrink-0 text-st-await/70" style={{ fontSize: "9.5px" }} data-testid="awaiting-banner-since">
              {waitingLabel}
            </span>
          )}
        </div>
        {body && (
          <span className="break-words text-text" style={{ fontSize: "11px", lineHeight: 1.45 }} data-testid="awaiting-banner-message">
            {body}
          </span>
        )}
        {hint && (
          <span className="text-text-3" style={{ fontSize: "10px", lineHeight: 1.4 }} data-testid="awaiting-banner-hint">
            {hint}
          </span>
        )}
      </div>
    </div>
  );
}
