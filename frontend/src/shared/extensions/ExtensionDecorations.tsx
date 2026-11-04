import { useMemo } from "react";
import { useAppearancePreview, useElementMarks } from "../../hooks/useElementMarks";
import { contributionElementId, SURFACE_OF_LOCATION } from "../../lib/appearance";
import type { UiLocation } from "../generated/UiLocation";
import { useContributions, useLocationContext } from "./catalogue";
import type { DecorationContext, DecorationProps, LocationContext } from "./contract";
import ExtensionBoundary from "./ExtensionBoundary";

/**
 * A decoration location (`run/item`, `port/item`, `statusbar`…): the marks
 * the extensions put on one item, side by side, each behind its own error
 * boundary. **Nothing at all without a contribution**, so an item without
 * decoration keeps its layout; with several, they wrap rather than push the
 * item's own text out.
 *
 * At a location of a surface with Éléments masquables (`run/item`…, see
 * `SURFACE_OF_LOCATION`), a contribution the Apparence hides is left out; a
 * sample preview of Settings › Appearance draws none.
 *
 * `extra` is what the location adds to the base context (`entity`, and `node`,
 * `port` or `file` where the location says so); memoise it.
 */
export default function ExtensionDecorations<C extends DecorationContext>({
  location,
  extra,
  className = "",
  block = false,
  nowrap = false,
}: {
  location: UiLocation;
  extra: Omit<C, keyof LocationContext>;
  className?: string;
  /** A row of its own (a `div`, `flex`) rather than a run of inline marks. */
  block?: boolean;
  /** On one line whatever the room: a list row, whose line ends in an ellipsis. */
  nowrap?: boolean;
}) {
  const context = useLocationContext<C>(location, extra);
  // `review/file` also takes actions (#30): those render as its buttons, not here.
  const all = useContributions<DecorationProps<C>>(location, context);
  const marksOf = useElementMarks();
  const preview = useAppearancePreview();
  const surface = SURFACE_OF_LOCATION[location];
  const decorations = useMemo(
    () =>
      all.flatMap((element) => {
        if (element.Component === null) return [];
        if (!surface) return [{ element, marks: {} }];
        const marks = marksOf(contributionElementId(surface, element.key));
        return marks ? [{ element, marks }] : [];
      }),
    [all, surface, marksOf],
  );
  if (decorations.length === 0 || (surface && preview?.sample)) return null;
  const Wrapper = block ? "div" : "span";
  return (
    <Wrapper
      className={`${block ? "flex" : "inline-flex"} min-w-0 ${nowrap ? "flex-nowrap" : "flex-wrap"} items-center gap-1 ${className}`}
      data-testid={`extension-decorations-${location}`}
    >
      {decorations.map(({ element: { key, extension, Component }, marks }) =>
        Component ? (
          <span
            key={key}
            className="inline-flex shrink-0 items-center"
            data-testid={`extension-decoration-${key}`}
            {...marks}
          >
            <ExtensionBoundary extensionId={extension.id} compact>
              <Component
                extension={{ id: extension.id, name: extension.name }}
                context={context}
                entity={context.entity}
              />
            </ExtensionBoundary>
          </span>
        ) : null,
      )}
    </Wrapper>
  );
}
