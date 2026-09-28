import { useCallback, useState } from "react";
import { useTour } from "../hooks/useTour";
import { loadTourOffered, markTourOffered, shouldOfferWelcome } from "../lib/tourMemory";
import { findTour, fullTourSequence } from "../lib/tours";
import type { RunListEntry } from "../shared/generated/RunListEntry";
import type { RunState } from "../shared/generated/RunState";

/** The guided tours' controller, the welcome-modal rule and the two starts. */
export function useAppTour(runs: RunListEntry[], runsLoaded: boolean, selectedRun: RunState | null) {
  // #823 — guided tours. The controller lives here because two surfaces start a
  // tour (the welcome modal and Settings › Tutorials) and a tour outlives both.
  // The run list is passed in rather than fetched: the tour observes what the UI
  // already knows (ADR-0071 §3) — how many Runs there are, and which is newest.
  // #825 adds the selected Run's detail, for the same reason and on the same
  // terms: the reading steps need a node's status and its completion guard, and
  // the inspector is already polling both.
  const tour = useTour(runs, selectedRun);
  const [tourOffered, setTourOffered] = useState(loadTourOffered);
  const showWelcome = shouldOfferWelcome({
    offered: tourOffered,
    runsLoaded,
    runCount: runs.length,
  });
  const startTourById = useCallback(
    (tourId: string) => {
      // Starting a tour is also an answer to "have you been offered one?" — a
      // reader who replays from Settings must not meet the modal on next load.
      markTourOffered();
      setTourOffered(true);
      const def = findTour(tourId);
      if (def) tour.start(def);
    },
    [tour],
  );
  const startFullTour = useCallback(() => {
    markTourOffered();
    setTourOffered(true);
    // #824: two tours exist now, so « Full tour » finally means a sequence. The
    // rest of the chain rides along, and each end card's primary runs the next.
    const [first, ...rest] = fullTourSequence();
    if (first) tour.start(first, rest);
  }, [tour]);
  return { tour, showWelcome, setTourOffered, startTourById, startFullTour };
}
