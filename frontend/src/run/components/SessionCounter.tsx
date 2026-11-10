import { Bot } from "lucide-react";
import { Tooltip } from "../../shared/ui/kit";

interface Props {
  /** Live NodeRun sessions counted daemon-wide (managers excluded). */
  live: number;
  /** Configured global session cap. */
  cap: number;
}

/**
 * Live session counter for the bottom status bar (#159 / ADR-0012).
 *
 * Shows "N/cap" after a bot icon, and shifts to the amber treatment as the
 * count approaches the cap, so throttling is visible before it bites. Its name
 * is in the tooltip.
 */
export default function SessionCounter({ live, cap }: Props) {
  // Warn within one slot of the cap (and once it is full): throttling is either
  // imminent or already happening.
  const nearCap = cap > 0 && live >= cap - 1;

  return (
    <Tooltip label="Sessions" description="Manager sessions excluded" side="top">
      <span
        data-testid="session-counter"
        data-near-cap={nearCap}
        className="sh-sb__item"
        aria-label={`${live} / ${cap} sessions`}
        tabIndex={0}
      >
        <Bot size={13} strokeWidth={1.5} aria-hidden />
        <span className="sh-sb__num">
          {live}/{cap}
        </span>
      </span>
    </Tooltip>
  );
}
