import type { ReactNode } from "react";
import type { TourRect } from "../../hooks/useTour";

/**
 * The **Projecteur** (#823, CONTEXT.md § « Tours guidés ») — the overlay that dims
 * everything except the step's target and swallows the clicks that land elsewhere.
 *
 * It is built from four blockers (above / below / left / right of the hole) rather
 * than one dimmed sheet with a CSS mask, for one reason that decides everything: a
 * hole punched with a mask still eats the click. Four real elements leave the hole
 * *physically empty*, so the target below receives the user's click exactly as it
 * would outside a tour — the tour never has to forward or synthesise a gesture.
 *
 * It sits above modals and portal menus on purpose (`z-[120]`, one layer over the
 * `z-[70]` ceiling the rest of the app uses): a tour that guides through a dialog
 * has to dim around that dialog, not disappear behind it.
 */

interface Props {
  /** The lit rectangle. `null` dims the whole window (target missing / end card). */
  hole: TourRect | null;
  /**
   * On a step inside an open menu, the menu's box. The blockers are cut around
   * THIS instead of the hole, so every option stays clickable — a tour points at
   * the right one, it does not forbid the others (spec #821, story 17).
   */
  zone?: TourRect | null;
  /**
   * The zone IS the step's target (#825) — a whole panel to roam in rather than a
   * control to hit. The dim lightens, and the ring gives way to the dashed
   * outline: a ring around six hundred pixels reads as "click this", and the one
   * step that lights a panel is the one with nothing to click.
   */
  wide?: boolean;
  /** Clicking the dim quits nothing; it is absorbed. The popover goes here. */
  children?: ReactNode;
}

function blockerStyle(rect: TourRect | null, side: "top" | "bottom" | "left" | "right") {
  if (!rect) return undefined;
  switch (side) {
    case "top":
      return { top: 0, left: 0, right: 0, height: Math.max(0, rect.top) };
    case "bottom":
      return { top: rect.top + rect.height, left: 0, right: 0, bottom: 0 };
    case "left":
      return { top: rect.top, left: 0, width: Math.max(0, rect.left), height: rect.height };
    case "right":
      return { top: rect.top, left: rect.left + rect.width, right: 0, height: rect.height };
  }
}

export default function Projecteur({ hole, zone, wide, children }: Props) {
  // The cut-out the blockers leave open. A soft step opens the whole menu.
  const opening = zone ?? hole;
  const dim = wide ? "bg-tour-dim-soft" : "bg-tour-dim";

  // `overflow-hidden` on the root: a target that is momentarily off-screen puts
  // its ring and its bottom blocker thousands of pixels down, and an overlay that
  // grew to match would hand the page a scroll range that belongs to nothing.
  return (
    <div className="pointer-events-none fixed inset-0 z-[120] overflow-hidden" data-testid="projecteur">
      {opening ? (
        (["top", "bottom", "left", "right"] as const).map((side) => (
          <div
            key={side}
            data-testid={`projecteur-blocker-${side}`}
            className={`pointer-events-auto absolute ${dim}`}
            style={blockerStyle(opening, side)}
            // Absorbed, not acted on: a stray click during a tour should do
            // nothing at all — not quit, not advance, not reach the app.
            onClick={(e) => e.preventDefault()}
            onMouseDown={(e) => e.preventDefault()}
          />
        ))
      ) : (
        <div
          data-testid="projecteur-blocker-all"
          className={`pointer-events-auto absolute inset-0 ${dim}`}
          onClick={(e) => e.preventDefault()}
          onMouseDown={(e) => e.preventDefault()}
        />
      )}

      {/* The dashed zone of a portal menu — a hint, never a wall. */}
      {zone && (
        <div
          data-testid="projecteur-zone"
          className="pointer-events-none absolute rounded-md border border-dashed border-acc-border"
          style={{ top: zone.top, left: zone.left, width: zone.width, height: zone.height }}
        />
      )}

      {/* The ring on the target itself. Never interactive: it must not stand
          between the pointer and the thing the instruction points at. */}
      {hole && !wide && (
        <div
          data-testid="projecteur-hole"
          className="pointer-events-none absolute rounded-md ring-2 ring-acc"
          style={{ top: hole.top, left: hole.left, width: hole.width, height: hole.height }}
        />
      )}

      {children}
    </div>
  );
}
