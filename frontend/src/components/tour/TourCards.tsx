import { Fragment, type ReactNode } from "react";
import { CircleCheck, Loader, TriangleAlert } from "lucide-react";
import {
  recapIntroOf,
  recapOf,
  type TourAppState,
  type TourDef,
  type TourFailure,
} from "../../lib/tour";
import type { TourPrepView } from "../../hooks/useTour";
import { TOUR_CATALOG } from "../../lib/tours";

/**
 * The cards that bracket a tour: the one it opens on (#824), the recap when it
 * reaches the last step, and the honest stop when a target never appeared or the
 * app refused (#823, #824).
 *
 * All of them live on the Projecteur layer, over a fully dimmed window — at those
 * moments there is no target to point at, and the choice on the card is the only
 * thing left to do.
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

/**
 * The card a tour opens on (#824): what it is about to do, the live state of its
 * preparations, and the only `Start` it has. Also its own error state — a
 * preparation the daemon refused stops the tour *before* step 1, quoting the
 * refusal, because a tour that began half-prepared would fail later on a step the
 * user was doing perfectly.
 */
export function TourIntroCard({
  tour,
  prep,
  onStart,
  onRetry,
  onClose,
}: {
  tour: TourDef;
  prep: TourPrepView;
  onStart: () => void;
  onRetry: () => void;
  onClose: () => void;
}) {
  const intro = tour.intro;
  if (!intro) return null;

  if (prep.failure) {
    return (
      <CardShell testId="tour-intro-card">
        <div
          className="flex items-center gap-1.5 font-semibold uppercase tracking-wide text-st-await"
          style={{ fontSize: "9.5px", letterSpacing: "0.08em" }}
        >
          <TriangleAlert size={12} />
          {tour.title} · could not prepare
        </div>
        <h2 className="mt-1.5 font-semibold text-fg" style={{ fontSize: "14px" }}>
          {prep.failure.title}
        </h2>
        <p className="mt-1.5 text-fg-3" style={{ fontSize: "11px" }}>
          The daemon answered:
        </p>
        <pre
          data-testid="tour-intro-reason"
          className="mt-1 whitespace-pre-wrap break-words rounded border border-line bg-bg-3 p-2 font-mono text-fg-2"
          style={{ fontSize: "10.5px", lineHeight: 1.5 }}
        >
          {prep.failure.reason}
        </pre>
        <p className="mt-2 text-fg-2" style={{ fontSize: "11px", lineHeight: 1.55 }}>
          Fix that, then retry. The tour did not start.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            data-testid="tour-intro-close"
            className="cursor-pointer rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
            style={{ fontSize: "11.5px" }}
          >
            Close
          </button>
          <button
            type="button"
            onClick={onRetry}
            data-testid="tour-intro-retry"
            className="cursor-pointer rounded bg-acc px-3 py-1 font-medium text-on-acc transition-colors hover:bg-acc-dim"
            style={{ fontSize: "11.5px" }}
          >
            Retry
          </button>
        </div>
      </CardShell>
    );
  }

  return (
    <CardShell testId="tour-intro-card">
      <div
        className="font-semibold uppercase tracking-wide text-acc"
        style={{ fontSize: "9.5px", letterSpacing: "0.08em" }}
      >
        {tour.title} · ~{tour.minutes} min · {tour.steps.length} steps
      </div>
      <h2 className="mt-1.5 font-semibold text-fg" style={{ fontSize: "14px" }}>
        {intro.title}
      </h2>
      <p className="mt-2 text-fg-2" style={{ fontSize: "11.5px", lineHeight: 1.55 }}>
        {withCode(intro.body)}
      </p>

      <ul className="mt-3 flex flex-col gap-1" data-testid="tour-intro-checklist">
        {prep.items.map((item) => (
          <li
            key={item.id}
            data-testid={`tour-intro-prep-${item.id}`}
            data-done={item.done ? "true" : "false"}
            className={`flex items-center gap-1.5 ${item.done ? "text-fg-2" : "text-fg-4"}`}
            style={{ fontSize: "11px" }}
          >
            {item.done ? (
              <CircleCheck size={12} className="shrink-0 text-acc" />
            ) : (
              <Loader size={12} className="shrink-0 animate-spin" />
            )}
            <span className="min-w-0 flex-1">{withCode(item.label)}</span>
          </li>
        ))}
      </ul>

      <p className="mt-3 text-fg-4" style={{ fontSize: "10.5px", lineHeight: 1.55 }}>
        {intro.footnote}
      </p>

      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onClose}
          data-testid="tour-intro-later"
          className="cursor-pointer rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
          style={{ fontSize: "11.5px" }}
        >
          Later
        </button>
        <button
          type="button"
          onClick={onStart}
          disabled={!prep.ready}
          data-testid="tour-intro-start"
          className="rounded bg-acc px-3 py-1 font-medium text-on-acc transition-colors enabled:cursor-pointer enabled:hover:bg-acc-dim disabled:cursor-not-allowed disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
        >
          Start
        </button>
      </div>
    </CardShell>
  );
}

export function TourEndCard({
  tour,
  app,
  nextTour,
  onFinish,
  onStartTour,
}: {
  tour: TourDef;
  /** What the tour last observed — the recap may name the Run the user launched. */
  app: TourAppState;
  /** The next leg of a Full tour: the primary then chains instead of closing. */
  nextTour?: TourDef | null;
  onFinish: () => void;
  onStartTour: (tourId: string) => void;
}) {
  // Mid-Full-tour, the remaining leg is the primary, so it is not also offered as
  // an afterthought below.
  const others = TOUR_CATALOG.filter(
    (entry) => entry.id !== tour.id && entry.id !== nextTour?.id,
  );
  const recap = recapOf(tour, app);

  return (
    <CardShell testId="tour-end-card">
      <div className="flex items-center gap-2">
        <CircleCheck size={18} className="shrink-0 text-acc" />
        <h2 className="font-semibold text-fg" style={{ fontSize: "14px" }}>
          {tour.outro?.title ?? `${tour.title} — done`}
        </h2>
      </div>
      <p className="mt-2 text-fg-2" style={{ fontSize: "11.5px", lineHeight: 1.55 }}>
        {withCode(recapIntroOf(tour, app))}
      </p>
      <ul className="mt-2 flex flex-col gap-1.5">
        {recap.map((item) => (
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

      {tour.outro?.closing && (
        <p className="mt-2.5 text-fg-3" style={{ fontSize: "11px", lineHeight: 1.55 }}>
          {withCode(tour.outro.closing)}
        </p>
      )}

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
          {/* In a Full tour the primary moves on; on its own it gets out of the
              way. Either way it is the same click — `onFinish` marks this tour
              done first, then chains or closes. */}
          {nextTour ? `Next tour · ${nextTour.title}` : (tour.outro?.primaryLabel ?? "Finish")}
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
        {failure.title ?? "This step could not be completed"}
      </h2>
      {failure.kind === "refused" ? (
        // The app said no in its own words (#824). Quoted, never paraphrased: the
        // reason is the only thing that tells the user what to go and fix.
        <>
          <p className="mt-1.5 text-fg-3" style={{ fontSize: "11px" }}>
            The daemon refused:
          </p>
          <pre
            data-testid="tour-failed-reason"
            className="mt-1 whitespace-pre-wrap break-words rounded border border-line bg-bg-3 p-2 font-mono text-fg-2"
            style={{ fontSize: "10.5px", lineHeight: 1.5 }}
          >
            {failure.reason}
          </pre>
          {failure.hint && (
            <p className="mt-2 text-fg-2" style={{ fontSize: "11px", lineHeight: 1.55 }}>
              {failure.hint}
            </p>
          )}
        </>
      ) : (
        <>
          <p className="mt-1.5 text-fg-2" style={{ fontSize: "11.5px", lineHeight: 1.55 }}>
            The tour was waiting for <span className="font-medium text-fg">{failure.waitingFor}</span>,
            but it did not appear.{failure.hint ? ` ${failure.hint}` : ""}
          </p>
          <p className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px", lineHeight: 1.55 }}>
            Nothing was created, changed or deleted — a tour only ever watches.
          </p>
        </>
      )}
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
