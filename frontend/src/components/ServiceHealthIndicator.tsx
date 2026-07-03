import type { ServiceHealth } from "../types";

interface Props {
  /** The daemon's cached service health, or undefined until it responds. */
  service?: ServiceHealth;
}

/**
 * Persistence signal for the bottom status bar (#156 / ADR-0019).
 *
 * Silence-when-healthy: renders **nothing** when the daemon is persistent
 * (`persistent === true`) or its state is unknown (`persistent === null` /
 * absent). Renders a small amber `ephemeral` pill ONLY when the daemon is
 * reachable but NOT installed as a service (`persistent === false`) — the one
 * signal the connection dot structurally cannot express (reachable ≠
 * persistent). Uses the same amber token (`text-st-await`) the reconnecting dot
 * and the near-cap counter use — no new colour vocabulary, and never red (this
 * is not an error).
 */
export default function ServiceHealthIndicator({ service }: Props) {
  if (service?.persistent !== false) return null;

  return (
    <span
      data-testid="service-ephemeral-pill"
      className="font-medium text-st-await"
      title="Daemon not installed as a service — stops on logout/reboot. Run `pdo service install`."
    >
      ephemeral
    </span>
  );
}
