import { Fragment, type ReactNode } from "react";
import { CircleCheck, TriangleAlert } from "lucide-react";
import type { TourDef, TourFailure } from "../../lib/tour";
import { TOUR_CATALOG } from "../../lib/tours";

/**
 * The two cards that end a tour (#823): the recap when it reaches the last step,
 * and the honest stop when a target never appeared.
 *
 * Both live on the Projecteur layer, over a fully dimmed window — at that point
 * there is no target left to point at, and the choice ("Finish", "Back to tours")
 * is the only thing left to do.
 */

/** Render `\`code\`` fragments as mono, so a recap can name a field or a value
 *  the way the UI spells it without the definition carrying JSX. */
function withCode(text: string): ReactNode {
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

function CardShell({ testId, children }: { testId: string; children: ReactNode }) {
  return (
    <div className="pointer-events-auto absolute inset-0 flex items-center justify-center">
      <div
        data-testid={testId}
        className="w-[420px] rounded-lg border border-line-strong bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
      >
        {children}
      </div>
    </div>
  );
}

export function TourEndCard({
  tour,
  onFinish,
  onStartTour,
}: {
  tour: TourDef;
  onFinish: () => void;
  onStartTour: (tourId: string) => void;
}) {
  const others = TOUR_CATALOG.filter((entry) => entry.id !== tour.id);

  return (
    <CardShell testId="tour-end-card">
      <div className="flex items-center gap-2">
        <CircleCheck size={18} className="shrink-0 text-acc" />
        <h2 className="font-semibold text-fg" style={{ fontSize: "14px" }}>
          {tour.title} — done
        </h2>
      </div>
      <p className="mt-2 text-fg-2" style={{ fontSize: "11.5px", lineHeight: 1.55 }}>
        {withCode(tour.recapIntro)}
      </p>
      <ul className="mt-2 flex flex-col gap-1.5">
        {tour.recap.map((item) => (
          <li
            key={item.label}
            className="flex gap-1.5 text-fg-2"
            style={{ fontSize: "11px", lineHeight: 1.55 }}
          >
            <span className="text-fg-4">•</span>
            <span>
              <span className="font-semibold text-fg">{item.label}</span> · {withCode(item.text)}
            </span>
          </li>
        ))}
      </ul>

      {others.length > 0 && (
        <>
          <div
            className="mt-4 font-semibold uppercase tracking-wide text-fg-4"
            style={{ fontSize: "9.5px", letterSpacing: "0.08em" }}
          >
            Next
          </div>
          <div className="mt-1.5 flex flex-col gap-1.5">
            {others.map((entry) => (
              <button
                key={entry.id}
                type="button"
                disabled={entry.def == null}
                data-testid={`tour-end-next-${entry.id}`}
                onClick={() => entry.def && onStartTour(entry.id)}
                className="flex items-center gap-2 rounded border border-line bg-bg-3 px-2.5 py-2 text-left transition-colors enabled:cursor-pointer enabled:hover:border-acc-border disabled:opacity-45"
              >
                <span className="min-w-0 flex-1">
                  <span className="block font-medium text-fg" style={{ fontSize: "11.5px" }}>
                    {entry.title}
                  </span>
                  <span className="block text-fg-3" style={{ fontSize: "10.5px" }}>
                    {entry.blurb}
                  </span>
                </span>
                <span className="shrink-0 font-mono text-fg-4" style={{ fontSize: "9.5px" }}>
                  {entry.def ? `~${entry.minutes} min` : "soon"}
                </span>
              </button>
            ))}
          </div>
        </>
      )}

      <div className="mt-4 flex justify-end">
        <button
          type="button"
          onClick={onFinish}
          data-testid="tour-finish"
          className="cursor-pointer rounded bg-acc px-3 py-1 font-medium text-on-acc transition-colors hover:bg-acc-dim"
          style={{ fontSize: "11.5px" }}
        >
          Finish
        </button>
      </div>
    </CardShell>
  );
}

export function TourFailedCard({
  tour,
  failure,
  onClose,
  onBackToTours,
}: {
  tour: TourDef;
  failure: TourFailure;
  onClose: () => void;
  onBackToTours: () => void;
}) {
  return (
    <CardShell testId="tour-failed-card">
      <div
        className="flex items-center gap-1.5 font-semibold uppercase tracking-wide text-st-await"
        style={{ fontSize: "9.5px", letterSpacing: "0.08em" }}
      >
        <TriangleAlert size={12} />
        {tour.title} · step {failure.stepNumber} of {failure.total}
      </div>
      <h2 className="mt-1.5 font-semibold text-fg" style={{ fontSize: "14px" }}>
        This step could not be completed
      </h2>
      <p className="mt-1.5 text-fg-2" style={{ fontSize: "11.5px", lineHeight: 1.55 }}>
        The tour was waiting for <span className="font-medium text-fg">{failure.waitingFor}</span>, but
        it did not appear.{failure.hint ? ` ${failure.hint}` : ""}
      </p>
      <p className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px", lineHeight: 1.55 }}>
        Nothing was created, changed or deleted — a tour only ever watches.
      </p>
      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onClose}
          data-testid="tour-failed-close"
          className="cursor-pointer rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
          style={{ fontSize: "11.5px" }}
        >
          Close
        </button>
        <button
          type="button"
          onClick={onBackToTours}
          data-testid="tour-failed-back"
          className="cursor-pointer rounded bg-acc px-3 py-1 font-medium text-on-acc transition-colors hover:bg-acc-dim"
          style={{ fontSize: "11.5px" }}
        >
          Back to tours
        </button>
      </div>
    </CardShell>
  );
}
