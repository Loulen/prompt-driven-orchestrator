import { useLayoutEffect, useRef } from "react";
import { Banner } from "../ui/kit";
import { toCss } from "../ui/zoom";
import { useContributions, useLocationContext } from "./catalogue";
import type { DecorationContext, DecorationProps } from "./contract";
import ExtensionBoundary from "./ExtensionBoundary";

/** The application banner marks nothing in particular: no entity. */
const APP_BANNER_ITEM: Omit<DecorationContext, "location" | "run" | "project"> = { entity: null };

/** The CSS variable holding the banner's height, which the full-window surfaces start under. */
export const APP_BANNER_HEIGHT_VAR = "--h-appbanner";

/**
 * The `app/banner` location: full-width lines under the top bar, above every
 * screen, one per contribution in the catalogue's order, each behind its
 * extension's error boundary. The component draws itself (the kit's `Banner`)
 * and decides alone whether to show: **a contribution that renders nothing
 * takes no room** (its row is empty, hidden by `:empty`; when every row is,
 * the whole banner is hidden), and without a contribution nothing is rendered
 * at all. The person cannot dismiss it.
 *
 * Its height is published on `<html>` as `--h-appbanner`, so a full-window
 * surface (Settings, Stats, an extension's page) opens under the banner
 * rather than over it.
 */
export default function ExtensionAppBanner() {
  const context = useLocationContext<DecorationContext>("app/banner", APP_BANNER_ITEM);
  const banners = useContributions<DecorationProps<DecorationContext>>("app/banner", context).filter(
    (element) => element.Component !== null,
  );
  const ref = useRef<HTMLDivElement>(null);
  const shown = banners.length > 0;

  useLayoutEffect(() => {
    const el = ref.current;
    const root = document.documentElement;
    if (!el) return;
    // In CSS pixels inside the app: the surfaces that read it are drawn under the UI scale.
    const publish = () =>
      root.style.setProperty(APP_BANNER_HEIGHT_VAR, `${toCss(el.getBoundingClientRect().height, el)}px`);
    publish();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(publish);
    observer?.observe(el);
    return () => {
      observer?.disconnect();
      root.style.removeProperty(APP_BANNER_HEIGHT_VAR);
    };
  }, [shown]);

  if (!shown) return null;
  return (
    <div ref={ref} className="sh-appbanner" data-testid="extension-app-banner">
      {banners.map(({ key, extension, Component }) =>
        Component && (
          <div key={key} className="sh-appbanner__item" data-testid={`extension-app-banner-${key}`}>
            <ExtensionBoundary
              extensionId={extension.id}
              fallback={<Banner tone="failed" title={`The banner of the extension ${extension.name} failed`} />}
            >
              <Component
                extension={{ id: extension.id, name: extension.name }}
                context={context}
                entity={null}
              />
            </ExtensionBoundary>
          </div>
        ),
      )}
    </div>
  );
}
