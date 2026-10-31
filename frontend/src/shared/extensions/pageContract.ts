/**
 * What a page (`topbar/start`, `topbar/end`, `settings/page`) and a panel of
 * `topbar/end` receive, and what a page may do with the shell around it. Part
 * of the contract of `contract.ts`, which re-exports them.
 */

import type { ReactNode } from "react";
import type { Icon } from "../ui/kit/tones";
import type { ContributionProps, LocationContext } from "./contract";

/** A page (`topbar/start`, `topbar/end`, `settings/page`): the path of its deep link. */
export interface PageContext extends LocationContext {
  /** What follows `#/ext/<extension>/<location>/`; `""` from its entry. */
  path: string;
}

/**
 * An icon of the shell's header: the URL of an SVG the extension serves (drawn
 * as a contribution's icon), or a component taking `size` and `strokeWidth`
 * (a `lucide-react` icon).
 */
export type PageIcon = string | Icon;

/** What a page may do with the full-window shell around it. */
export interface PageShell {
  /** The crumbs after the page's title, in the shell's header. */
  setBreadcrumb(items: string[]): void;
  /**
   * The title, and the icon, of the shell's header for the route on screen
   * (API 0.20): two routes of one page read as two screens. Without `icon`,
   * the page's own. It holds until the path changes: a route that sets none
   * shows the page's label and icon, and so does the page once reopened.
   * `null` restores them at once.
   */
  setTitle(title: string | null, icon?: PageIcon): void;
  /** Right-aligned header content, before ✕. */
  setActions(node: ReactNode): void;
  /** Close the page (as ✕ and Escape do). */
  close(): void;
}

export interface PageProps extends ContributionProps<PageContext> {
  /** Go to another path of the page: the deep link follows. */
  navigate(path: string): void;
  shell: PageShell;
}

/**
 * A panel of `topbar/end` (API 0.20), opened under its icon. Escape, a click
 * outside and the icon close it; a click in a toast does not.
 */
export interface PanelProps extends ContributionProps<LocationContext> {
  /** Close the panel (before following a link it offers, say). */
  close(): void;
}
