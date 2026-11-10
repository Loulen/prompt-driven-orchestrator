import { Fragment, useEffect, useMemo, useState, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { dismissTransientOverlays } from "../../lib/overlays";
import FullWindowShell from "../ui/FullWindowShell";
import { toast } from "../ui/toast";
import { elementsAt, useLocationContext, type ContributionElement } from "./catalogue";
import type { PageContext, PageIcon, PageProps, PageShell } from "./contract";
import {
  closePage,
  formatDeepLink,
  navigatePage,
  openPage,
  registerPageHost,
  syncWithAddress,
  usePages,
  type OpenPage,
} from "./deepLink";
import ExtensionBoundary from "./ExtensionBoundary";
import ExtensionIcon from "./ExtensionIcon";
import { resolveDeepLink } from "./resolveDeepLink";
import { useExtensions, useExtensionsLoaded } from "./store";

/**
 * Where an extension's page shows, full-window, when its entry or its deep link
 * opens it (`#/ext/<extension>/<location>/<path>`). Follows the address bar; a
 * link that cannot open says why in an error toast, never a blank page.
 */
export default function ExtensionPageHost() {
  const extensions = useExtensions();
  const loaded = useExtensionsLoaded();
  const open = usePages((s) => s.open);
  const requested = usePages((s) => s.requested);

  useEffect(() => {
    const unregister = registerPageHost();
    syncWithAddress();
    window.addEventListener("hashchange", syncWithAddress);
    window.addEventListener("popstate", syncWithAddress);
    return () => {
      unregister();
      window.removeEventListener("hashchange", syncWithAddress);
      window.removeEventListener("popstate", syncWithAddress);
    };
  }, []);

  useEffect(() => {
    if (!requested || !loaded) return;
    const resolved = resolveDeepLink(requested, extensions);
    if ("page" in resolved) {
      // A link followed from an artifact viewer: the viewer gives way to the page.
      dismissTransientOverlays();
      usePages.setState({ open: resolved.page, requested: null });
      return;
    }
    toast(resolved.error, "error");
    const current = usePages.getState().open;
    usePages.setState({ requested: null });
    const { pathname, search } = window.location;
    const address = current
      ? formatDeepLink(current.extension, current.location, current.path)
      : `${pathname}${search}`;
    window.history.replaceState(window.history.state, "", address);
  }, [requested, loaded, extensions]);

  // A page of the Settings shows in the Settings surface, on its rail entry.
  if (!open || open.location === "settings/page") return null;
  const pages = elementsAt<PageProps>(extensions, open.location);
  const active = pages.find((p) => p.key === open.key);
  if (!active) return null;
  return <ExtensionPage key={active.key} open={open} active={active} pages={pages} />;
}

/** A page's crumbs, after its title in the shell's header. */
export function Breadcrumb({ items }: { items: string[] }) {
  if (items.length === 0) return null;
  return (
    <nav
      className="flex min-w-0 items-center gap-1 text-sm text-text-3"
      aria-label="Breadcrumb"
      data-testid="extension-page-breadcrumb"
    >
      {items.map((item, i) => (
        <Fragment key={i}>
          {i > 0 && <ChevronRight size={12} className="shrink-0 text-text-3" />}
          <span className={`truncate ${i === items.length - 1 ? "text-text-2" : ""}`}>{item}</span>
        </Fragment>
      ))}
    </nav>
  );
}

/** The title and the icon a page set for one of its routes (`shell.setTitle`). */
interface RouteHeader {
  path: string;
  title: string;
  icon?: PageIcon;
}

/** An icon of the header: an SVG the extension serves, or a component. */
function HeaderIcon({ icon, label }: { icon: PageIcon | null | undefined; label: string }) {
  if (icon && typeof icon !== "string") {
    const Icon = icon;
    return (
      <span className="text-primary" data-testid="extension-page-icon">
        <Icon size={14} strokeWidth={1.5} aria-hidden />
      </span>
    );
  }
  return (
    <span className="text-primary" data-testid="extension-page-icon">
      <ExtensionIcon url={icon} label={label} />
    </span>
  );
}

function ExtensionPage({
  open,
  active,
  pages,
}: {
  open: OpenPage;
  active: ContributionElement<PageProps>;
  pages: ContributionElement<PageProps>[];
}) {
  const [breadcrumb, setBreadcrumb] = useState<string[]>([]);
  const [actions, setActions] = useState<ReactNode>(null);
  const [routeHeader, setRouteHeader] = useState<RouteHeader | null>(null);
  const shell = useMemo<PageShell>(
    () => ({
      setBreadcrumb,
      setActions,
      // Kept for the path on screen when the page sets it: another route
      // shows the page's own until it sets its own.
      setTitle: (title, icon) => {
        const path = usePages.getState().open?.path ?? "";
        setRouteHeader(title === null ? null : { path, title, icon });
      },
      close: closePage,
    }),
    [],
  );
  const extra = useMemo(() => ({ path: open.path }), [open.path]);
  const context = useLocationContext<PageContext>(open.location, extra);
  const { Component, extension, contribution } = active;
  if (!Component) return null;
  const header = routeHeader?.path === open.path ? routeHeader : null;
  const label = header?.title ?? contribution.label;
  const declaredIcon = contribution.type === "page" ? contribution.icon : null;
  const body = (
    <ExtensionBoundary extensionId={extension.id}>
      <Component
        extension={{ id: extension.id, name: extension.name }}
        context={context}
        navigate={navigatePage}
        shell={shell}
      />
    </ExtensionBoundary>
  );
  const common = {
    title: label,
    headerExtras: <Breadcrumb items={breadcrumb} />,
    headerActions: actions,
    testId: "extension-page",
    onClose: closePage,
    closeLabel: `Close ${label}`,
    mainClassName: "min-h-0 min-w-0 flex-1 overflow-y-auto px-7 py-5",
  };
  if (open.location === "topbar/start") {
    // The 0.1 navigation page: every page of the location on the rail.
    return (
      <FullWindowShell
        {...common}
        titleIcon={header?.icon ? <HeaderIcon icon={header.icon} label={label} /> : undefined}
        rail={pages.map((p) => ({ id: p.key, label: p.contribution.label }))}
        activeRail={active.key}
        onRailChange={(key) => {
          const next = pages.find((p) => p.key === key);
          if (next) openPage({ key, extension: next.extension.id, location: "topbar/start", path: "" });
        }}
        railWidth={176}
        railAriaLabel="Extension pages"
        railTestIdPrefix="extension-page"
      >
        {body}
      </FullWindowShell>
    );
  }
  // A `topbar/end` page brings its own navigation: no rail.
  return (
    <FullWindowShell
      {...common}
      titleIcon={<HeaderIcon icon={header?.icon ?? declaredIcon} label={label} />}
      mainClassName="relative flex min-h-0 min-w-0 flex-1 overflow-hidden"
    >
      {body}
    </FullWindowShell>
  );
}
