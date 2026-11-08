import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import Projecteur from "./Projecteur";
import { toCssRect } from "../../shared/ui/zoom";
import TourPopover, { TourAside } from "./TourPopover";
import { TourEndCard, TourFailedCard } from "./TourCards";
import { TourIntroCard } from "./TourIntroCard";
import WelcomeModal from "./WelcomeModal";
import { findTour, fullTourSequence } from "../../lib/tours";
import { markTourDone, markTourOffered } from "../../lib/tourMemory";
import type { TourController } from "../../hooks/tour/tourView";

/**
 * Everything a running tour puts on screen (#823), in one place so no existing
 * component ever learns that tours exist (acceptance criterion « Aucune logique de
 * tour dans les composants existants »).
 *
 * It owns three things the machine has no opinion about: the keyboard (Escape,
 * and ← / → for Previous / Next), the four-second notice after a quit, and the
 * welcome modal's three answers.
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

/**
 * Surfaces that own Escape **while the keystroke is theirs** (#825) — the
 * embedded tmux terminal. Escape in there is the agent's key (vim, the harness's
 * own menus), and a tour that swallowed it would make the step it just asked for
 * ("click in the terminal and paste the line") the step that throws it away.
 *
 * Checked on the event's target rather than on presence: the terminal is on
 * screen for four of the tour's steps, and Escape outside it must still quit.
 */
const ESC_TARGET_OWNERS = '[data-testid="tmux-terminal"]';

/**
 * Where ← / → belong to whatever has the focus, not to the tour (CONTEXT.md §
 * « Rejeu d'étape »): a field moves its caret, the terminal sends the key to the
 * agent, a node or an edge of the canvas is nudged by it. The same surfaces that
 * keep their Escape, plus the widgets whose arrows are their own navigation: a
 * radio group, a tab strip, a slider, a list's options, a menu's items, the Run
 * tree of the left panel.
 */
const ARROW_TARGET_OWNERS = [
  "input",
  "textarea",
  "select",
  '[contenteditable]:not([contenteditable="false"])',
  ESC_TARGET_OWNERS,
  // The whole canvas: its nodes and edges, its zoom controls and minimap. A
  // click on the empty pane takes no focus, so this costs the tour nothing.
  ".react-flow",
  // A Run row walks the Run tree with ← / →, but only claims the key when it
  // does something: on a row with nothing to open or close, the tour would
  // otherwise move under a reader who meant the list.
  "[data-run-row]",
  '[role="radio"]',
  '[role="tab"]',
  '[role="slider"]',
  '[role="listbox"]',
  '[role="option"]',
  '[role="menuitem"]',
  '[role="menuitemcheckbox"]',
  '[role="menuitemradio"]',
  '[role="treeitem"]',
].join(", ");

/**
 * Surfaces that own ← / → **while they are up**, wherever the focus is: an open
 * menu, dropdown or combobox list walks its options with them, and the artifact
 * viewer and the image lightbox page through files and iterations, on `window`,
 * so the tour could not tell their keystroke from its own. Open popups only: a
 * list that is always on screen (the Stats master list) keeps its arrows through
 * the focus, above, and leaves them to the tour otherwise.
 */
const ARROW_OWNERS =
  '[role="menu"], .d-pop, [data-slot="dropdown-menu-content"], .art-scrim, [data-testid="image-lightbox"]';

/**
 * What the arrows press right now, read from the latest view by the listener:
 * the button's own handler while it is shown and live, else `null`.
 */
interface ArrowControls {
  back: (() => void) | null;
  next: (() => void) | null;
}

/** ← / → for the tour, or `null` when the keystroke is somebody else's. */
function arrowOf(event: KeyboardEvent): "back" | "next" | null {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return null;
  if (event.defaultPrevented) return null;
  // Alt+← is the browser's Back, Shift+← extends a selection: not ours.
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return null;
  // A held key would fly through every step that waits for Next.
  if (event.repeat) return null;
  const target = event.target;
  if (target instanceof Element && target.closest(ARROW_TARGET_OWNERS)) return null;
  if (document.querySelector(ARROW_OWNERS)) return null;
  return event.key === "ArrowLeft" ? "back" : "next";
}

/**
 * How far along the **Full tour** the intermediate card sits (#825), or `null`
 * for a tour started on its own — there is no chain to report on.
 *
 * Derived from the catalog rather than carried in the controller: a chain is
 * always the full sequence (only « Full tour » builds one), so its position is a
 * lookup, and one less thing to keep in sync with the tour that is running.
 */
function chainProgress(tourId: string, nextTourId: string | null) {
  if (!nextTourId) return null;
  const sequence = fullTourSequence();
  const index = sequence.findIndex((entry) => entry.id === tourId);
  return index < 0 ? null : { done: index + 1, total: sequence.length };
}

export default function TourHost({ controller, showWelcome, onWelcomeAnswered, onOpenTutorials }: Props) {
  const { view, start, quit, next, back, skip, finish, finishHere, continueChain, begin, retryPrep } =
    controller;
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
      const target = event.target;
      if (target instanceof Element && target.closest(ESC_TARGET_OWNERS)) return;
      quitWithNotice();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [running, quitWithNotice]);

  // ← = Previous, → = Next, each only when its button is shown and live: a key
  // never does what the card does not offer. Same registration as Escape (bubble
  // phase, keyed on « running »), so the live state and the handlers are read
  // through a ref rather than by re-registering on every view.
  const arrows = useRef<ArrowControls>({ back: null, next: null });
  const pressBack = running && (view?.stepNumber ?? 0) > 1 && view?.canGoBack ? back : null;
  const pressNext = running && view?.awaitingConfirm && view?.ready ? next : null;
  // A layout effect, so a keystroke right after a render never reads the
  // buttons of the view before it.
  useLayoutEffect(() => {
    arrows.current = { back: pressBack, next: pressNext };
  }, [pressBack, pressNext]);
  useEffect(() => {
    if (!running) return;
    const onKey = (event: KeyboardEvent) => {
      const action = arrowOf(event);
      const press = action ? arrows.current[action] : null;
      if (!press) return;
      event.preventDefault();
      press();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [running]);

  return (
    <>
      {showWelcome && !view && (
        <WelcomeModal
          onStartFullTour={() => {
            markTourOffered();
            onWelcomeAnswered();
            const [first, ...rest] = fullTourSequence();
            if (first) start(first, rest);
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

      {/* #824 — the card a tour opens on, while its preparations run. Nothing is
          targeted yet, so the whole window dims. */}
      {view && view.run.phase === "intro" && view.prep && (
        <Projecteur hole={null}>
          <TourIntroCard
            tour={view.tour}
            prep={view.prep}
            onStart={begin}
            onRetry={retryPrep}
            // Leaving here is leaving before step 1: nothing to notice, and
            // whatever the preparations created is idempotent and stays.
            onClose={quit}
          />
        </Projecteur>
      )}

      {view && view.run.phase === "running" && view.step && (
        // The view measures the screen; the overlay is drawn in the app's CSS
        // pixels, under the UI scale (`shared/ui/zoom.ts`).
        <Projecteur
          hole={view.hole && toCssRect(view.hole)}
          zone={view.zone && toCssRect(view.zone)}
          wide={view.wideZone}
          lit={view.lit.map(toCssRect)}
        >
          {/* Side bubbles first, so the card paints over one that strays under it. */}
          {view.asides.map((aside) => (
            <TourAside key={aside.target} rect={toCssRect(aside.rect)} text={aside.text} />
          ))}
          <TourPopover
            tour={view.tour}
            step={view.step}
            stepNumber={view.stepNumber}
            total={view.total}
            hole={view.hole && toCssRect(view.hole)}
            body={view.body}
            note={view.note}
            ready={view.ready}
            awaitingConfirm={view.awaitingConfirm}
            checklist={view.checklist}
            canGoBack={view.canGoBack}
            onNext={next}
            onBack={back}
            onSkip={skip}
            onQuit={quitWithNotice}
          />
        </Projecteur>
      )}

      {view && view.run.phase === "finished" && (
        <Projecteur hole={null}>
          <TourEndCard
            tour={view.tour}
            app={view.run.observedApp}
            nextTour={view.nextTour}
            chainProgress={chainProgress(view.tour.id, view.nextTour?.id ?? null)}
            tidyUpFailure={view.tidyUpFailure}
            onFinish={finish}
            onFinishHere={finishHere}
            onStartTour={(tourId) => {
              // The checkmark belongs to the tour that was actually completed,
              // not to the one started next. Deliberately NOT `finish()`: that
              // also advances a pending Full-tour chain, so picking a tour from
              // the list would start two at once. Picking one by hand leaves the
              // sequence, which `start` does by taking no chain.
              markTourDone(view.tour.id);
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
            nextTour={view.nextTour}
            tidyUpFailure={view.tidyUpFailure}
            onClose={quit}
            onBackToTours={() => {
              quit();
              onOpenTutorials();
            }}
            onContinue={continueChain}
          />
        </Projecteur>
      )}

      {quitNotice && (
        <div
          data-testid="tour-quit-notice"
          className="fixed bottom-8 left-1/2 z-[110] -translate-x-1/2 rounded border border-line bg-surface-2 px-3 py-1.5 text-xs text-text-2 shadow-lg"
        >
          Tour closed · Settings › Tutorials
        </div>
      )}
    </>
  );
}
