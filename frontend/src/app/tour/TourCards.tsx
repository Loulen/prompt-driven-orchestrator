import { CircleCheck, Play, TriangleAlert } from "lucide-react";
import { recapIntroOf, recapOf, type TourFailure } from "../../lib/tour";
import type { TourAppState, TourDef } from "../../lib/tourDef";
import { TOUR_CATALOG } from "../../lib/tours";
import { Button } from "../../shared/ui/kit";
import { CardShell } from "./TourCardShell";
import { withCode } from "./tourCardText";
import TourTidyUpFailure from "./TourTidyUpFailure";

/**
 * The cards that bracket a tour: the one it opens on (#824), the recap when it
 * reaches the last step, and the honest stop when a target never appeared or the
 * app refused (#823, #824).
 *
 * All of them live on the Projecteur layer, over a fully dimmed window — at those
 * moments there is no target to point at, and the choice on the card is the only
 * thing left to do.
 *
 * The card a tour opens on lives in `TourIntroCard.tsx`.
 */

/**
 * The card a tour ends on — and, mid-Full-tour, the card BETWEEN two tours
 * (#825). Same recap either way; what changes is the way out.
 *
 * Alone, it offers the other tours and one `Finish`. Inside a chain it says how
 * far the Full tour has got, names the next leg, and puts the decision on two
 * explicit buttons: *Finish here* stops the chain, *Continue* starts the next
 * one. Both mark THIS tour done — it was finished. A single "Next tour" button
 * with the ✕ as the only way out would hide half of that choice, which is the
 * thing the welcome modal deliberately did not do.
 */
export function TourEndCard({
  tour,
  app,
  nextTour,
  /** How many legs of the Full tour are done / in total. Absent outside a chain. */
  chainProgress,
  tidyUpFailure,
  onFinish,
  onFinishHere,
  onStartTour,
}: {
  tour: TourDef;
  /** What the tour last observed — the recap may name the Run the user launched. */
  app: TourAppState;
  /** The next leg of a Full tour: the primary then chains instead of closing. */
  nextTour?: TourDef | null;
  chainProgress?: { done: number; total: number } | null;
  /** The tour's teardown failed while this card was up (#911). */
  tidyUpFailure?: { title: string; reason: string } | null;
  onFinish: () => void;
  onFinishHere: () => void;
  onStartTour: (tourId: string) => void;
}) {
  // Mid-Full-tour, the remaining leg is the primary, so it is not also offered as
  // an afterthought below — the chain already answers "what next".
  const others = nextTour ? [] : TOUR_CATALOG.filter((entry) => entry.id !== tour.id);
  const recap = recapOf(tour, app);

  const footer = (
    <>
      {nextTour && (
        <span className="d-meta min-w-0 truncate" title={nextTour.blurb} data-testid="tour-end-next-line">
          Next: {nextTour.title} · ~{nextTour.minutes} min
        </span>
      )}
      <span className="d-grow" />
      {nextTour && (
        <Button onClick={onFinishHere} data-testid="tour-finish-here">
          Finish here
        </Button>
      )}
      {/* Either way it is the same click — `onFinish` marks this tour done
          first, then chains or closes. */}
      <Button tone="primary" onClick={onFinish} data-testid="tour-finish">
        {nextTour ? "Continue" : (tour.outro?.primaryLabel ?? "Finish")}
      </Button>
    </>
  );
  const title = tour.outro?.title ?? `${tour.title} · done`;

  return (
    <CardShell testId="tour-end-card" label={title} footer={footer}>
      {chainProgress && (
        <div
          className="tr-chain"
          title={`Full tour · ${chainProgress.done} of ${chainProgress.total} done`}
          data-testid="tour-chain-progress"
        >
          {Array.from({ length: chainProgress.total }, (_, i) => (
            <span key={i} data-done={i < chainProgress.done ? "true" : "false"} />
          ))}
        </div>
      )}
      <div className="tr-head">
        <CircleCheck size={14} strokeWidth={1.5} className="text-primary" aria-hidden />
        {/* The recap's lead sentence is the title's tooltip: the card itself is the list. */}
        <h2 className="tr-bcard__title min-w-0 flex-1" title={recapIntroOf(tour, app).replaceAll("`", "") || undefined}>
          {title}
        </h2>
        {chainProgress && (
          <span className="d-meta tnum font-mono">
            {chainProgress.done}/{chainProgress.total}
          </span>
        )}
      </div>
      <dl className="tr-recap">
        {recap.map((item) => (
          <div key={item.label}>
            <dt title={item.label}>{item.label}</dt>
            <dd>{withCode(item.text)}</dd>
          </div>
        ))}
      </dl>
      {!nextTour && tour.outro?.closing && <p className="tr-note">{withCode(tour.outro.closing)}</p>}

      <TourTidyUpFailure failure={tidyUpFailure} />

      {others.length > 0 && (
        <div className="flex flex-col gap-1">
          <div className="d-caps tr-cap">Other tours</div>
          {others.map((entry) => (
            <button
              key={entry.id}
              type="button"
              disabled={entry.def == null}
              data-testid={`tour-end-next-${entry.id}`}
              onClick={() => entry.def && onStartTour(entry.id)}
              className="tr-row"
              title={entry.blurb}
            >
              <Play size={13} strokeWidth={1.5} className="tr-row__ic" aria-hidden />
              <span className="tr-row__text">
                <span className="tr-row__t">{entry.title}</span>
                <span className="tr-row__x">{entry.blurb}</span>
              </span>
              <span className="tr-min">{entry.def ? `~${entry.minutes} min` : "soon"}</span>
            </button>
          ))}
        </div>
      )}
    </CardShell>
  );
}

/**
 * The reassurance under a stopped tour's reason. « Nothing was created » is only
 * true of a tour with no intro preparations: *First run* and *Overview* set
 * things up before step 1, and *Overview* puts its example Trigger away on this
 * very card (#911). The steps themselves still only ever watch.
 */
function stopFootnote(tour: TourDef): string {
  const prepared = (tour.intro?.prepare.length ?? 0) > 0;
  if (!prepared) return "Nothing was created, changed or deleted — a tour only ever watches.";
  const tidied = (tour.teardown?.length ?? 0) > 0;
  return tidied
    ? "The steps changed nothing — a tour only ever watches. What its intro prepared stays yours, except what it set up only to show you, which is removed now."
    : "The steps changed nothing — a tour only ever watches. What its intro prepared stays yours.";
}

/**
 * The card a tour stops on: a target that never appeared, or an app that said no
 * out loud (#823, #824).
 *
 * Inside a Full tour it gains the chain's next leg as its primary (#825). A
 * refusal at Launch is a lesson about the machine — no harness on `PATH` — not a
 * reason to lose the tour that would have taught something else entirely. The
 * stopped tour is **not** marked done either way.
 */
export function TourFailedCard({
  tour,
  failure,
  nextTour,
  tidyUpFailure,
  onClose,
  onBackToTours,
  onContinue,
}: {
  tour: TourDef;
  failure: TourFailure;
  /** The pending leg of a Full tour, or `null` — the last leg stopping offers no
   *  Continue, because there is nothing to continue into. */
  nextTour?: TourDef | null;
  /** The tour's teardown failed while this card was up (#911). */
  tidyUpFailure?: { title: string; reason: string } | null;
  onClose: () => void;
  onBackToTours: () => void;
  onContinue: () => void;
}) {
  const title = failure.title ?? "This step could not be completed";
  const footer = (
    <>
      <span className="d-meta tnum font-mono">
        Step {failure.stepNumber} of {failure.total}
      </span>
      <span className="d-grow" />
      <Button onClick={onClose} data-testid="tour-failed-close">
        Close
      </Button>
      <Button tone={nextTour ? "default" : "primary"} onClick={onBackToTours} data-testid="tour-failed-back">
        Back to tours
      </Button>
      {nextTour && (
        <Button tone="primary" onClick={onContinue} data-testid="tour-failed-continue" title={nextTour.title}>
          Continue
        </Button>
      )}
    </>
  );

  return (
    <CardShell testId="tour-failed-card" label={title} footer={footer}>
      <div className="tr-head">
        <TriangleAlert size={13} strokeWidth={1.5} className="text-st-await" aria-hidden />
        <span className="d-caps tr-name tr-name--warn">{tour.title}</span>
        {nextTour && (
          <span className="d-meta" data-testid="tour-failed-chain">
            Full tour
          </span>
        )}
      </div>
      <h2 className="tr-bcard__title">{title}</h2>
      {failure.kind === "refused" ? (
        // The app said no in its own words (#824). Quoted, never paraphrased: the
        // reason is the only thing that tells the user what to go and fix.
        <>
          <pre data-testid="tour-failed-reason" className="tr-reason">
            {failure.reason}
          </pre>
          {failure.hint && <p className="tr-body">{failure.hint}</p>}
        </>
      ) : (
        <>
          <p className="tr-body">
            Waiting for <span className="font-medium text-text">{failure.waitingFor}</span>, which did not
            appear.{failure.hint ? ` ${failure.hint}` : ""}
          </p>
          <p className="tr-note">{stopFootnote(tour)}</p>
        </>
      )}
      {/* Why Continue is worth pressing, in the next tour's own words: the thing
          that just refused is not what the next leg needs. Only on a refusal —
          a target that failed to appear says nothing about the machine. */}
      {nextTour?.chainNote && failure.kind === "refused" && (
        <p className="tr-note" data-testid="tour-failed-chain-note">
          {nextTour.chainNote}
        </p>
      )}
      <TourTidyUpFailure failure={tidyUpFailure} />
    </CardShell>
  );
}
