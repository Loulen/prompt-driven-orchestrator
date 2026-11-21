import { useEffect, useState } from "react";
import { CircleCheck, CircleX } from "lucide-react";
import type { ConnectionStatus } from "../run/store/useDaemonSocket";
import { useOperations, type Operation } from "../shared/http/operations";
import ExtensionIcon from "../shared/extensions/ExtensionIcon";
import { Tooltip } from "../shared/ui/kit";
import { TooltipLayer } from "../shared/ui/kit/tip";
import { plural } from "../shared/lib/plural";
import { formatElapsed, ringCount } from "./daemonIndicatorText";

/** What the connection says, in the tooltip, when it is lost. */
const LOST: Record<Exclude<ConnectionStatus, "connected">, string> = {
  reconnecting: "Daemon: disconnected, reconnecting…",
  disconnected: "Daemon: disconnected",
};
const LOST_HINT = "Actions are refused until the daemon is back";

/** `Date.now()`, again every half second while `on`. */
function useNow(on: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!on) return;
    const tick = () => setNow(Date.now());
    const first = setTimeout(tick, 0);
    const t = setInterval(tick, 500);
    return () => {
      clearTimeout(first);
      clearInterval(t);
    };
  }, [on]);
  return now;
}

/** The ruby ring that turns, the count in its centre. */
function Ring({ count }: { count: number }) {
  return (
    <span className="sh-sb__ring" aria-hidden data-testid="daemon-ops-ring">
      <svg viewBox="0 0 16 16" className="sh-sb__ring-arc d-spin">
        <circle cx="8" cy="8" r="6.5" className="sh-sb__ring-track" />
        <circle cx="8" cy="8" r="6.5" className="sh-sb__ring-head" />
      </svg>
      <span className="sh-sb__ring-n">{ringCount(count)}</span>
    </span>
  );
}

/** The operations in flight, one row each: its spinner (or extension icon), label, duration. */
function OperationList({ operations, now, lost }: { operations: Operation[]; now: number; lost: string | null }) {
  return (
    <span className="sh-sb__ops" data-testid="daemon-ops-list">
      <span className="d-pop__group">In progress</span>
      {operations.map((o) => (
        <span key={o.id} className="d-pop__row" data-testid="daemon-ops-row">
          <span className="d-pop__mark">
            {o.icon ? (
              <ExtensionIcon url={o.icon} label={o.label} size={12} />
            ) : (
              <span className="sh-sb__mini-spin d-spin" aria-hidden />
            )}
          </span>
          <span className="d-pop__label">{o.label}</span>
          <span className="d-pop__hint">{formatElapsed(now - o.startedAt)}</span>
        </span>
      ))}
      {lost && <span className="d-pop__why">{lost}</span>}
    </span>
  );
}

/**
 * The daemon's indicator of the status bar: one icon for the connection and
 * the Opérations en cours. A grey check when connected and nothing lasts; a
 * ruby ring that turns, their count in its centre, while operations last
 * more than 300 ms; a red cross when the connection is lost, which wins (the
 * operations stay listed on hover). Hover or focus names the operation, or
 * lists them with their durations.
 */
export default function DaemonIndicator({ status }: { status: ConnectionStatus }) {
  const operations = useOperations().filter((o) => o.visible);
  const now = useNow(operations.length > 0);
  const lost = status === "connected" ? null : LOST[status];
  const count = operations.length;

  const name = lost
    ? `${lost}${count > 0 ? `, ${plural(count, "operation")} in progress` : ""}`
    : count === 0
      ? "Daemon: connected · nothing in progress"
      : count === 1
        ? operations[0].label
        : `${plural(count, "operation")} in progress`;

  const item = (
    <span
      className="sh-sb__item sh-sb__daemon"
      role="status"
      aria-live="polite"
      aria-label={name}
      data-st={status}
      data-ops={count}
      data-testid="daemon-status"
      tabIndex={0}
    >
      {lost ? (
        <CircleX size={13} strokeWidth={1.5} aria-hidden />
      ) : count > 0 ? (
        <Ring count={count} />
      ) : (
        <CircleCheck size={13} strokeWidth={1.5} aria-hidden />
      )}
      {/* The live region reads its text, not its label: the same words, for a screen reader. */}
      <span className="d-sr">{name}</span>
    </span>
  );

  // Several operations (or any while the connection is lost): their list, in a popover's look.
  if (count > 1 || (lost && count > 0)) {
    return (
      <TooltipLayer side="top" bubbleClassName="d-pop sh-sb__pop" content={<OperationList operations={operations} now={now} lost={lost} />}>
        {item}
      </TooltipLayer>
    );
  }
  return (
    <Tooltip label={lost ?? (count === 1 ? operations[0].label : name)} description={lost ? LOST_HINT : undefined} side="top">
      {item}
    </Tooltip>
  );
}
