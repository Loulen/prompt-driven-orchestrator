/**
 * What an extension's components and commands receive, location by location
 * (ADR-0081, amendment #29). The reference page
 * (`docs/reference/extension-ui.md`) documents the same types for extension
 * authors; the catalogue itself (`UiLocation`) is generated from the API.
 *
 * Every component receives `extension` and `context`; `context` holds at least
 * the selected Run and its Projet, plus what the location puts at stake. The
 * props of the 0.1 slots (`entity`, `filter`, `setFilter`, `resource`) are still
 * passed.
 */

import type { ReactNode } from "react";
import type { UiLocation } from "../generated/UiLocation";
import type { Attributes } from "./attributes";
import type { BaseContext } from "./context";

export type { BaseContext, ProjectContext, RunContext } from "./context";

/** Who the element belongs to. */
export interface ExtensionRef {
  id: string;
  name: string;
}

/** The context of a location: at least the selected Run and its Projet. */
export interface LocationContext extends BaseContext {
  location: UiLocation;
}

export interface ContributionProps<C extends LocationContext = LocationContext> {
  extension: ExtensionRef;
  context: C;
}

/** A page (`topbar/start`, `topbar/end`): the path of its deep link. */
export interface PageContext extends LocationContext {
  /** What follows `#/ext/<extension>/<location>/`; `""` from its entry. */
  path: string;
}

/** What a page may do with the full-window shell around it. */
export interface PageShell {
  /** The crumbs after the page's title, in the shell's header. */
  setBreadcrumb(items: string[]): void;
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

/** A section (`settings`). */
export type SectionProps = ContributionProps;

/** A list filter (`runs/filter`): the attribute filter in force. */
export interface ListFilterContext extends LocationContext {
  resource: string;
  filter: Attributes;
}

export interface ListFilterProps extends ContributionProps<ListFilterContext> {
  resource: string;
  /** The attribute filter in force: prefixed key → value. */
  filter: Attributes;
  setFilter: (next: Attributes) => void;
}

/** The entity a detail shows. */
export interface EntityContext {
  kind: string;
  id: string;
  attributes: Attributes;
}

/** A tab (`run/detail`): the entity it details. */
export interface TabContext extends LocationContext {
  entity: EntityContext;
}

export interface TabProps extends ContributionProps<TabContext> {
  entity: EntityContext;
}

/** What a command is called with: its action's location context. */
export interface CommandContext extends LocationContext {
  extension: ExtensionRef;
  /** The id of the action that called it. */
  contribution: string;
}

/** What a command may do back. */
export interface CommandHost {
  /** Tell the operator, in a toast. */
  notify(message: string, kind?: "info" | "error"): void;
  /** Follow a deep link (`#/ext/<extension>/<location>/<path>`). */
  open(link: string): void;
}

export type CommandFunction = (context: CommandContext, host: CommandHost) => unknown;
