/**
 * Positions under the UI scale. The app is drawn with CSS `zoom`
 * (`lib/uiScale.ts`): a rect (`getBoundingClientRect`), a pointer
 * (`clientX`) and the window (`innerWidth`) are in screen pixels, while a
 * length written to `style` inside the app is multiplied by the zoom. Convert
 * with `toCss` before writing one.
 */

/**
 * The zoom the app is drawn at: the element's `currentCSSZoom`; in an engine
 * without it, the zoom of the UI scale painted on `<html>`; 1 outside a
 * browser.
 */
export function appZoom(el?: Element | null): number {
  if (typeof document === "undefined") return 1;
  const target = el ?? document.getElementById("root");
  const z = (target as { currentCSSZoom?: number } | null)?.currentCSSZoom;
  if (typeof z === "number" && z > 0) return z;
  if (target && z === undefined) {
    const painted = Number.parseFloat(document.documentElement.style.getPropertyValue("--app-zoom"));
    if (painted > 0 && target.closest("#root")) return painted;
  }
  return 1;
}

/** Screen pixels (a rect, a pointer, the window) to CSS pixels inside the app. */
export const toCss = (px: number, el?: Element | null): number => px / appZoom(el);

/** A screen rect (`getBoundingClientRect`) in CSS pixels inside the app. */
export function toCssRect<R extends { top: number; left: number; width: number; height: number }>(r: R): R {
  const z = appZoom();
  return { ...r, top: r.top / z, left: r.left / z, width: r.width / z, height: r.height / z };
}

/** The window's size in CSS pixels inside the app. */
export function cssViewport(): { width: number; height: number } {
  const z = appZoom();
  return { width: window.innerWidth / z, height: window.innerHeight / z };
}

/**
 * Where a floating layer is portalled: the app's root, which carries the
 * zoom, so the layer is drawn at the scale of the app. A floating-ui layer in
 * it uses absolute positioning, which reads the zoom off its offset parent.
 * `undefined` without one (a test): the library's default, the body.
 */
export function appRoot(): HTMLElement | undefined {
  return (typeof document !== "undefined" ? document.getElementById("root") : null) ?? undefined;
}
