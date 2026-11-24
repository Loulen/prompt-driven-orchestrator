import { useEffect } from "react";

/**
 * CORE-281: while `active`, a reload or a close of the browser tab asks the
 * browser's own « Leave site? » alert (browsers ignore any custom text). The
 * listener is only attached while something is losable, so the page stays
 * eligible for the back/forward cache the rest of the time.
 */
export function useBeforeUnloadGuard(active: boolean): void {
  useEffect(() => {
    if (!active) return;
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      e.preventDefault();
      // Older browsers only show the alert when `returnValue` is set.
      e.returnValue = "";
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [active]);
}
