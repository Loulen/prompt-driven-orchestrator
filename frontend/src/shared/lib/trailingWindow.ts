/**
 * Batch the calls of `fn` by window: the first call opens a window of `ms`,
 * the calls inside it add nothing, and `fn` runs once at its end. A burst of
 * events costs one refresh, and a steady stream one refresh per window (a
 * debounce would starve it).
 */
export interface TrailingWindow {
  (): void;
  /** Drop a pending call. */
  cancel(): void;
}

/** The window of the shell's refreshes on the live stream (ADR-0084). */
export const REFRESH_WINDOW_MS = 300;

export function trailingWindow(fn: () => void, ms: number = REFRESH_WINDOW_MS): TrailingWindow {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const call = () => {
    if (timer !== undefined) return;
    timer = setTimeout(() => {
      timer = undefined;
      fn();
    }, ms);
  };
  call.cancel = () => {
    clearTimeout(timer);
    timer = undefined;
  };
  return call;
}
