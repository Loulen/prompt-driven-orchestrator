// The screen guard (#883, grilling Q34): a demo instance lives under a
// throwaway `/tmp/pdo-readme-media-*` root, and no GIF may show it. The paths a
// visitor reads are those of a real machine (`~/code/shop-app`,
// `~/code/qa-skills`).
//
// The recorder samples the filmed DOM while a variant plays: every text node
// and form value that holds a forbidden path AND is drawn inside the crop.
// `assertCleanScreen` then fails the variant when a sample falls in a window
// the GIF keeps (a marker, a `keep`, a `fast`, a `hold`, the poster). A path in
// a cut stretch, or outside the crop, is never on screen: it passes.
//
// Generic: every scene gets it through the recorder, nothing to declare.

/** What never shows in a GIF: the demo root, whatever its random suffix. */
export const FORBIDDEN_ON_SCREEN = ["/tmp/pdo-readme-media"];

/**
 * In `page`, the forbidden strings drawn inside `crop` (viewport px): text
 * nodes (by their rendered boxes) and the values of inputs and textareas.
 * Returns the offending snippets, `[]` when the screen is clean. Pure DOM read.
 */
export async function forbiddenOnScreen(page, { crop, needles = FORBIDDEN_ON_SCREEN }) {
  return page.evaluate(
    ({ crop, needles }) => {
      const inCrop = (r) =>
        r.width > 0 && r.height > 0 && r.right > crop.x && r.left < crop.x + crop.width && r.bottom > crop.y && r.top < crop.y + crop.height;
      const drawn = (el) => {
        for (let e = el; e && e.nodeType === 1; e = e.parentElement) {
          const s = getComputedStyle(e);
          if (s.visibility === "hidden" || s.display === "none" || Number(s.opacity) === 0) return false;
        }
        return true;
      };
      const snippet = (text, at) => text.slice(Math.max(0, at - 20), at + 60).replace(/\s+/g, " ").trim();
      const hits = [];
      const walker = document.createTreeWalker(document.body ?? document.documentElement, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const text = node.nodeValue ?? "";
        for (const needle of needles) {
          const at = text.indexOf(needle);
          if (at < 0 || !drawn(node.parentElement)) continue;
          const range = document.createRange();
          range.setStart(node, at);
          range.setEnd(node, Math.min(text.length, at + needle.length));
          if ([...range.getClientRects()].some(inCrop)) hits.push(snippet(text, at));
        }
      }
      for (const field of document.querySelectorAll("input, textarea")) {
        const value = field.value ?? "";
        for (const needle of needles) {
          const at = value.indexOf(needle);
          if (at >= 0 && drawn(field) && inCrop(field.getBoundingClientRect())) hits.push(snippet(value, at));
        }
      }
      return hits;
    },
    { crop, needles },
  );
}

/** The windows of recording time the GIF plays: marker windows and keeps
 *  (holds included) at normal speed, fast stretches fast-forwarded. */
function filmedWindows({ keeps = [], fasts = [] }) {
  return [...keeps, ...fasts];
}

/**
 * Throw when a sample of `timeline.screen` (`[{ t, hits }]`, recording ms)
 * falls in a window the GIF keeps. The message names the moment and the text.
 */
export function assertCleanScreen(timeline) {
  const windows = filmedWindows(timeline);
  const filmed = (timeline.screen ?? []).filter((s) => s.hits.length > 0 && windows.some((w) => s.t >= w.start && s.t <= w.end));
  if (filmed.length === 0) return;
  const first = filmed[0];
  throw new Error(
    `a demo path is on screen (${FORBIDDEN_ON_SCREEN.join(", ")}) at ${(first.t / 1000).toFixed(1)} s of recording: « ${first.hits[0]} »` +
      (filmed.length > 1 ? ` (and ${filmed.length - 1} more sample(s))` : "") +
      ". Show a realistic path (~/code/...) or crop it out.",
  );
}
