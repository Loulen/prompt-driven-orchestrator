import { CircleCheck, Loader, TriangleAlert } from "lucide-react";
import type { TourDef } from "../../lib/tourDef";
import type { TourPrepView } from "../../hooks/tour/tourView";
import { CardShell } from "./TourCardShell";
import { withCode } from "./tourCardText";

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
