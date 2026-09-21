import { BookOpen, Play, Workflow } from "lucide-react";
import { TOUR_CATALOG, fullTourMinutes } from "../../lib/tours";

/**
 * The **Modale de bienvenue** (#823, spec #821 stories 1–4).
 *
 * Shown once, on a browser that was never asked and an instance with no Run at
 * all — the decision itself lives in `lib/tourMemory.ts` so it can be tested
 * without a DOM. Three ways out, and **every one of them sets the key**: a tour
 * that has to be refused twice is a tour that harasses (story 3).
 *
 * No choice ever starts a tour on its own (ADR-0071 §2): the user picks, or leaves.
 */

interface Props {
  onStartFullTour: () => void;
  onStartTour: (tourId: string) => void;
  /** "Later" — remembers the answer and does nothing else. */
  onLater: () => void;
}

export default function WelcomeModal({ onStartFullTour, onStartTour, onLater }: Props) {
  const full = fullTourMinutes();

  return (
    <div
      className="fixed inset-0 z-[110] flex items-center justify-center bg-black/60"
      data-testid="tour-welcome-backdrop"
    >
      <div
        className="w-[420px] rounded-lg border border-line-strong bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        data-testid="tour-welcome"
      >
        <div className="flex items-center gap-2">
          <BookOpen size={17} className="shrink-0 text-acc" />
          <h2 className="font-semibold text-fg" style={{ fontSize: "14px" }}>
            Welcome to PDO
          </h2>
        </div>
        <p className="mt-2 text-fg-2" style={{ fontSize: "11.5px", lineHeight: 1.55 }}>
          Nothing has run here yet. A guided tour walks you through the real UI — it points,
          you click. You can leave at any time with{" "}
          <span className="rounded border border-line-strong bg-bg-3 px-1 font-mono" style={{ fontSize: "10px" }}>
            Esc
          </span>
          .
        </p>

        <button
          type="button"
          onClick={onStartFullTour}
          data-testid="tour-welcome-full"
          className="mt-3 flex w-full cursor-pointer items-center gap-2.5 rounded-md border border-acc-border bg-acc-bg px-3 py-2.5 text-left transition-colors hover:border-acc"
        >
          <Play size={15} className="shrink-0 text-acc" />
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
        </button>

        <div
          className="mt-3 font-semibold uppercase tracking-wide text-fg-4"
          style={{ fontSize: "9.5px", letterSpacing: "0.08em" }}
        >
          Or pick one
        </div>
        <div className="mt-1.5 flex flex-col gap-1.5">
          {TOUR_CATALOG.map((entry) => (
            <button
              key={entry.id}
              type="button"
              disabled={entry.def == null}
              data-testid={`tour-welcome-${entry.id}`}
              onClick={() => entry.def && onStartTour(entry.id)}
              className="flex items-center gap-2.5 rounded-md border border-line bg-bg-3 px-3 py-2 text-left transition-colors enabled:cursor-pointer enabled:hover:border-acc-border disabled:opacity-45"
            >
              <Workflow size={14} className="shrink-0 text-fg-3" />
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

        <div className="mt-3 text-center">
          <button
            type="button"
            onClick={onLater}
            data-testid="tour-welcome-later"
            className="cursor-pointer text-fg-3 transition-colors hover:text-fg"
            style={{ fontSize: "10.5px" }}
          >
            Later — you can start a tour any time from Settings › Tutorials
          </button>
        </div>
      </div>
    </div>
  );
}
