import { useEffect, useState, type ReactNode } from "react";
import { Bot, SquareTerminal } from "lucide-react";
import { Chip } from "../../../shared/ui/kit";
import { durationSince, isLive, kindLabel, stateLabel, stateSince, stateTone } from "../../model/hostedSession";
import { useHostedSession } from "../../store/useHostedSession";
import { useInstanceAgent } from "../../store/useInstanceAgent";
import "./hostedSession.css";

/** How often the duration on show moves. */
const TICK_MS = 30_000;

function useNow(every: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), every);
    return () => clearInterval(timer);
  }, [every]);
  return now;
}

/**
 * The head of a Session hébergée's detail (CORE-125): its kind's icon, its
 * name, its state and its harness (the instance's when it inherits it), then in mono its kind, how long it has
 * been in that state, and what the host adds (`context`: where it sits).
 * `actions` go at the right of the first line (the host's close button).
 */
export default function SessionHeader({
  session: id,
  name,
  context,
  actions,
}: {
  session: string;
  /** What the host calls it; its key by default. */
  name?: string;
  context?: ReactNode;
  actions?: ReactNode;
}) {
  const { session, events } = useHostedSession(id);
  const now = useNow(TICK_MS);
  const { inherited } = useInstanceAgent();
  if (!session) {
    return (
      <header className="hs-head" data-testid="hosted-session-header">
        <div className="hs-head__line">
          <span className="hs-head__name">{name ?? id}</span>
          <span className="hs-head__fill" />
          {actions}
        </div>
      </header>
    );
  }
  const Icon = session.kind === "agent" ? Bot : SquareTerminal;
  const label = stateLabel(session);
  const at = stateSince(session, events);
  const since = isLive(session) ? durationSince(at, now) : `${label.toLowerCase()} ${durationSince(at, now)} ago`;
  return (
    <header className="hs-head" data-testid="hosted-session-header" data-state={session.state}>
      <div className="hs-head__line">
        <Icon size={14} strokeWidth={1.5} className="hs-head__icon" aria-hidden />
        <h2 className="hs-head__name" title={`${name ?? session.key} · ${session.id}`}>
          {name ?? session.key}
        </h2>
        <span data-testid="hosted-session-state">
          <Chip tone={stateTone(session)} title={session.wait_message ?? label}>
            {label}
          </Chip>
        </span>
        {session.kind === "agent" && (
          <span data-testid="hosted-session-harness" title={session.config.harness ? "Harness" : "Harness (inherited)"}>
            <Chip mono>{session.config.harness ?? inherited.harness}</Chip>
          </span>
        )}
        <span className="hs-head__fill" />
        {actions}
      </div>
      <div className="hs-head__meta">
        {[kindLabel(session.kind), since].filter(Boolean).join(" · ")}
        {context ? <> · {context}</> : null}
      </div>
    </header>
  );
}
