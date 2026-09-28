import { Fragment, type ReactNode } from "react";

/**
 * Text helpers shared by the cards that bracket a tour (`TourIntroCard.tsx`,
 * `TourCards.tsx`), kept apart from them so each card file only exports
 * components (React Fast Refresh).
 */

/** Render `\`code\`` fragments as mono, so a recap can name a field or a value
 *  the way the UI spells it without the definition carrying JSX. */
export function withCode(text: string): ReactNode {
  return text.split(/(`[^`]+`)/g).map((part, i) =>
    part.startsWith("`") && part.endsWith("`") && part.length > 1 ? (
      <span key={i} className="rounded bg-bg-4 px-1 font-mono text-fg-2" style={{ fontSize: "10px" }}>
        {part.slice(1, -1)}
      </span>
    ) : (
      <Fragment key={i}>{part}</Fragment>
    ),
  );
}
