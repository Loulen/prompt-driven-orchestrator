/**
 * How a surface renders its Éléments masquables (CONTEXT.md « Élément
 * masquable »): an element the Apparence hides is not rendered at all.
 *
 * Under a preview of Settings › Appearance ({@link AppearancePreview}), the
 * element its switch points to (hover or focus) is ringed, and comes back as a
 * ghost where it sits if it is hidden; a sample preview draws no extension mark.
 */
import { createContext, useCallback, useContext } from "react";
import { isVisible, type Appearance } from "../lib/appearance";
import { useAppearance } from "./useAppearance";

export interface AppearancePreview {
  /** The element id whose switch the pointer or the focus is on. */
  pointed: string | null;
  /** A sample, not one of the user's entities: no extension mark. */
  sample: boolean;
}

/** Set by a preview of Settings › Appearance; `null` everywhere else. */
export const AppearancePreviewContext = createContext<AppearancePreview | null>(null);

/** What an element rendered spreads on its root: the preview's marks, nothing in a real surface. */
export interface ElementMarks {
  "data-element"?: string;
  "data-pointed"?: true;
  "data-ghost"?: true;
}

const SHOWN: ElementMarks = {};

/**
 * The marks of the element `id`, or `null` when it is not rendered: hidden,
 * and not the element a preview points to.
 */
export function elementMarks(
  appearance: Appearance,
  preview: AppearancePreview | null,
  id: string,
): ElementMarks | null {
  const visible = isVisible(appearance, id);
  if (!preview) return visible ? SHOWN : null;
  const pointed = preview.pointed === id;
  if (!visible && !pointed) return null;
  return {
    "data-element": id,
    ...(pointed ? { "data-pointed": true as const } : {}),
    ...(visible ? {} : { "data-ghost": true as const }),
  };
}

/** {@link elementMarks} on the live Apparence and the enclosing preview. */
export function useElementMarks(): (id: string) => ElementMarks | null {
  const appearance = useAppearance();
  const preview = useContext(AppearancePreviewContext);
  return useCallback((id: string) => elementMarks(appearance, preview, id), [appearance, preview]);
}

/** The enclosing preview of Settings › Appearance, `null` in a real surface. */
export function useAppearancePreview(): AppearancePreview | null {
  return useContext(AppearancePreviewContext);
}
