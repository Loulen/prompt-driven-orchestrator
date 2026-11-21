import { useEffect, useState } from "react";
import { Clock } from "lucide-react";
import { Button } from "../../../shared/ui/kit";
import { Modal } from "../../../shared/ui/kit/Modal";
import { useSupportAgent, type StillThere } from "../../store/supportAgentStore";
import { countdown, idleDelay } from "../../model/supportAgent";

/** How often the countdown redraws. */
const TICK_MS = 250;

/** Past the end of the countdown by this much, the daemon's word was lost (a
 *  dropped message): the agent is read again. */
export const OVERDUE_MS = 10_000;

/**
 * « Still there? »: the agent had no input for the instance's idle timeout,
 * and closes when the countdown ends. Keep (the default, and Escape) starts
 * its clock over; Close now ends it. An answer in any tab closes the window
 * in the others; nobody answering, the daemon closes the agent and a toast
 * says so.
 */
export default function SupportStillThere({ asked, harness }: { asked: StillThere; harness: string }) {
  const keep = useSupportAgent((s) => s.keep);
  const end = useSupportAgent((s) => s.end);
  const pending = useSupportAgent((s) => s.pending);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), TICK_MS);
    return () => window.clearInterval(timer);
  }, []);
  const overdue = now - asked.closesAt > OVERDUE_MS;
  useEffect(() => {
    if (overdue) void useSupportAgent.getState().refresh();
  }, [overdue]);
  const agent = harness ? `The ${harness} support agent` : "The support agent";
  return (
    <Modal
      size="s"
      icon={<Clock size={14} strokeWidth={1.5} aria-hidden />}
      title="Still there?"
      onClose={() => void keep()}
      onSubmit={() => void keep()}
      data-testid="support-still-there"
      actions={
        <>
          <Button disabled={pending} onClick={() => void end()} data-testid="support-still-there-close">
            Close now
          </Button>
          <Button
            tone="primary"
            data-autofocus=""
            disabled={pending}
            onClick={() => void keep()}
            data-testid="support-still-there-keep"
          >
            Keep
          </Button>
        </>
      }
    >
      <div className="sa-still">
        <p className="sa-still__lead">
          {agent} has had no input for {idleDelay(asked.idleTimeoutSecs)}.
        </p>
        <p className="sa-still__note">
          It closes in{" "}
          <span className="sa-still__count" data-testid="support-still-there-countdown">
            {countdown(asked.closesAt - now)}
          </span>
          .
        </p>
      </div>
    </Modal>
  );
}
