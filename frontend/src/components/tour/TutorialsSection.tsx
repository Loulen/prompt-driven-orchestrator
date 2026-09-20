import { useState } from "react";
import { CircleCheck, Circle, Play } from "lucide-react";
import { TOUR_CATALOG, fullTourMinutes, fullTourSequence } from "../../lib/tours";
import { loadToursDone, resetTourMemory } from "../../lib/tourMemory";

/**
 * Settings › General › **Tutorials** (#823, spec #821 stories 5–6).
 *
 * Presentation only: nothing here belongs to the instance form, so no field of it
 * ever enters the dirty set and Save does not apply to it — the checkmarks and the
 * welcome prompt are this browser's memory, like the theme. Starting a tour closes
 * the Settings surface first, because every tour points at the app underneath it.
 */

interface Props {
  /** Close Settings, then run this tour. */
  onStartTour: (tourId: string) => void;
  /** Close Settings, then run the full tour. */
  onStartFullTour: () => void;
}

function shortDate(iso: string): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return "";
  return at.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export default function TutorialsSection({ onStartTour, onStartFullTour }: Props) {
  const [done, setDone] = useState(loadToursDone);
  const full = fullTourMinutes();
  const hasStartableTour = fullTourSequence().length > 0;

  return (
    <div className="flex flex-col gap-3" data-testid="settings-tutorials">
      <div className="flex items-center gap-2 rounded-md border border-acc-border bg-acc-bg px-3 py-2.5">
        <span className="min-w-0 flex-1">
          <span className="block font-semibold text-fg" style={{ fontSize: "12px" }}>
            Full tour
          </span>
          <span className="block text-fg-3" style={{ fontSize: "10.5px" }}>
            Every tour PDO has, back to back.
          </span>
        </span>
        <span className="shrink-0 font-mono text-fg-3" style={{ fontSize: "10px" }}>
          ~{full} min
        </span>
        <button
          type="button"
          onClick={onStartFullTour}
          disabled={!hasStartableTour}
          data-testid="tutorials-start-full"
          className="flex shrink-0 items-center gap-1 rounded bg-acc px-2.5 py-1 font-medium text-on-acc transition-colors enabled:cursor-pointer enabled:hover:bg-acc-dim disabled:opacity-40"
          style={{ fontSize: "11px" }}
        >
          <Play size={11} />
          Start
        </button>
      </div>

      {TOUR_CATALOG.map((entry) => {
        const finishedAt = done[entry.id];
        return (
          <div
            key={entry.id}
            data-testid={`tutorials-row-${entry.id}`}
            data-done={finishedAt ? "true" : "false"}
            className={`flex items-center gap-2.5 ${entry.def ? "" : "opacity-45"}`}
          >
            {finishedAt ? (
              <CircleCheck
                size={15}
                className="shrink-0 text-acc"
                aria-label={`${entry.title} completed`}
                data-testid={`tutorials-done-${entry.id}`}
              />
            ) : (
              <Circle size={15} className="shrink-0 text-fg-5" aria-hidden="true" />
            )}
            <span className="min-w-0 flex-1">
              <span className="block font-medium text-fg" style={{ fontSize: "11.5px" }}>
                {entry.title}
              </span>
              <span className="block text-fg-3" style={{ fontSize: "10.5px" }}>
                {entry.blurb}
                {entry.def ? ` ${entry.def.steps.length} steps.` : ""}
              </span>
            </span>
            {finishedAt && (
              <span className="shrink-0 font-mono text-acc" style={{ fontSize: "9.5px" }}>
                done · {shortDate(finishedAt)}
              </span>
            )}
            <span className="shrink-0 font-mono text-fg-4" style={{ fontSize: "9.5px" }}>
              {entry.def ? `~${entry.minutes} min` : "soon"}
            </span>
            <button
              type="button"
              disabled={entry.def == null}
              onClick={() => onStartTour(entry.id)}
              data-testid={`tutorials-start-${entry.id}`}
              className="shrink-0 rounded border border-line-strong bg-bg-3 px-2 py-0.5 font-medium text-fg-2 transition-colors enabled:cursor-pointer enabled:hover:border-acc enabled:hover:text-fg disabled:opacity-40"
              style={{ fontSize: "10.5px" }}
            >
              {finishedAt ? "Replay" : "Start"}
            </button>
          </div>
        );
      })}

      <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
        Checkmarks and the welcome prompt are stored in this browser.{" "}
        <button
          type="button"
          data-testid="tutorials-reset"
          onClick={() => {
            resetTourMemory();
            setDone({});
          }}
          className="cursor-pointer text-fg-3 underline underline-offset-2 transition-colors hover:text-fg"
        >
          Reset tutorial memory
        </button>
      </div>
    </div>
  );
}
