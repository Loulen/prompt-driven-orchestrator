import { Banner } from "../../../shared/ui/kit";
import { useHostedSession } from "../../store/useHostedSession";

/** What to do, in the tooltip and for a screen reader: an Enter alone does
 *  not answer the agent. */
export const WAITING_HINT = "Type your answer in the terminal, then press Enter";

/**
 * « Waiting for you » while the agent of a Session hébergée waits for a
 * person (`pdo wait-user`), with what it asked (CORE-125). Nothing otherwise.
 */
export default function SessionWaiting({ session: id }: { session: string }) {
  const { session } = useHostedSession(id);
  if (session?.state !== "awaiting_user") return null;
  const question = session.wait_message?.trim() || "its agent waits for an answer";
  return (
    <div data-testid="hosted-session-waiting" title={`${question} · ${WAITING_HINT}`}>
      <Banner tone="awaiting" title="Waiting for you" text={question} />
      <span className="sr-only">{WAITING_HINT}</span>
    </div>
  );
}
