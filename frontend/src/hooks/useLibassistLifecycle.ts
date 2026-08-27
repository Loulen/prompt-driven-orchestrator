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
 * Two separate questions, deliberately not the same argument:
 *
 * - **`activeId`** — *which* template the assistant should talk about. The active
 *   edit tab, or `null` while the user is looking at a Run.
 * - **`hasEditView`** — *whether anyone is still editing at all*, i.e. whether
 *   any template tab remains open. This, and only this, decides the reap.
 *
 * Collapsing the two into one argument is what ADR-0051 §4 calls "leaving every
 * edit view", and the first cut got it wrong: it reaped on the *active* tab, so
 * glancing at a Run with two templates still open threw the conversation away —
 * the exact regret this issue exists to remove. While templates stay open the
 * focus keeps naming the last one edited: it is still what the user would be
 * talking about, and the Assistant tab is not offered on a Run anyway.
 *
 * Three effects, one fact each:
 * - while a template tab is open, declare the focus and keep declaring it (the
 *   daemon reads it both as *what is open* and as *someone is still here*);
 * - when the last template tab **closes**, reap (the `DELETE` clears the focus
 *   daemon-side, so leaving is one gesture, not two that can diverge);
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
  activeId: string | null,
  activeScope: string | undefined,
  hasEditView: boolean,
): void {
  // Whether this page has had an edit view open at all. Written only inside
  // effects (a ref write during render is a React anti-pattern the lint rejects).
  const hasEdited = useRef(false);
  // The last template the user actually had in front of them, kept for the
  // stretch where they are on a Run with templates still open. Declaring nothing
  // there would let the idle TTL reap a session they are coming straight back to.
  // Written inside the effect below, never during render — and the effect re-runs
  // on every `activeId` change, so it is never read stale.
  const lastEdited = useRef<{ id: string; scope?: string } | null>(null);

  useEffect(() => {
    if (!hasEditView) return;
    if (activeId) lastEdited.current = { id: activeId, scope: activeScope };
    const target = lastEdited.current;
    if (!target) return;
    hasEdited.current = true;

    const declare = () => {
      void putLibassistFocus(target.id, target.scope).catch(() => {});
    };
    declare();
    const timer = setInterval(declare, FOCUS_HEARTBEAT_MS);
    return () => clearInterval(timer);
  }, [hasEditView, activeId, activeScope]);

  useEffect(() => {
    if (hasEditView || !hasEdited.current) return;
    hasEdited.current = false;
    lastEdited.current = null;
    // The last edit view closed. One call: the daemon clears the focus as part of
    // the reap, so there is no window where a stale focus outlives the session.
    void closeLibraryAssistant().catch(() => {});
  }, [hasEditView]);

  useEffect(() => {
    const onPageHide = () => {
      if (!hasEdited.current) return;
      void closeLibraryAssistant({ keepalive: true }).catch(() => {});
    };
    window.addEventListener("pagehide", onPageHide);
    return () => window.removeEventListener("pagehide", onPageHide);
  }, []);
}
