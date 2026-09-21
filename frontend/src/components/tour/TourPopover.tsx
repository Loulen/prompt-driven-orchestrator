import { useLayoutEffect, useRef, useState } from "react";
import { Check, Circle, Copy, LoaderCircle, X } from "lucide-react";
import { placePopover, type PopoverSide } from "../../lib/tourPlacement";
import type { TourRect } from "../../hooks/useTour";
import type { TourChecklistItem, TourDef, TourStep } from "../../lib/tour";

/**
 * The instruction card of a running tour (#823).
 *
 * It is the only part of the Projecteur layer that takes clicks: Quit must be
 * reachable at every instant (story 11), and `Next` / `Skip` live here too. It
 * anchors to the hole and flips side to stay on screen — see `lib/tourPlacement.ts`
 * for the rules; this component only measures itself and applies the result.
 *
 * Layout, after the design review of #825: the header carries the tour name, the
 * progress and the way out (`✕ quit`, a small grey label — *reachable*, not
 * prominent); the footer carries `Skip` on the left and, on the right, either
 * `Next` or the one grey line that says what the tour is waiting for.
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
  onNext: () => void;
  onSkip: () => void;
  onQuit: () => void;
}

const WIDTH = 266;

/** The little triangle pointing back at the hole. */
function arrowClass(side: PopoverSide): string {
  switch (side) {
    case "right":
      return "left-[-5px] top-1/2 -translate-y-1/2 border-b border-l";
    case "left":
      return "right-[-5px] top-1/2 -translate-y-1/2 border-t border-r";
    case "bottom":
      return "top-[-5px] left-1/2 -translate-x-1/2 border-t border-l";
    case "top":
      return "bottom-[-5px] left-1/2 -translate-x-1/2 border-b border-r";
  }
}

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
  onNext,
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
  // The spinner row belongs to the wait, not to the step: once the condition is
  // met there is nothing left to wait for, and a spinner that kept turning under
  // a satisfied checklist would be the card contradicting itself.
  const waiting = step.waitingNote && !ready;

  // Measure after paint and re-place: the card's height depends on its body and
  // on whether it carries a copy block, so a fixed estimate would flip it on the
  // wrong side near a window edge.
  useLayoutEffect(() => {
    const height = cardRef.current?.offsetHeight ?? 150;
    setPlacement(
      placePopover(
        hole,
        { width: WIDTH, height },
        { width: window.innerWidth, height: window.innerHeight },
      ),
    );
  }, [hole, step.id, body, note, checklist]);

  const copy = () => {
    if (!step.copyBlock) return;
    void navigator.clipboard?.writeText(step.copyBlock).catch(() => {});
    setCopiedStep(step.id);
  };

  return (
    <div
      ref={cardRef}
      data-testid="tour-popover"
      data-step={step.id}
      data-side={placement.side}
      className="pointer-events-auto absolute flex flex-col gap-2 rounded-lg border border-line-strong bg-bg-2 p-3 shadow-lg"
      style={{ top: placement.top, left: placement.left, width: WIDTH, fontSize: "11.5px" }}
    >
      <span
        aria-hidden="true"
        className={`absolute h-2.5 w-2.5 rotate-45 border-line-strong bg-bg-2 ${arrowClass(placement.side)}`}
      />
      <div className="flex items-center justify-between gap-2">
        <span
          className="font-semibold uppercase tracking-wide text-acc"
          style={{ fontSize: "9.5px", letterSpacing: "0.08em" }}
          data-testid="tour-progress"
        >
          {tour.title} · {stepNumber} / {total}
        </span>
        {/* Small and grey on purpose (design review of #825): leaving a tutorial
            must always be one click away, and never look like the thing to do. */}
        <button
          type="button"
          onClick={onQuit}
          data-testid="tour-quit"
          title="Quit tour (Esc)"
          className="flex shrink-0 cursor-pointer items-center gap-0.5 text-fg-4 transition-colors hover:text-fg-2"
          style={{ fontSize: "9.5px" }}
        >
          <X size={9} />
          quit
        </button>
      </div>

      <div className="font-semibold text-fg" style={{ fontSize: "13px" }}>
        {step.title}
      </div>
      {body && (
        <p className="text-fg-2" style={{ fontSize: "11px", lineHeight: 1.5 }} data-testid="tour-body">
          {body}
        </p>
      )}

      {/* A step that asks for two gestures says which one is still missing. The
          honest answer to "I ticked one and nothing happened": something did. */}
      {checklist.length > 0 && (
        <ul className="flex flex-col gap-1" data-testid="tour-checklist">
          {checklist.map((item) => (
            <li
              key={item.label}
              data-testid={`tour-checklist-${item.label}`}
              data-done={item.done ? "true" : "false"}
              className={`flex items-start gap-1.5 ${item.done ? "text-acc" : "text-fg-4"}`}
              style={{ fontSize: "10.5px" }}
            >
              {item.done ? (
                <Check size={11} className="mt-px shrink-0" />
              ) : (
                <Circle size={11} className="mt-px shrink-0" />
              )}
              <span className="flex min-w-0 flex-1 flex-wrap items-center gap-1">
                <span className="truncate font-mono">{item.label}</span>
                {item.badge && (
                  <span
                    data-testid="tour-checklist-badge"
                    className="rounded bg-st-failed-bg px-1 font-mono text-st-failed"
                    style={{ fontSize: "9px" }}
                  >
                    {item.badge}
                  </span>
                )}
                {/* The engine has no « back »: an item left open says, in place,
                    what to do about it — the control is inside the soft zone. */}
                {item.note && (
                  <span data-testid="tour-checklist-note" className="text-st-await">
                    — {item.note}
                  </span>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}

      {note && (
        <p className="text-fg-3" style={{ fontSize: "10.5px", lineHeight: 1.5 }} data-testid="tour-note">
          {note}
        </p>
      )}

      {step.copyBlock && (
        <div className="flex items-start gap-1.5 rounded border border-line bg-bg-3 p-1.5">
          <pre
            className="min-w-0 flex-1 whitespace-pre-wrap break-words font-mono text-fg-2"
            style={{ fontSize: "10px", lineHeight: 1.5 }}
            data-testid="tour-copy-block"
          >
            {step.copyBlock}
          </pre>
          <button
            type="button"
            onClick={copy}
            data-testid="tour-copy"
            aria-label="Copy the block"
            className="flex shrink-0 cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-4 px-1.5 py-0.5 text-fg-3 transition-colors hover:text-fg"
            style={{ fontSize: "10px" }}
          >
            {copied ? <Check size={11} className="text-acc" /> : <Copy size={11} />}
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
      )}

      {/* The one step with no time limit has to say so (#825): an unbounded wait
          with nothing turning reads as a freeze. */}
      {waiting && (
        <div
          className="flex items-center gap-2 rounded border border-line bg-bg-3 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
          data-testid="tour-waiting"
        >
          <LoaderCircle size={11} className="shrink-0 animate-spin text-acc" />
          {step.waitingNote}
        </div>
      )}

      {/* Progress bar — the same count as the label, for the glance rather than the read. */}
      <div className="h-0.5 w-full overflow-hidden rounded-full bg-bg-4">
        <div className="h-full bg-acc" style={{ width: `${(stepNumber / total) * 100}%` }} />
      </div>

      {/* Not rendered at all when the step has neither: a step that is simply
          waiting for the right click says so in its title, and an empty row
          under the progress bar would be a control that is not there. */}
      {(step.skippable || awaitingConfirm || step.advanceHint) && (
        <div className="flex items-center justify-between gap-2">
          {step.skippable ? (
            <button
              type="button"
              onClick={onSkip}
              data-testid="tour-skip"
              className="cursor-pointer text-fg-3 transition-colors hover:text-fg"
              style={{ fontSize: "10.5px" }}
            >
              Skip
            </button>
          ) : (
            <span />
          )}
          {awaitingConfirm ? (
            <button
              type="button"
              onClick={onNext}
              disabled={!ready}
              data-testid="tour-next"
              className="cursor-pointer rounded bg-acc px-2 py-0.5 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:cursor-not-allowed disabled:opacity-40"
              style={{ fontSize: "10.5px" }}
            >
              Next
            </button>
          ) : (
            step.advanceHint && (
              <span
                className="text-right font-mono text-fg-4"
                style={{ fontSize: "9.5px" }}
                data-testid="tour-advance-hint"
              >
                {step.advanceHint}
              </span>
            )
          )}
        </div>
      )}
    </div>
  );
}
