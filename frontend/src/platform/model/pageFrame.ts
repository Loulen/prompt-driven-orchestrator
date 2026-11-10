/**
 * Moving the frame of a mounted page to a place a guide entry names. The page is
 * served by the daemon on this origin, so the frame's document is reachable.
 */

import { pageUrl } from "./mountedPage";

/**
 * Show `link` of the page in `frame`: an anchor scrolls to its element (the page is
 * same-origin), a relative path loads beneath the mount.
 */
export function showInFrame(frame: HTMLIFrameElement, name: string, link: string): void {
  const root = pageUrl("", name);
  if (link.startsWith("#")) {
    let anchor = link.slice(1);
    try {
      anchor = decodeURIComponent(anchor);
    } catch {
      // Not encoded: as written.
    }
    const doc = frame.contentDocument;
    const target = doc?.getElementById(anchor) ?? doc?.getElementsByName(anchor)[0] ?? null;
    if (target) {
      target.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }
    if (frame.contentWindow) frame.contentWindow.location.hash = link;
    return;
  }
  const base = new URL(root, window.location.origin);
  const next = new URL(link, base);
  frame.src = `${next.pathname}${next.search}${next.hash}`;
}
