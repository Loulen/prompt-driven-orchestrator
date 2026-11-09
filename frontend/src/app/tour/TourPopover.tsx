import { useLayoutEffect, useRef, useState } from "react";
import { Check, ChevronLeft, Circle, Copy, LoaderCircle, X } from "lucide-react";
import { placePopover } from "../../lib/tourPlacement";
import { Button, IconButton, StatusIcon, Tooltip } from "../../shared/ui/kit";
import type { TourRect } from "../../hooks/tour/tourView";
import type { TourChecklistItem, TourDef, TourStep } from "../../lib/tourDef";
import { copyBlockOf } from "../../lib/tour";
import type { TourIllustration } from "../../lib/tourPanels";
import { copyToClipboard } from "../../shared/lib/clipboard";
import { cssViewport } from "../../shared/ui/zoom";
import { withCode } from "./tourCardText";

/**
 * The instruction card of a running tour (#823).
 *
 * It is the only part of the Projecteur layer that takes clicks: Quit must be
 * reachable at every instant (story 11), and `Next` / `Skip` live here too. It
 * anchors to the hole and flips side to stay on screen — see `lib/tourPlacement.ts`
 * for the rules; this component only measures itself and applies the result.
 *
 * Layout, after the design review of #825: the header carries the tour name, the
 * progress and the way out (`✕`, an icon with its tooltip — *reachable*, not
 * prominent); the footer carries `Skip` on the left and, on the right, either
 * `Next` or the one grey line that says what the tour is waiting for.
 *
 * From step 2 on, `‹ Previous` opens the left side of the footer, before `Skip`.
 * It **replays** the step before (CONTEXT.md § « Rejeu d'étape »), and is greyed
 * out with a tooltip saying why when that step's screen is gone.
 */

interface Props {
  tour: TourDef;
  step: TourStep;
  stepNumber: number;
  total: number;
  hole: TourRect | null;
  /** The step's body, already resolved against the observation (#825). */
  body: string;
  /** The quieter second paragraph, under the body and the checklist. */
  note: string | null;
  /** The step's condition holds — `Next` is live. */
  ready: boolean;
  /** This step waits for `Next` rather than advancing on its own. */
  awaitingConfirm: boolean;
  /** Live sub-conditions of a step that asks for more than one gesture (#824). */
  checklist: TourChecklistItem[];
  /** `Previous` would replay the step before. Ignored on step 1. */
  canGoBack?: boolean;
  onNext: () => void;
  /** `Previous`. The button is only rendered from step 2 on, and only with a handler. */
  onBack?: () => void;
  onSkip: () => void;
  onQuit: () => void;
}

const WIDTH = 266;

const PREVIOUS_GONE = "This step's screen is gone: it can't be replayed";

export default function TourPopover({
  tour,
  step,
  stepNumber,
  total,
  hole,
  body,
  note,
  ready,
  awaitingConfirm,
  checklist,
  canGoBack = false,
  onNext,
  onBack,
  onSkip,
  onQuit,
}: Props) {
  const cardRef = useRef<HTMLDivElement>(null);
  const [placement, setPlacement] = useState(() =>
    placePopover(hole, { width: WIDTH, height: 150 }, { width: 1280, height: 800 }),
  );
  // Which step's block was copied, not "was something copied": keyed on the step,
  // the badge resets by itself when the tour moves on — no effect to reconcile it.
  const [copiedStep, setCopiedStep] = useState<string | null>(null);
  const copied = copiedStep === step.id;
  const copyBlock = copyBlockOf(step);
  // The spinner row belongs to the wait, not to the step: once the condition is
  // met there is nothing left to wait for, and a spinner that kept turning under
  // a satisfied checklist would be the card contradicting itself.
  const waiting = step.waitingNote && !ready;
  // Skip is for a gesture not yet made: once the step's action is done, only Next
  // is left. A reading step (no `done`) has no gesture, so Skip stays beside Next.
  const skippable = step.skippable && !(step.done && ready);
  // No Previous on step 1: nothing before it, and a Full tour's legs are
  // independent, so there is no going back into the previous tour.
  const showBack = stepNumber > 1 && onBack !== undefined;

  // Measure after paint and re-place: the card's height depends on its body and
  // on whether it carries a copy block, so a fixed estimate would flip it on the
  // wrong side near a window edge.
  useLayoutEffect(() => {
    const height = cardRef.current?.offsetHeight ?? 150;
    setPlacement(
      placePopover(hole, { width: WIDTH, height }, cssViewport()),
    );
  }, [hole, step.id, body, note, checklist]);

  const copy = () => {
    if (!copyBlock) return;
    void copyToClipboard(copyBlock);
    setCopiedStep(step.id);
  };

  return (
    <div
      ref={cardRef}
      data-testid="tour-popover"
      data-step={step.id}
      data-side={placement.side}
      // A group, not a dialog: a dialog on screen owns Escape, and Escape here quits the tour.
      role="group"
      aria-label={`${tour.title} · step ${stepNumber} of ${total}`}
      className="tr-frame tr-card pointer-events-auto"
      style={{ top: placement.top, left: placement.left, width: WIDTH }}
    >
      <span aria-hidden="true" className="tr-caret" />
      <div className="tr-head">
        <span className="d-caps tr-name">{tour.title}</span>
        <span className="d-meta tnum" data-testid="tour-progress">
          {stepNumber}/{total}
        </span>
        <span className="d-grow" />
        {/* Leaving a tutorial must always be one click away, and never look
            like the thing to do (design review of #825). */}
        <IconButton icon={X} label="Quit tour" kbd="Esc" size="sm" className="tr-quit" onClick={onQuit} data-testid="tour-quit" />
      </div>

      <div className="tr-title">{step.title}</div>
      {body && (
        <p className="tr-body" data-testid="tour-body">
          {withCode(body)}
        </p>
      )}

      {step.illustration && <Illustration illustration={step.illustration} />}

      {/* A step that asks for two gestures says which one is still missing. The
          honest answer to "I ticked one and nothing happened": something did. */}
      {checklist.length > 0 && (
        <ul className="tr-check" data-testid="tour-checklist">
          {checklist.map((item) => (
            <li key={item.label} data-testid={`tour-checklist-${item.label}`} data-done={item.done ? "true" : "false"}>
              {item.done ? <Check size={11} strokeWidth={2} aria-hidden /> : <Circle size={11} strokeWidth={1.5} aria-hidden />}
              <span className="tr-check__text">
                <span className="tr-check__label">{item.label}</span>
                {item.badge && (
                  <span data-testid="tour-checklist-badge" className="tr-check__badge">
                    {item.badge}
                  </span>
                )}
                {/* The engine has no « back »: an item left open says, in place,
                    what to do about it — the control is inside the soft zone. */}
                {item.note && (
                  <span data-testid="tour-checklist-note" className="tr-check__note">
                    {item.note}
                  </span>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}

      {note && (
        <p className="tr-note" data-testid="tour-note">
          {note}
        </p>
      )}

      {copyBlock && (
        <div className="tr-copy">
          <pre data-testid="tour-copy-block">{copyBlock}</pre>
          <IconButton
            icon={copied ? Check : Copy}
            label={copied ? "Copied" : "Copy the block"}
            size="sm"
            onClick={copy}
            data-testid="tour-copy"
          />
        </div>
      )}

      {/* The one step with no time limit has to say so (#825): an unbounded wait
          with nothing turning reads as a freeze. */}
      {waiting && (
        <div className="tr-wait" data-testid="tour-waiting">
          <LoaderCircle size={11} strokeWidth={1.5} className="d-spin" aria-hidden />
          {step.waitingNote}
        </div>
      )}

      {/* Progress bar — the same count as the label, for the glance rather than the read. */}
      <span className="tr-prog" aria-hidden="true">
        <span style={{ width: `${(stepNumber / total) * 100}%` }} />
      </span>

      {/* Not rendered at all when the step has neither: a step that is simply
          waiting for the right click says so in its title, and an empty row
          under the progress bar would be a control that is not there. */}
      {(showBack || skippable || awaitingConfirm || step.advanceHint) && (
        // A press here leaves the focus where it is (FP of CORE-227): moving it
        // blurred the field just pasted into, which re-rendered under the press —
        // several fields commit on blur — and the card moved with its hole
        // before the release, so the first Next landed nowhere. The buttons act
        // on the click; the keyboard reaches them as before.
        <div className="tr-cfoot" onMouseDown={(e) => e.preventDefault()}>
          {showBack && <PreviousButton enabled={canGoBack} onBack={onBack} />}
          {skippable ? (
            <Button tone="ghost" size="sm" onClick={onSkip} data-testid="tour-skip">
              Skip
            </Button>
          ) : (
            !showBack && <span />
          )}
          <span className="d-grow" />
          {awaitingConfirm ? (
            <Button tone="primary" size="sm" onClick={onNext} disabled={!ready} data-testid="tour-next" title="Next (→)">
              Next
            </Button>
          ) : (
            step.advanceHint && (
              <span className="tr-hint" data-testid="tour-advance-hint">
                {step.advanceHint}
              </span>
            )
          )}
        </div>
      )}
    </div>
  );
}

/**
 * `‹ Previous`: ghost, like `Skip`, so it stays secondary to `Next`.
 *
 * Not replayable → greyed the way the kit's `disabledReason` greys a control
 * (`aria-disabled`, still focusable, a click that does nothing, the reason in
 * its tooltip), **not** `disabled`: a disabled button gets neither hover nor
 * focus, so the tooltip explaining why would never open. Written out rather
 * than passed as `disabledReason` so the element keeps one shape: the button
 * flips state as the reader works, and a shape change would remount it under a
 * keyboard user's focus.
 */
function PreviousButton({ enabled, onBack }: { enabled: boolean; onBack?: () => void }) {
  return (
    <Tooltip label={enabled ? "Previous step" : PREVIOUS_GONE} kbd={enabled ? "←" : undefined}>
      <Button
        tone="ghost"
        size="sm"
        icon={ChevronLeft}
        onClick={enabled ? onBack : undefined}
        aria-disabled={enabled ? undefined : true}
        data-testid="tour-back"
      >
        Previous
      </Button>
    </Tooltip>
  );
}

/**
 * A step's picture (#911): one row per status, each led by the Status icon the
 * real list draws, and named by its shape. Framed like a small panel so it reads
 * as « what it looks like », not as rows of the real list.
 */
function Illustration({ illustration }: { illustration: TourIllustration }) {
  return (
    <ul className="tr-illus" data-testid="tour-illustration" aria-label="Example">
      {illustration.rows.map((row) => (
        <li key={row.status} data-testid="tour-illustration-row" data-status={row.status}>
          <StatusIcon kind="run" status={row.status} size="sm" />
          <span className="min-w-0 flex-1 truncate text-text-2">{row.label}</span>
        </li>
      ))}
    </ul>
  );
}

const ASIDE_WIDTH = 190;

/**
 * A side bubble (#911): a short note pinned beside part of the panel a gesture
 * opened. Quieter than the card — no header, no buttons, not part of the flow —
 * and transparent to the pointer, so it never stands between the reader and the
 * panel they are exploring.
 */
export function TourAside({ rect, text }: { rect: TourRect; text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [placement, setPlacement] = useState(() =>
    placePopover(rect, { width: ASIDE_WIDTH, height: 60 }, { width: 1280, height: 800 }),
  );
  useLayoutEffect(() => {
    const height = ref.current?.offsetHeight ?? 60;
    setPlacement(
      placePopover(rect, { width: ASIDE_WIDTH, height }, cssViewport()),
    );
  }, [rect, text]);

  return (
    <div
      ref={ref}
      data-testid="tour-aside"
      data-side={placement.side}
      role="note"
      className="tr-aside pointer-events-none"
      style={{ top: placement.top, left: placement.left, width: ASIDE_WIDTH }}
    >
      <span aria-hidden="true" className="tr-caret" />
      {text}
    </div>
  );
}
