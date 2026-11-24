/**
 * Binds a button to its mutation: `pending` while the work runs, for the
 * `pending` prop of `Button` / `SplitButton` (a spinner, the present tense,
 * inert). The Opération en cours itself (the status bar, the outcome's toast)
 * is the HTTP client's, or `trackOperation`'s.
 */
import * as React from "react";

export interface Pending {
  /** The work started by `run` has not settled yet. */
  pending: boolean;
  /**
   * Run `work` (a promise, or a function that starts one) and stay `pending`
   * until it settles. A second call while pending does nothing and resolves
   * `undefined`. Hands back the result; a rejection passes through unchanged.
   */
  run: <T>(work: Promise<T> | (() => Promise<T>)) => Promise<T | undefined>;
}

export function usePending(): Pending {
  const [pending, setPending] = React.useState(false);
  const busy = React.useRef(false);
  const run = React.useCallback(async <T>(work: Promise<T> | (() => Promise<T>)): Promise<T | undefined> => {
    if (busy.current) return undefined;
    busy.current = true;
    setPending(true);
    try {
      return await (typeof work === "function" ? work() : work);
    } finally {
      busy.current = false;
      setPending(false);
    }
  }, []);
  return { pending, run };
}
