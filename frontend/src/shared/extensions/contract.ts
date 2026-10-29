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
import type { Actor } from "../generated/Actor";
import type { TriggerSuspension } from "../generated/TriggerSuspension";
import type { Attributes } from "./attributes";
import type { BaseContext, ProjectContext } from "./context";
import type { PaletteContext } from "./paletteContract";

export type { BaseContext, ProjectContext, RunContext } from "./context";
// API 0.14: the sources of completion of `review/composer`.
export type * from "./completionContract";
// API 0.18: the command field, `palette`, and its Sources de recherche.
export type * from "./paletteContract";

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

/** A page (`topbar/start`, `topbar/end`, `settings/page`): the path of its deep link. */
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

/**
 * The period the Stats surface shows (`stats/tab`): the preset of its header,
 * its window (ISO instants, `to` excluded) and the bucket the core's charts
 * fold by.
 */
export interface StatsPeriod {
  preset: "7d" | "30d" | "90d" | "all";
  from: string;
  to: string;
  bucket: "day" | "week" | "month";
}

/** `stats/tab`: the period on screen, and the count of the header's Refresh. */
export interface StatsTabContext extends LocationContext {
  period: StatsPeriod;
  /** Bumped by each click on Refresh: a tab reloads what it shows when it changes. */
  refresh: number;
}

export type StatsTabProps = ContributionProps<StatsTabContext>;

/**
 * How a section takes part in its host form's Save and Cancel (`project/edit`,
 * `run/new`): a section keeps its changes as a draft, and applies them only
 * once the host saved. Each registration returns its unregistration; a
 * section's handlers are dropped when it unmounts.
 */
export interface SectionForm {
  /**
   * Run `handler` once the host saved, with what it saved (its id is known
   * then, even for an entity the form just created). The host waits for every
   * handler, in the sections' order, before it closes; a throw or a rejection
   * keeps the form open and shows its message (`run/new`, whose Run or Trigger
   * already exists, closes and shows it in an error toast).
   */
  onSave(handler: (saved: EntityContext) => unknown): () => void;
  /** Run `handler` when the host is cancelled or closed without saving. */
  onCancel(handler: () => unknown): () => void;
  /**
   * Provide a value to the creation the host is about to submit (API 0.9):
   * `handler` is called just before the submit, and what it answers (or
   * resolves to) is sent under the id of the section's extension, as
   * `POST /runs` `extensions.<id>`; the daemon hands it to that extension's
   * `validate_create` then `on_create_with_data`. `undefined` provides
   * nothing. A throw or a rejection keeps the form open with its message; so
   * does the extension's refusal of the value. Two sections of one extension
   * that both provide an object are merged, key by key. Only `run/new`, and
   * only when it launches a Run (not a Trigger): elsewhere it is never called.
   */
  provide?(handler: () => unknown): () => void;
}

/** A section: `settings`, `project/edit`, `run/new`, `node/config`, `pipeline/config`, `run/info`. */
export interface SectionProps<C extends LocationContext = LocationContext> extends ContributionProps<C> {
  /** The entity the host shows or edits, when it has one (as in `context.entity`). */
  entity?: EditedEntity | EntityContext;
  /** Present where the host has a Save and a Cancel (`project/edit`, `run/new`). */
  form?: SectionForm;
}

/**
 * The entity an editor edits: `id` is `null` while it is being created (the
 * id exists once the host saved; `SectionForm.onSave` receives it).
 */
export interface EditedEntity {
  kind: string;
  id: string | null;
  name: string;
  attributes: Attributes;
}

/** `project/edit`: the Projet being named or edited. */
export interface ProjectEditContext extends LocationContext {
  entity: EditedEntity;
  /** The editor names a new Projet (no id before Save). */
  creating: boolean;
}

/** `run/new`: what the New Run form is about to launch (or the Trigger it saves). */
export interface RunNewContext extends LocationContext {
  mode: "run" | "trigger";
  /** The pipeline picked, by id; `null` before one is. */
  pipeline: string | null;
  /** The target repository, as typed. */
  repository: string;
  /** The prompt of the entry node, as typed (API 0.9). */
  input: string;
  /** The Run's name, as typed (`""`: none); the Trigger's name in `trigger` mode (API 0.9). */
  name: string;
  /**
   * The Projet owning the target repository (compared verbatim, as the Runs
   * list groups it), whose rights the Run will be created under; `null` for a
   * repository no Projet owns, or none typed yet (CORE-5).
   */
  targetProject: ProjectContext | null;
  /** The Trigger being edited, when the form edits one. */
  trigger: string | null;
}

/** `node/config`: the node of a pipeline the inspector shows. */
export interface NodeConfigContext extends LocationContext {
  entity: EntityContext;
  pipeline: { id: string; name: string };
  /** The inspector shows a node of a Run: nothing may change. */
  readOnly: boolean;
}

/** `pipeline/config`: the template the Info tab of Pipeline info shows. */
export interface PipelineConfigContext extends LocationContext {
  entity: EntityContext;
}

/** `run/info`: the Run the Info tab shows. */
export interface RunInfoContext extends LocationContext {
  entity: RunInfoEntity;
}

/** The Run of `run/info`: who created it and, when a Trigger fired it, the
 *  Trigger's last editor (API 0.8); `null` when unknown. */
export interface RunInfoEntity extends EntityContext {
  created_by: Actor | null;
  trigger_editor: Actor | null;
}

/** A list filter (`runs/filter`, `triggers/filter`, `pipelines/filter`, `projects/filter`): the attribute filter in force. */
export interface ListFilterContext extends LocationContext {
  /** `run`, `trigger`, `pipeline` or `project`: what the filter keeps. */
  resource: string;
  filter: Attributes;
}

export interface ListFilterProps extends ContributionProps<ListFilterContext> {
  resource: string;
  /** The contribution's declared label (#31): the control's caption. */
  label: string;
  /** The attribute filter in force: prefixed key → value. */
  filter: Attributes;
  setFilter: (next: Attributes) => void;
}

/** The entity a detail, a row or a card shows. */
export interface EntityContext {
  /** `run`, `node`, `trigger`, `skill`, `pipeline`, `project`, `port`, `file`. */
  kind: string;
  id: string;
  /** Its attributes; `{}` for an entity that carries none (a node, a skill). */
  attributes: Attributes;
  /** What the interface calls it, when it has a name. */
  name?: string | null;
}

/** A tab (`run/detail`, `node/detail`, `trigger/detail`, `skill/detail`, `session/detail`): the entity it details. */
export interface TabContext extends LocationContext {
  entity: EntityContext;
}

export interface TabProps extends ContributionProps<TabContext> {
  entity: EntityContext;
}

/**
 * A decoration (`run/item`, `run/item/people`, `trigger/item`, `pipeline/item`,
 * `canvas/node`, `port/item`, `review/file`, `statusbar`): the item it marks, `null` on the
 * status bar. A decoration sits inside its item (often a button): a clickable
 * one calls `event.stopPropagation()`.
 */
export interface DecorationContext extends LocationContext {
  entity: EntityContext | null;
}

/** `canvas/node`: the node card, and the pipeline it belongs to. */
export interface CanvasNodeContext extends DecorationContext {
  entity: EntityContext;
  node: { type: string; status: string };
}

/** `port/item`: a port of a node of a Run. */
export interface PortItemContext extends DecorationContext {
  entity: EntityContext;
  port: { run: string; node: string; name: string; direction: "input" | "output"; type: string };
}

export interface DecorationProps<C extends DecorationContext = DecorationContext>
  extends ContributionProps<C> {
  entity: EntityContext | null;
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
  /** Its last creation or edit (API 0.8); `null` when unknown. */
  last_editor: Actor | null;
  /** Why an extension suspends it (API 0.8); `null` when none does. */
  suspension: TriggerSuspension | null;
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
  /** `agent`, `script`, `start`… on the canvas; `null` where unknown. */
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

/**
 * `review/file`: a file of the Review page, for its actions (#30) and its
 * decorations (#31). `file.run` names the Run under review.
 */
export interface ReviewFileContext extends ReviewContext {
  entity: FileEntity;
  file: { run: string; path: string; status: string };
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
  palette: PaletteContext;
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
  /**
   * Follow a deep link: an extension's page (`#/ext/<extension>/<location>/<path>`)
   * or a Run (`#/runs/<run_id>`, which closes the open page and selects it).
   */
  open(link: string): void;
}

export type CommandFunction = (context: CommandContext, host: CommandHost) => unknown;

