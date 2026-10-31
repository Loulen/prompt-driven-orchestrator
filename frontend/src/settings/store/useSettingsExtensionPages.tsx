import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useSettingsPages } from "../../shared/extensions/catalogue";
import type { PageShell } from "../../shared/extensions/contract";
import { leaveSettingsPage, openPage, usePages } from "../../shared/extensions/deepLink";
import { Breadcrumb } from "../../shared/extensions/ExtensionPageHost";
import ExtensionIcon from "../../shared/extensions/ExtensionIcon";
import ExtensionSettingsPage from "../../shared/extensions/ExtensionSettingsPage";
import type { SettingsRailEntry } from "../components/settingsSurface/SettingsRail";

/** The page of `settings/page` the address bar names, if any. */
export function settingsPageOpen() {
  const open = usePages.getState().open;
  return open?.location === "settings/page" ? open : null;
}

/**
 * The pages of `settings/page` in the Settings surface (ADR-0081, amendment CORE-30): a
 * rail entry each (`<extension>:<id>`). The one on screen follows the deep link
 * (`usePages`): a link names it, its entry writes it, leaving it or closing the surface
 * (✕, Escape, Stats opening) removes it, reopening on one writes it back.
 */
export function useSettingsExtensionPages({
  open,
  category,
  setCategory,
  requestClose,
}: {
  open: boolean;
  category: string;
  setCategory: (id: string) => void;
  /** What the page's `shell.close()` does: the surface's guarded close. */
  requestClose: () => void;
}) {
  const pages = useSettingsPages();
  const [breadcrumb, setBreadcrumb] = useState<string[]>([]);
  const [actions, setActions] = useState<ReactNode>(null);
  const linked = usePages((s) => (s.open?.location === "settings/page" ? s.open : null));
  useEffect(
    () =>
      usePages.subscribe((state, previous) => {
        const page = state.open;
        if (page === previous.open || page?.location !== "settings/page") return;
        if (page.key !== previous.open?.key) {
          setBreadcrumb([]);
          setActions(null);
        }
        setCategory(page.key);
      }),
    [setCategory],
  );
  const active = pages.find((page) => page.key === category) ?? null;

  const requestCloseRef = useRef(requestClose);
  useEffect(() => {
    requestCloseRef.current = requestClose;
  });
  // What a page may do with the Settings header; the same object for the page's
  // lifetime, so it can sit in an effect's dependencies. The header keeps the
  // Settings' title: `setTitle` changes nothing there.
  const shell = useMemo<PageShell>(
    () => ({ setBreadcrumb, setActions, setTitle: () => {}, close: () => requestCloseRef.current() }),
    [],
  );

  const wasOpen = useRef(open);
  useEffect(() => {
    if (wasOpen.current && !open) leaveSettingsPage();
    if (!wasOpen.current && open && active && !linked) {
      openPage({ key: active.key, extension: active.extension.id, location: "settings/page", path: "" });
    }
    wasOpen.current = open;
  }, [open, active, linked]);

  /** Show the rail entry `id`: an extension's page writes its link, anything else leaves it. */
  const select = (id: string) => {
    setBreadcrumb([]);
    setActions(null);
    const page = pages.find((item) => item.key === id);
    if (page) {
      openPage({ key: page.key, extension: page.extension.id, location: "settings/page", path: "" });
    } else {
      leaveSettingsPage();
    }
    setCategory(id);
  };

  return {
    active,
    /** Their rail entries, after the core's and Extensions. */
    rail: pages.map(
      (page): SettingsRailEntry => ({
        id: page.key,
        label: page.contribution.label,
        icon: (
          <ExtensionIcon
            url={page.contribution.type === "page" ? page.contribution.icon : null}
            label={page.contribution.label}
          />
        ),
      }),
    ),
    select,
    /** The header's crumbs while a page is on screen. */
    headerExtras: active ? <Breadcrumb items={breadcrumb} /> : undefined,
    /** The header's actions: the page's, else `fallback`. */
    headerActions: (fallback: ReactNode) => (active && actions ? actions : fallback),
    content: active && (
      <ExtensionSettingsPage
        key={active.key}
        page={active}
        path={linked?.key === active.key ? linked.path : ""}
        shell={shell}
      />
    ),
  };
}
