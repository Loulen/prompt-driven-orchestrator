interface Props {
  /** Live NodeRun sessions counted daemon-wide (managers excluded). */
  live: number;
  /** Configured global session cap. */
  cap: number;
}

/**
 * Live session counter for the bottom status bar (#159 / ADR-0012).
 *
 * Shows "N / cap sessions" and shifts to a warning (amber) treatment as the
 * count approaches the cap, so throttling is visible before it bites. Rendered
 * inline among the other technical info in the bottom status bar.
 */
export default function SessionCounter({ live, cap }: Props) {
  // Warn within one slot of the cap (and once it is full): throttling is either
  // imminent or already happening.
  const nearCap = cap > 0 && live >= cap - 1;

  return (
    <span
      data-testid="session-counter"
      data-near-cap={nearCap}
      className={nearCap ? "font-medium text-st-await" : undefined}
      title="Live NodeRun sessions (manager sessions excluded)"
    >
      {live} / {cap} sessions
    </span>
  );
}
