import type { HostedSessionEvent } from "../../../shared/generated/HostedSessionEvent";
import type { HostedSessionEventKind } from "../../../shared/generated/HostedSessionEventKind";
import { EmptyState } from "../../../shared/ui/kit";
import { History } from "lucide-react";
import ActorLabel from "../../../shared/actors/ActorLabel";
import { liveUpdatedAgo } from "../../../run/model/liveArtifacts";
import { useHostedSession } from "../../store/useHostedSession";
import "./hostedSession.css";

const LABEL: Record<HostedSessionEventKind, string> = {
  opened: "Opened",
  running: "Running",
  awaiting_user: "Waiting for you",
  resumed: "Answered",
  terminated: "Terminated",
  exited: "Exited",
  relaunched: "Relaunched",
  configured: "Configuration changed",
};

/**
 * The journal of a Session hébergée (CORE-125), newest first:
 * what happened to it, by whom, when; what its agent asked while it waited.
 * Live.
 */
export default function SessionEvents({ session: id }: { session: string }) {
  const { events } = useHostedSession(id);
  if (events.length === 0) {
    return (
      <div className="hs-block" data-testid="hosted-session-events">
        <EmptyState icon={History} layout="row">
          No event yet
        </EmptyState>
      </div>
    );
  }
  return (
    <ol className="hs-events" data-testid="hosted-session-events">
      {[...events].reverse().map((event, i) => (
        <EventRow key={`${event.at}:${event.kind}:${i}`} event={event} />
      ))}
    </ol>
  );
}

function EventRow({ event }: { event: HostedSessionEvent }) {
  return (
    <li className="hs-event" data-testid="hosted-session-event" data-kind={event.kind}>
      <span className="hs-event__what">{LABEL[event.kind] ?? event.kind}</span>
      <span className="hs-event__by">{event.by ? <ActorLabel actor={event.by} /> : null}</span>
      <span className="hs-event__at" title={event.at}>
        {liveUpdatedAgo(event.at)}
      </span>
      {event.message && <span className="hs-event__message">“{event.message}”</span>}
    </li>
  );
}
