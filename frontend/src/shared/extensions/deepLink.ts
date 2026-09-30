/**
 * The deep links of the extensions' pages (ADR-0081, amendment #29):
 * `#/ext/<extension>/<location>/<path>` opens the extension's page at
 * `<location>` (its first one there, in declaration order) and hands `<path>`
 * to its component. The open page and the address bar follow each other: an
 * opened page writes its link, a link typed, clicked or brought back by
 * « Back » opens its page.
 */

import { create } from "zustand";
import type { UiLocation } from "../generated/UiLocation";

/** The locations whose elements are pages, hence reachable by deep link. */
export const PAGE_LOCATIONS = ["topbar/start", "topbar/end"] as const satisfies readonly UiLocation[];
export type PageLocation = (typeof PAGE_LOCATIONS)[number];

export const DEEP_LINK_PREFIX = "#/ext/";

export interface DeepLink {
  extension: string;
  /** `null` when what follows the extension's id names no page location. */
  location: PageLocation | null;
  /** What follows the location; for an unknown location, everything after the id. */
  path: string;
}

/** Whether `href` is a deep link (`#/ext/…`, `/#/ext/…`). */
export function isDeepLink(href: string | null | undefined): href is string {
  return typeof href === "string" && (href.startsWith(DEEP_LINK_PREFIX) || href.startsWith(`/${DEEP_LINK_PREFIX}`));
}

/** The deep link `href` (or a hash, or a URL ending with one) stands for; `null` if none. */
export function parseDeepLink(href: string): DeepLink | null {
  const at = href.indexOf(DEEP_LINK_PREFIX);
  if (at < 0) return null;
  const rest = decodeURI(href.slice(at + DEEP_LINK_PREFIX.length));
  const slash = rest.indexOf("/");
  const extension = slash < 0 ? rest : rest.slice(0, slash);
  if (!extension) return null;
  const after = slash < 0 ? "" : rest.slice(slash + 1);
  const location =
    PAGE_LOCATIONS.find((l) => after === l || after.startsWith(`${l}/`)) ?? null;
  const path = location ? after.slice(location.length + 1) : after;
  return { extension, location, path };
}

/** The deep link of `path` in the page of `extension` at `location`. */
export function formatDeepLink(extension: string, location: PageLocation, path = ""): string {
  const trimmed = path.replace(/^\/+/, "");
  return encodeURI(`${DEEP_LINK_PREFIX}${extension}/${location}${trimmed ? `/${trimmed}` : ""}`);
}

/** The page on screen. */
export interface OpenPage {
  /** Its contribution: `<extension id>:<contribution id>`. */
  key: string;
  extension: string;
  location: PageLocation;
  path: string;
}

interface PagesState {
  /** A deep link waiting for the extensions to load before it opens. */
  requested: DeepLink | null;
  open: OpenPage | null;
}

export const usePages = create<PagesState>(() => ({ requested: null, open: null }));

/** Point the address bar at `link`, or at no page; a new history entry. */
function writeAddress(link: string | null): void {
  const { pathname, search, hash } = window.location;
  const next = link ?? `${pathname}${search}`;
  if ((link ?? "") === hash) return;
  window.history.pushState(window.history.state, "", next);
}

/** Open the page `page`, and write its deep link. */
export function openPage(page: OpenPage): void {
  usePages.setState({ open: page, requested: null });
  writeAddress(formatDeepLink(page.extension, page.location, page.path));
}

/** Show another path of the open page. */
export function navigatePage(path: string): void {
  const open = usePages.getState().open;
  if (!open) return;
  openPage({ ...open, path: path.replace(/^\/+/, "") });
}

/** Close the page on screen, and leave its deep link. */
export function closePage(): void {
  usePages.setState({ open: null, requested: null });
  writeAddress(null);
}

/** Forget any page and request (tests). */
export function resetPages(): void {
  usePages.setState({ open: null, requested: null });
}

/** Follow the address bar: the page its deep link names, or none. */
export function syncWithAddress(): void {
  const link = parseDeepLink(window.location.hash);
  const { open } = usePages.getState();
  if (!link) {
    if (open) usePages.setState({ open: null });
    return;
  }
  if (
    open &&
    open.extension === link.extension &&
    open.location === link.location &&
    open.path === link.path
  ) {
    return;
  }
  usePages.setState({ requested: link });
}

let hostsMounted = 0;

/** Count the mounted page hosts: a deep link followed without one reloads the app on it. */
export function registerPageHost(): () => void {
  hostsMounted += 1;
  return () => {
    hostsMounted -= 1;
  };
}

/**
 * Follow the deep link `href`: in the app, its page opens; elsewhere (the
 * Review page), the app loads on it.
 */
export function followDeepLink(href: string): void {
  const at = href.indexOf(DEEP_LINK_PREFIX);
  if (at < 0) return;
  const hash = href.slice(at);
  if (hostsMounted > 0) {
    if (window.location.hash === hash) syncWithAddress();
    else window.location.hash = hash;
  } else {
    window.location.assign(`/${hash}`);
  }
}
