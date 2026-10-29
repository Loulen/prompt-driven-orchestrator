import { CircleCheck, LoaderCircle, Play, TriangleAlert } from "lucide-react";
import type { TourDef } from "../../lib/tourDef";
import type { TourPrepView } from "../../hooks/tour/tourView";
import { Button } from "../../shared/ui/kit";
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
  const failure = prep.failure;

  const footer = (
    <>
      <span className="d-meta" title={intro.footnote} data-testid="tour-intro-footnote">
        Training data only
      </span>
      <span className="d-grow" />
      {failure ? (
        <>
          <Button onClick={onClose} data-testid="tour-intro-close">
            Close
          </Button>
          <Button tone="primary" onClick={onRetry} data-testid="tour-intro-retry">
            Retry
          </Button>
        </>
      ) : (
        <>
          <Button onClick={onClose} data-testid="tour-intro-later">
            Later
          </Button>
          <Button tone="primary" onClick={onStart} disabled={!prep.ready} data-testid="tour-intro-start">
            Start
          </Button>
        </>
      )}
    </>
  );

  return (
    <CardShell testId="tour-intro-card" label={failure ? failure.title : intro.title} footer={footer}>
      <div className="tr-head">
        {failure ? (
          <TriangleAlert size={13} strokeWidth={1.5} className="text-st-await" aria-hidden />
        ) : (
          <Play size={13} strokeWidth={1.5} className="text-primary" aria-hidden />
        )}
        <span className={`d-caps tr-name ${failure ? "tr-name--warn" : ""}`}>{tour.title}</span>
        <span className="d-meta tnum font-mono">
          {failure ? "could not prepare" : `~${tour.minutes} min · ${tour.steps.length} steps`}
        </span>
      </div>
      <h2 className="tr-bcard__title" title={failure ? undefined : intro.body}>
        {failure ? failure.title : intro.title}
      </h2>
      {failure ? (
        <pre data-testid="tour-intro-reason" className="tr-reason">
          {failure.reason}
        </pre>
      ) : (
        <ul className="tr-prep" data-testid="tour-intro-checklist">
          {prep.items.map((item) => (
            <li key={item.id} data-testid={`tour-intro-prep-${item.id}`} data-done={item.done ? "true" : "false"}>
              {item.done ? (
                <CircleCheck size={12} strokeWidth={1.5} aria-hidden />
              ) : (
                <LoaderCircle size={12} strokeWidth={1.5} className="d-spin" aria-hidden />
              )}
              <span className="min-w-0 flex-1">{withCode(item.label)}</span>
            </li>
          ))}
        </ul>
      )}
    </CardShell>
  );
}
