import { useEffect, useRef } from "react";
import { closeLibraryAssistant, putLibassistFocus } from "../api";

/**
 * How often the UI re-declares its focus while an edit view is open.
 *
 * Well under the daemon's 120 s idle TTL, and deliberately so: Chrome throttles a
 * background tab's timers to roughly one tick a minute, and a heartbeat that
 * missed the TTL would reap the assistant of a user who is merely on another tab.
 * A *frozen* tab (bfcache) stops timers entirely — but a frozen tab is nobody
 * looking, which is exactly the session the reaper should take.
 */
const FOCUS_HEARTBEAT_MS = 20_000;

/**
 * The library assistant's lifecycle, hung on the user rather than on a tab
 * (#594 / ADR-0051).
 *
 * `assistantId` is non-null exactly while a pipeline **template** edit view is
 * open. That predicate — not "is the Assistant tab showing" — is what the
 * assistant's life is worth: the info panel closes by itself on every edit-tab
 * switch (#385), so keying on the tab reaped the conversation each time the user
 * looked at another template, which is the complaint this issue is made of.
 *
 * Three effects, one fact each:
 * - while an edit view is open, declare the focus and keep declaring it (the
 *   daemon reads it both as *what is open* and as *someone is still here*);
 * - when the last edit view **closes**, clear the focus and reap;
 * - on `pagehide`, reap with `keepalive`. Not `beforeunload`: it does not fire
 *   reliably (a mobile tab discarded in the background never sees it), and React
 *   runs no effect cleanup on unload at all — which is precisely why the daemon's
 *   sweep is the real backstop and this is only the fast path.
 *
 * Both reaps require this page to have been editing at some point. The session is
 * shared by every browser tab pointed at the daemon, so an unconditional reap
 * would let a second tab sitting on the runs view kill the assistant of the tab
 * actually authoring a template. A page that never opened an editor has nothing
 * of its own to reap — and if it is a *reload* of a page that did, the leaked
 * session is the sweep's job (no `DELETE` survives an unload anyway).
 *
 * Everything is best-effort: a failed declaration must never surface as an error
 * in an editor the user is working in. The cost of losing one is bounded — the
 * next heartbeat re-declares, and the sweep catches whatever the `DELETE` misses.
 */
export function useLibassistLifecycle(
  assistantId: string | null,
  assistantScope?: string,
): void {
  // Whether this page has had an edit view open at all. Written only inside
  // effects (a ref write during render is a React anti-pattern the lint rejects).
  const hasEdited = useRef(false);

  useEffect(() => {
    if (!assistantId) return;
    hasEdited.current = true;

    const declare = () => {
      void putLibassistFocus(assistantId, assistantScope).catch(() => {});
    };
    declare();
    const timer = setInterval(declare, FOCUS_HEARTBEAT_MS);
    return () => clearInterval(timer);
  }, [assistantId, assistantScope]);

  useEffect(() => {
    if (assistantId || !hasEdited.current) return;
    hasEdited.current = false;
    // The last edit view closed: say so, then reap. Clearing the focus first
    // matters — a stale focus would keep the session alive for a whole TTL if the
    // DELETE were lost in flight.
    void putLibassistFocus(null).catch(() => {});
    void closeLibraryAssistant().catch(() => {});
  }, [assistantId]);

  useEffect(() => {
    const onPageHide = () => {
      if (!hasEdited.current) return;
      void closeLibraryAssistant({ keepalive: true }).catch(() => {});
    };
    window.addEventListener("pagehide", onPageHide);
    return () => window.removeEventListener("pagehide", onPageHide);
  }, []);
}
