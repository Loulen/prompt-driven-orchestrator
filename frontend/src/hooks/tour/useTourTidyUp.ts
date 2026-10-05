/**
 * The teardown half of `useTour` (#911, ADR-0071 §4): what a tour installed only
 * to be shown (the example Trigger of *Overview*) is put away at **any** exit —
 * its end card, its stop card, a quit — exactly once. The machine decides when
 * (`shouldTidyUp`); this hook plays it, and keeps the failure for the card that
 * is showing at that moment.
 */

import { useCallback, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { markTidiedUp, shouldTidyUp, type TourRun } from "../../lib/tour";
import type { TourDef } from "../../lib/tourDef";
import type { TourView } from "./tourView";

export interface TourTidyUpHandles {
  /** Commit a machine transition, playing the teardown when it lands on an exit. */
  commit: (def: TourDef, next: TourRun) => void;
  /** Leave a tour that is not over yet: tidy it up without a card to report on. */
  leave: () => void;
  tidyUpFailure: TourView["tidyUpFailure"];
  /** The preparations in flight, handed to `useTourPreparation`. */
  prepFlightRef: MutableRefObject<Promise<unknown> | null>;
  /** Aborts the preparations, handed to `useTourPreparation`. */
  prepAbortRef: MutableRefObject<AbortController | null>;
}

export function useTourTidyUp(
  tourRef: MutableRefObject<TourDef | null>,
  runRef: MutableRefObject<TourRun | null>,
  setRun: Dispatch<SetStateAction<TourRun | null>>,
): TourTidyUpHandles {
  // The preparations in flight, so a teardown played while they still run
  // waits for them: a Trigger created a beat AFTER it was deleted would be left
  // behind by the very quit that meant to put it away.
  const prepFlightRef = useRef<Promise<unknown> | null>(null);
  const prepAbortRef = useRef<AbortController | null>(null);
  // Bumped by every start and quit: a teardown that fails after the user has
  // moved on has no card left to say it on.
  const generation = useRef(0);
  const [tidyUpFailure, setTidyUpFailure] = useState<TourView["tidyUpFailure"]>(null);

  /**
   * Play a tour's teardown. Every item runs, even after one failed; the first
   * failure is reported when `report` holds and the card it belongs to is still
   * the one on screen. A quit reports nothing: there is no card, and the next
   * pass of the tour picks the object up again.
   */
  const tidyUp = useCallback((def: TourDef, report: boolean) => {
    const items = def.teardown ?? [];
    if (items.length === 0) return;
    const pending = prepFlightRef.current;
    const gen = generation.current;
    void (async () => {
      await pending;
      const results = await Promise.allSettled(items.map((item) => item.run()));
      const index = results.findIndex((r) => r.status === "rejected");
      if (!report || index < 0 || generation.current !== gen) return;
      const failed = results[index] as PromiseRejectedResult;
      const e = failed.reason as unknown;
      setTidyUpFailure({
        title: items[index].failureTitle,
        reason: e instanceof Error ? e.message : String(e),
      });
    })();
  }, []);

  /**
   * Every transition of the machine goes through here, so the teardown is
   * played the moment a tour reaches its end card or its stop card — once
   * (`shouldTidyUp` says when, `markTidiedUp` makes it once).
   */
  const commit = useCallback(
    (def: TourDef, nextRun: TourRun) => {
      let committed = nextRun;
      if (shouldTidyUp(def, committed)) {
        committed = markTidiedUp(committed);
        tidyUp(def, true);
      }
      runRef.current = committed;
      setRun(committed);
    },
    [tidyUp, runRef, setRun],
  );

  const leave = useCallback(() => {
    const def = tourRef.current;
    const current = runRef.current;
    if (def && current && shouldTidyUp(def, current, "quit")) {
      runRef.current = markTidiedUp(current);
      tidyUp(def, false);
    }
    prepAbortRef.current?.abort();
    prepAbortRef.current = null;
    generation.current += 1;
    setTidyUpFailure(null);
  }, [tidyUp, tourRef, runRef]);

  return { commit, leave, tidyUpFailure, prepFlightRef, prepAbortRef };
}
