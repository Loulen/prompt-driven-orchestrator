import { useEffect } from "react";
import { Compass, Play, Rocket, Workflow, type LucideIcon } from "lucide-react";
import { Button, Kbd } from "../../shared/ui/kit";
import { BrandLockup } from "../../shared/ui/brand/BrandLockup";
import { BySignature } from "../../shared/ui/brand/BySignature";
import { currentBrand } from "../../shared/ui/brand/brand";
import { TOUR_CATALOG, fullTourMinutes } from "../../lib/tours";

/**
 * The **Modale de bienvenue** (#823, spec #821 stories 1–4).
 *
 * Shown once, on a browser that was never asked and an instance with no Run at
 * all — the decision itself lives in `lib/tourMemory.ts` so it can be tested
 * without a DOM. Three ways out, and **every one of them sets the key**: a tour
 * that has to be refused twice is a tour that harasses (story 3). Escape is Later.
 *
 * No choice ever starts a tour on its own (ADR-0071 §2): the user picks, or leaves.
 *
 * On top, the lockup of the Marque (CORE-225); under PDO's, « by Acogni » (under
 * Acogni Driver's the wordmark already says it).
 */

interface Props {
  onStartFullTour: () => void;
  onStartTour: (tourId: string) => void;
  /** "Later" — remembers the answer and does nothing else. */
  onLater: () => void;
}

const ICON: Record<string, LucideIcon> = { overview: Compass, "first-run": Rocket, "first-pipeline": Workflow };

export default function WelcomeModal({ onStartFullTour, onStartTour, onLater }: Props) {
  const full = fullTourMinutes();
  const available = TOUR_CATALOG.filter((entry) => entry.def != null).length;
  const brand = currentBrand();

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onLater();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onLater]);

  return (
    <div className="tr-scrim z-[110]" data-testid="tour-welcome-backdrop">
      <div className="tr-frame tr-welcome" role="dialog" aria-label={`Welcome to ${brand.name}`} data-testid="tour-welcome">
        <div className="tr-welcome__brand" data-brand={brand.id}>
          <BrandLockup />
          {brand.id === "pdo" && <BySignature />}
        </div>
        <div className="tr-welcome__body">
          <button
            type="button"
            onClick={onStartFullTour}
            data-testid="tour-welcome-full"
            className="tr-full"
            autoFocus
          >
            <Play size={14} strokeWidth={1.5} aria-hidden />
            <span className="tr-full__t">Full tour</span>
            <span className="d-meta">{available} tours</span>
            <span className="d-grow" />
            <span className="tr-min">~{full} min</span>
          </button>
          <div className="d-caps tr-cap">Or pick one</div>
          {TOUR_CATALOG.map((entry) => {
            const Icon = ICON[entry.id] ?? Workflow;
            return (
              <button
                key={entry.id}
                type="button"
                disabled={entry.def == null}
                data-testid={`tour-welcome-${entry.id}`}
                onClick={() => entry.def && onStartTour(entry.id)}
                className="tr-row"
                title={entry.blurb}
              >
                <Icon size={14} strokeWidth={1.5} className="tr-row__ic" aria-hidden />
                <span className="tr-row__text">
                  <span className="tr-row__t">{entry.title}</span>
                  <span className="tr-row__x">{entry.blurb}</span>
                </span>
                <span className="tr-min">{entry.def ? `~${entry.minutes} min` : "soon"}</span>
              </button>
            );
          })}
        </div>
        <div className="tr-foot">
          <span className="d-meta">
            Quit any tour <Kbd>Esc</Kbd>
          </span>
          <span className="d-grow" />
          <Button tone="ghost" onClick={onLater} data-testid="tour-welcome-later" title="Tours stay in Settings › Tutorials">
            Later
          </Button>
        </div>
      </div>
    </div>
  );
}
