import { useCallback, useEffect, useState } from "react";
import Projecteur from "./Projecteur";
import TourPopover from "./TourPopover";
import { TourEndCard, TourFailedCard } from "./TourCards";
import WelcomeModal from "./WelcomeModal";
import { findTour, fullTourSequence } from "../../lib/tours";
import { markTourOffered } from "../../lib/tourMemory";
import type { TourController } from "../../hooks/useTour";

/**
 * Everything a running tour puts on screen (#823), in one place so no existing
 * component ever learns that tours exist (acceptance criterion « Aucune logique de
 * tour dans les composants existants »).
 *
 * It owns three things the machine has no opinion about: the Escape key, the
 * four-second notice after a quit, and the welcome modal's three answers.
 */

interface Props {
  controller: TourController;
  /** The welcome rule already said yes (`lib/tourMemory.ts`). */
  showWelcome: boolean;
  /** Called by every exit of the welcome modal — all of them set the key. */
  onWelcomeAnswered: () => void;
  /** « Back to tours » on the failure card opens Settings › General › Tutorials. */
  onOpenTutorials: () => void;
}

/**
 * Surfaces that own Escape while they are up. The tour yields to them (design Q2:
 * "menu-first Esc") — closing the menu you opened by mistake must not also throw
 * the tour away.
 */
const ESC_OWNERS =
  '[role="menu"], [role="listbox"], [role="dialog"], [data-slot="dropdown-menu-content"], [data-testid="new-pipeline-dialog"]';

export default function TourHost({ controller, showWelcome, onWelcomeAnswered, onOpenTutorials }: Props) {
  const { view, start, quit, next, skip, finish } = controller;
  const [quitNotice, setQuitNotice] = useState(false);

  const startById = useCallback(
    (tourId: string) => {
      const tour = findTour(tourId);
      if (tour) start(tour);
    },
    [start],
  );

  const quitWithNotice = useCallback(() => {
    quit();
    setQuitNotice(true);
  }, [quit]);

  useEffect(() => {
    if (!quitNotice) return;
    const timer = window.setTimeout(() => setQuitNotice(false), 4_000);
    return () => window.clearTimeout(timer);
  }, [quitNotice]);

  // Escape quits — immediately and without a confirmation (design Q3): a tour is
  // not a transaction, and asking "are you sure?" to leave a tutorial is a tax on
  // the person already telling you they want out. Registered in the BUBBLE phase so
  // a surface that stops propagation in its own handler keeps its Escape.
  // Keyed on « is a tour running », not on the view: the view changes with the
  // hole, ten times a second, and re-registering a window listener that often
  // would be a real cost for a handler that only reads one key.
  const running = view?.run.phase === "running";
  useEffect(() => {
    if (!running) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (document.querySelector(ESC_OWNERS)) return;
      quitWithNotice();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [running, quitWithNotice]);

  return (
    <>
      {showWelcome && !view && (
        <WelcomeModal
          onStartFullTour={() => {
            markTourOffered();
            onWelcomeAnswered();
            const [first] = fullTourSequence();
            if (first) start(first);
          }}
          onStartTour={(tourId) => {
            markTourOffered();
            onWelcomeAnswered();
            startById(tourId);
          }}
          onLater={() => {
            markTourOffered();
            onWelcomeAnswered();
          }}
        />
      )}

      {view && view.run.phase === "running" && view.step && (
        <Projecteur hole={view.hole} zone={view.zone}>
          <TourPopover
            tour={view.tour}
            step={view.step}
            stepNumber={view.stepNumber}
            total={view.total}
            hole={view.hole}
            ready={view.ready}
            awaitingConfirm={view.awaitingConfirm}
            onNext={next}
            onSkip={skip}
            onQuit={quitWithNotice}
          />
        </Projecteur>
      )}

      {view && view.run.phase === "finished" && (
        <Projecteur hole={null}>
          <TourEndCard
            tour={view.tour}
            onFinish={finish}
            onStartTour={(tourId) => {
              // Finishing this one before chaining: the checkmark belongs to the
              // tour that was actually completed, not to the one started next.
              finish();
              startById(tourId);
            }}
          />
        </Projecteur>
      )}

      {view && view.run.phase === "failed" && view.run.failure && (
        <Projecteur hole={null}>
          <TourFailedCard
            tour={view.tour}
            failure={view.run.failure}
            onClose={quit}
            onBackToTours={() => {
              quit();
              onOpenTutorials();
            }}
          />
        </Projecteur>
      )}

      {quitNotice && (
        <div
          data-testid="tour-quit-notice"
          className="fixed bottom-8 left-1/2 z-[110] -translate-x-1/2 rounded border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 shadow-lg"
          style={{ fontSize: "11px" }}
        >
          Tour closed. Start it again from Settings › Tutorials.
        </div>
      )}
    </>
  );
}
