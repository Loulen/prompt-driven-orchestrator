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
      <code key={i} className="d-code">
        {part.slice(1, -1)}
      </code>
    ) : (
      <Fragment key={i}>{part}</Fragment>
    ),
  );
}
