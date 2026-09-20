/**
 * The tour catalog (#823) — the one list the welcome modal, Settings › Tutorials
 * and the end-of-tour card all read. Adding a tour means adding a definition and an
 * entry here; the engine never changes (spec #821, story 34).
 */

import type { TourDef } from "../tour";
import { FIRST_PIPELINE_TOUR } from "./firstPipeline";
import { FIRST_RUN_TOUR } from "./firstRun";

export { FIRST_PIPELINE_TOUR, TUTORIAL_PIPELINE_ID } from "./firstPipeline";
export {
  FIRST_RUN_TOUR,
  TUTORIAL_REPO_PATH,
  TUTORIAL_RUN_PIPELINE_ID,
} from "./firstRun";

export interface TourCatalogEntry {
  id: string;
  title: string;
  blurb: string;
  /** Rough duration in minutes; `null` while the tour does not exist yet. */
  minutes: number | null;
  /** The definition, or `null` for an announced-but-unbuilt tour (shown greyed). */
  def: TourDef | null;
}

/**
 * Order matters: it is the order of the **full tour**, and the order the welcome
 * modal and Settings list the tours in.
 *
 * *First run* leads (#824, design Q6). #823 had *First pipeline* first, on the
 * reasoning that it builds a pipeline *First run* could then launch — but the
 * training pipeline is prepared for you, so nothing depends on that order any
 * more, and *First run* is half as long and ends with something moving on screen.
 * A newcomer's first four minutes should produce a live Run, not a saved file.
 */
export const TOUR_CATALOG: TourCatalogEntry[] = [
  {
    id: FIRST_RUN_TOUR.id,
    title: FIRST_RUN_TOUR.title,
    blurb: FIRST_RUN_TOUR.blurb,
    minutes: FIRST_RUN_TOUR.minutes,
    def: FIRST_RUN_TOUR,
  },
  {
    id: FIRST_PIPELINE_TOUR.id,
    title: FIRST_PIPELINE_TOUR.title,
    blurb: FIRST_PIPELINE_TOUR.blurb,
    minutes: FIRST_PIPELINE_TOUR.minutes,
    def: FIRST_PIPELINE_TOUR,
  },
];

/** The tours that can actually be started today, in full-tour order. */
export function availableTours(): TourDef[] {
  return TOUR_CATALOG.map((entry) => entry.def).filter((def): def is TourDef => def != null);
}

export function findTour(id: string): TourDef | null {
  return TOUR_CATALOG.find((entry) => entry.id === id)?.def ?? null;
}

/**
 * What « Full tour » starts: every available tour, in catalog order — *First run*,
 * then *First pipeline* (#824). The end card of each offers the next.
 */
export function fullTourSequence(): TourDef[] {
  return availableTours();
}

/** Combined estimate for the full tour, counting only the tours that exist. */
export function fullTourMinutes(): number {
  return availableTours().reduce((total, tour) => total + tour.minutes, 0);
}
