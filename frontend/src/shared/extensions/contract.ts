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

// The Action locations (amendment #30): a bar or a menu, an entry per action.
// Each hands over what is at stake where it sits.

/** A Run, as a row of the Runs list shows it (`run/item/menu`). */
export interface RunEntity extends EntityContext {
  kind: "run";
  name: string | null;
  pipeline: string;
  status: string;
}

/** A Trigger, as a row of the Triggers tab shows it (`trigger/item/menu`). */
export interface TriggerEntity extends EntityContext {
  kind: "trigger";
  name: string;
  pipeline: string;
  enabled: boolean;
}

/** A Pipeline: a row of the Pipelines tab, or an editor tab (`pipeline/item/menu`, `tab/menu`). */
export interface PipelineEntity extends EntityContext {
  kind: "pipeline";
  name: string;
}

/** A node: of the canvas (`canvas/node/menu`) or of a Run's detail (`node/toolbar`). */
export interface NodeEntity extends EntityContext {
  kind: "node";
  name: string | null;
  /** `agent`, `script`, `merge`… on the canvas; `null` where unknown. */
  type: string | null;
  /** In a Run: the node's status; `null` on a pipeline template. */
  status: string | null;
}

/** An edge of the canvas (`canvas/edge/menu`); its id is `<from node>.<port>-><to node>.<port>`. */
export interface EdgeEntity extends EntityContext {
  kind: "edge";
  from: string;
  to: string;
}

/** A file of the Review page (`review/file`); its id is its path. */
export interface FileEntity extends EntityContext {
  kind: "file";
  path: string;
  /** `added`, `modified`, `deleted` or `renamed`. */
  status: string;
}

/** A skill or a folder of the skill bank (`skill/menu`). */
export interface SkillEntity extends EntityContext {
  kind: "skill" | "skill_folder";
  name: string;
}

/** An item's menu or bar: the item it acts on. */
export interface ItemContext<E extends EntityContext = EntityContext> extends LocationContext {
  entity: E;
}

/** The multi-selection bar (`runs/bulk`): the ids of the selected items. */
export interface SelectionContext extends LocationContext {
  selection: { kind: "run"; ids: string[] };
}

/** The pipeline a canvas shows; `run` is the Run of a Run's canvas, else `null`. */
export interface CanvasPipeline {
  id: string;
  name: string;
  run: string | null;
}

/** A menu of the canvas (`canvas/node/menu`, `canvas/edge/menu`). */
export interface CanvasItemContext<E extends EntityContext> extends ItemContext<E> {
  pipeline: CanvasPipeline;
}

/** The pair of Run refs the Review page compares (`review/toolbar`, `review/file`). */
export interface ReviewPair {
  from: string;
  to: string;
}

export interface ReviewContext extends LocationContext {
  review: ReviewPair;
}

export interface ReviewFileContext extends ReviewContext {
  entity: FileEntity;
}

/** What each Action location hands over, beyond the selected Run and its Projet. */
export interface ActionContexts {
  "topbar/end": LocationContext;
  "runs/header": LocationContext;
  "run/item/menu": ItemContext<RunEntity>;
  "runs/bulk": SelectionContext;
  "run/toolbar": LocationContext;
  "canvas/node/menu": CanvasItemContext<NodeEntity>;
  "canvas/edge/menu": CanvasItemContext<EdgeEntity>;
  "node/toolbar": ItemContext<NodeEntity>;
  "trigger/item/menu": ItemContext<TriggerEntity>;
  "pipeline/item/menu": ItemContext<PipelineEntity>;
  "pipelines/header": LocationContext;
  "tab/menu": ItemContext<PipelineEntity>;
  "review/toolbar": ReviewContext;
  "review/file": ReviewFileContext;
  "skill/menu": ItemContext<SkillEntity>;
}

export type ActionLocation = keyof ActionContexts;

/** What an Action location adds to the base context: its `extra`. */
export type ActionExtra<L extends ActionLocation> = Omit<ActionContexts[L], "location" | "run" | "project"> &
  Partial<Pick<LocationContext, "run" | "project">>;

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
