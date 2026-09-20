/**
 * The tour catalog (#823) — the one list the welcome modal, Settings › Tutorials
 * and the end-of-tour card all read. Adding a tour means adding a definition and an
 * entry here; the engine never changes (spec #821, story 34).
 */

import type { TourDef } from "../tour";
import { FIRST_PIPELINE_TOUR } from "./firstPipeline";

export { FIRST_PIPELINE_TOUR, TUTORIAL_PIPELINE_ID } from "./firstPipeline";

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
 * modal and Settings list the tours in. *First pipeline* comes first because it
 * builds the thing *First run* then launches.
 *
 * *First run* is declared here without a definition on purpose: #824 builds it, and
 * a placeholder that says "soon" is more honest than a list that pretends PDO has
 * one tour. The welcome modal and Settings render it greyed and unstartable.
 */
export const TOUR_CATALOG: TourCatalogEntry[] = [
  {
    id: FIRST_PIPELINE_TOUR.id,
    title: FIRST_PIPELINE_TOUR.title,
    blurb: FIRST_PIPELINE_TOUR.blurb,
    minutes: FIRST_PIPELINE_TOUR.minutes,
    def: FIRST_PIPELINE_TOUR,
  },
  {
    id: "first-run",
    title: "First run",
    blurb: "Launch a Run on a one-node pipeline and talk to the node in its terminal.",
    minutes: null,
    def: null,
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
 * What « Full tour » starts. It chains every available tour, which today is the
 * single one that exists — so the button starts *First pipeline* rather than
 * being hidden or lying about a second leg (#823: « le bouton Full tour lance ce
 * seul tour pour l'instant »).
 */
export function fullTourSequence(): TourDef[] {
  return availableTours();
}

/** Combined estimate for the full tour, counting only the tours that exist. */
export function fullTourMinutes(): number {
  return availableTours().reduce((total, tour) => total + tour.minutes, 0);
}
