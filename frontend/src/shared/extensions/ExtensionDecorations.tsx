import type { UiLocation } from "../generated/UiLocation";
import { useContributions, useLocationContext } from "./catalogue";
import type { DecorationContext, DecorationProps, LocationContext } from "./contract";
import ExtensionBoundary from "./ExtensionBoundary";

/**
 * A decoration location (`run/item`, `canvas/node`, `statusbar`…): the marks
 * the extensions put on one item, side by side, each behind its own error
 * boundary. **Nothing at all without a contribution**, so an item without
 * decoration keeps its layout; with several, they wrap rather than push the
 * item's own text out.
 *
 * `extra` is what the location adds to the base context (`entity`, and `node`,
 * `port` or `file` where the location says so); memoise it.
 */
export default function ExtensionDecorations<C extends DecorationContext>({
  location,
  extra,
  className = "",
  block = false,
}: {
  location: UiLocation;
  extra: Omit<C, keyof LocationContext>;
  className?: string;
  /** A row of its own (a `div`, `flex`) rather than a run of inline marks. */
  block?: boolean;
}) {
  const context = useLocationContext<C>(location, extra);
  const decorations = useContributions<DecorationProps<C>>(location, context);
  if (decorations.length === 0) return null;
  const Wrapper = block ? "div" : "span";
  return (
    <Wrapper
      className={`${block ? "flex" : "inline-flex"} min-w-0 flex-wrap items-center gap-1 ${className}`}
      data-testid={`extension-decorations-${location}`}
    >
      {decorations.map(({ key, extension, Component }) =>
        Component ? (
          <span key={key} className="inline-flex shrink-0 items-center" data-testid={`extension-decoration-${key}`}>
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
