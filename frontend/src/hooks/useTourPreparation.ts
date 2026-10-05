/**
 * The intro card's preparations (#824), run by `useTour` while its card is up.
 *
 * Every item of the intro runs in parallel the moment the card opens, and `Start`
 * waits for all of them. Idempotent by contract, so a Retry — or a replayed tour —
 * costs nothing when the work is already done. `prepAttempt` is `useTour`'s retry
 * counter: bumping it re-runs the effect.
 *
 * Two handles go back to `useTour` (#911): the flight, so a teardown played
 * while the preparations still run waits for them, and the abort, so leaving the
 * card stops the one preparation that polls (a Run until `completed`).
 */

import { useEffect, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { TourIntro } from "../lib/tourDef";
import type { TourPrepView } from "./tour/tourView";

export function useTourPreparation(
  intro: TourIntro | null,
  prepAttempt: number,
  setPrep: Dispatch<SetStateAction<TourPrepView | null>>,
  prepFlightRef: MutableRefObject<Promise<unknown> | null>,
  prepAbortRef: MutableRefObject<AbortController | null>,
): void {
  useEffect(() => {
    if (!intro) return;
    let cancelled = false;
    const abort = new AbortController();
    prepAbortRef.current = abort;
    const done = new Set<string>();
    const render = (failure: TourPrepView["failure"]) => {
      if (cancelled) return;
      setPrep({
        items: intro.prepare.map((p) => ({
          id: p.id,
          label: done.has(p.id) ? p.ready : p.pending,
          done: done.has(p.id),
        })),
        ready: done.size === intro.prepare.length,
        failure,
      });
    };
    render(null);
    const flight = Promise.allSettled(
      intro.prepare.map((item) =>
        item
          .run(abort.signal)
          .then(() => {
            done.add(item.id);
            render(null);
          })
          .catch((e: unknown) => {
            // The daemon's sentence, quoted. The first refusal wins the card:
            // once it is showing a reason, a second one would only overwrite it.
            if (cancelled) return;
            setPrep((current) =>
              current?.failure
                ? current
                : {
                    items: current?.items ?? [],
                    ready: false,
                    failure: {
                      title: item.failureTitle,
                      reason: e instanceof Error ? e.message : String(e),
                    },
                  },
            );
          }),
      ),
    );
    prepFlightRef.current = flight;
    return () => {
      cancelled = true;
      abort.abort();
    };
  }, [intro, prepAttempt, setPrep, prepFlightRef, prepAbortRef]);
}
