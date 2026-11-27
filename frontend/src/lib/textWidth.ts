import { cssToken } from "./cssColor";

/**
 * The width a line of UI text takes when drawn, in CSS px, measured with the
 * canvas text metrics: no element is laid out, so a model can ask for it while
 * it places nodes React Flow has not drawn yet (CORE-347, a decorated marker
 * centred on the pill it replaces).
 *
 * The font is the UI's (`--font-ui`, body weight 450) at the size token given.
 * The canvas ignores `font-feature-settings`, so the width can differ from the
 * drawn text by a fraction of a pixel per glyph. `null` without an
 * `OffscreenCanvas` (jsdom): the caller falls back on its own estimate.
 */
export function uiTextWidth(text: string, size: `--${string}` = "--fs-sm"): number | null {
  const ctx = context();
  if (!ctx) return null;
  ctx.font = `450 ${cssToken(size, "12px")} ${cssToken("--font-ui", "sans-serif")}`;
  return ctx.measureText(text).width;
}

let shared: OffscreenCanvasRenderingContext2D | null | undefined;

function context(): OffscreenCanvasRenderingContext2D | null {
  if (shared === undefined) {
    shared = typeof OffscreenCanvas === "function" ? new OffscreenCanvas(1, 1).getContext("2d") : null;
  }
  return shared;
}

/** Forget the shared canvas, for a test that installs or removes `OffscreenCanvas`. */
export function resetTextWidth(): void {
  shared = undefined;
}
