import { TriangleAlert } from "lucide-react";
import type { ServiceHealth } from "../../shared/generated/ServiceHealth";
import { Tooltip } from "../../shared/ui/kit";

interface Props {
  /** The daemon's cached service health, or undefined until it responds. */
  service?: ServiceHealth;
}

/**
 * Persistence signal for the bottom status bar (#156 / ADR-0019).
 *
 * Silence-when-healthy: renders **nothing** when the daemon is persistent
 * (`persistent === true`) or its state is unknown (`persistent === null` /
 * absent). Renders an amber warning icon ONLY when the daemon is reachable but
 * NOT installed as a service (`persistent === false`) — the one signal the
 * connection icon structurally cannot express (reachable ≠ persistent). Its
 * name and the command that fixes it are in the tooltip. Amber
 * (`text-st-await`), like the reconnecting daemon and the near-cap counter:
 * never red, this is not an error.
 */
export default function ServiceHealthIndicator({ service }: Props) {
  if (service?.persistent !== false) return null;

  return (
    <Tooltip label="Ephemeral daemon" description="pdo service install" side="top">
      <span
        data-testid="service-ephemeral-pill"
        className="sh-sb__item text-st-await"
        aria-label="Ephemeral daemon: pdo service install"
        tabIndex={0}
      >
        <TriangleAlert size={13} strokeWidth={1.5} aria-hidden />
      </span>
    </Tooltip>
  );
}
